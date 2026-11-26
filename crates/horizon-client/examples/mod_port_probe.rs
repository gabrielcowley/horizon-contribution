//! The Horizon side of a mod port's harness (tools/mod_port_harness.py,
//! docs/modpacks.md): builds the content root for carried-over mods and
//! writes what Horizon makes of every block state, drop, recipe and tag the
//! NeoForge oracle exported.
//!
//! cargo run -p horizon-client-lib --example mod_port_probe -- PREPARED WORK OUT.json [MOD...]
//! cargo run -p horizon-client-lib --example mod_port_probe -- --root ROOT PREPARED WORK OUT.json --mod DIR...
//!
//! WORK is a `horizon-port-pack` work folder: `oracle/facts.json` and
//! `mods/<id>/carry/`. The root is built in `WORK/horizon-root`.
//!
//! With `--root`, the probe builds nothing: ROOT is a content root the game
//! already built over PREPARED (its log names it: `content from N mod(s) in
//! ROOT`), so it holds what a native mod declares in code (`BlocksEvent`,
//! `ItemsEvent`). Each `--mod` is the folder of a package that root holds; the
//! probe reads its `data/` over the root as the game does, and writes the facts
//! of the namespaces it owns. WORK then needs only `oracle/facts.json`.
//!
//! Either way the loaders read the mods' data files through the pack overlay
//! the game builds from them: the root holds only what its builder generated.
extern crate horizon_client_lib as horizon_client;
use horizon_client::{
    Result,
    assets::Assets,
    blocks::BlockFacts,
    crafting::{Recipe, Recipes},
    enchant::Enchantments,
    items::{ItemStack, Items},
    mod_content::{self, ContentMod},
    stations::StationRecipes,
};
use horizon_core::{geometry::Axis, random::LegacyRandom};
use horizon_world::chunk::BlockState;
use horizon_world::loot::PackOverlay;
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, fs, path::Path};

fn boxes(list: impl IntoIterator<Item = [f64; 6]>) -> Value {
    let mut v: Vec<[f64; 6]> = list.into_iter().collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    json!(v)
}

fn sound(s: &horizon_client::blocks::Sounds) -> Value {
    json!({"volume": s.volume, "pitch": s.pitch, "break": &*s.break_sound, "step": &*s.step,
        "place": &*s.place, "hit": &*s.hit, "fall": &*s.fall})
}

fn tool(name: &str) -> Option<ItemStack> {
    match name {
        "hand" => None,
        "silk_touch_pickaxe" => {
            let mut pick = ItemStack::new("minecraft:diamond_pickaxe", 1);
            pick.enchant("minecraft:silk_touch", 1);
            Some(pick)
        }
        other => Some(ItemStack::new(&format!("minecraft:{other}"), 1)),
    }
}

/// An equipment asset's first humanoid layers as files under the root, as worn armour reads them.
fn equipment_textures(root: &Path, asset: &str) -> Value {
    let (ns, path) = asset.split_once(':').unwrap_or(("minecraft", asset));
    let Ok(doc) = fs::read(root.join(format!("assets/{ns}/equipment/{path}.json"))) else {
        return Value::Null;
    };
    let doc: Value = serde_json::from_slice(&doc).unwrap_or_default();
    let mut out = Map::new();
    for kind in ["humanoid", "humanoid_leggings"] {
        if let Some(texture) = doc["layers"][kind][0]["texture"].as_str() {
            let (tns, tpath) = texture.split_once(':').unwrap_or(("minecraft", texture));
            let file = root.join(format!(
                "assets/{tns}/textures/entity/equipment/{kind}/{tpath}.png"
            ));
            out.insert(kind.into(), json!(file.exists().then_some(file)));
        }
    }
    Value::Object(out)
}

fn main() -> Result<()> {
    let args = Args::parse(std::env::args().skip(1))?;
    let (work, out) = (Path::new(&args.work), args.out.as_str());
    let oracle: Value = serde_json::from_slice(&fs::read(work.join("oracle/facts.json"))?)?;
    let base = Path::new(&args.base);
    let jar_store = horizon_client::logging::game_home().join("jars");
    let began = std::time::Instant::now();
    let (root, mods) = match &args.source {
        Source::Built { root, mods } => (
            root.clone(),
            mods.iter()
                .map(|dir| read_mod(dir, true))
                .collect::<Result<Vec<_>>>()?,
        ),
        Source::Packages { wanted } => {
            let mods = carried(work, wanted)?;
            let root = work.join("horizon-root");
            let inventory = horizon_mod_package::enabled::EnabledMods::new(
                mods.iter().map(|(m, side)| (m.id.clone(), *side)),
            );
            let mods: Vec<ContentMod> = mods.into_iter().map(|(m, _)| m).collect();
            mod_content::build(base, &jar_store, &mods, &root, &inventory)
                .map_err(step("content root"))?;
            (root, mods)
        }
    };
    let built = began.elapsed().as_secs_f64();
    let files = data_overlay(base, &root, &jar_store, &mods, work).map_err(step("mod data"))?;
    let inventory = horizon_mod_package::enabled::EnabledMods::read(&root)
        .map_err(|e| format!("{}: enabled mods: {e}", root.display()))?;
    let conditions = horizon_world::conditions::LoadConditions::new(inventory.server_ids());
    let overlay = PackOverlay {
        conditions: &conditions,
        files: &files,
    };
    let namespaces: Vec<&str> = mods
        .iter()
        .flat_map(|m| m.content.namespaces.iter().map(String::as_str))
        .collect();
    let assets = Assets::load(&root).map_err(step("assets"))?;
    let registry = assets.registry;
    let facts = BlockFacts::load(&root, registry).map_err(step("block facts"))?;
    let defaults =
        horizon_world::item_defaults::read(&root).map_err(|error| format!("items: {error}"))?;
    let items = Items::load(&root, registry, &facts, &overlay, &defaults).map_err(step("items"))?;
    let recipes =
        Recipes::load(&root, &items, &overlay, &server_load()).map_err(step("recipes"))?;
    let stations = StationRecipes::load(&root, &items, &overlay, &server_load());
    let enchantments = Enchantments::load(&root, &defaults)?;
    let collisions =
        horizon_client::collisions::load(&root, registry).map_err(step("collisions"))?;
    let loaded = began.elapsed().as_secs_f64() - built;
    let ours = |id: &str| {
        id.split_once(':')
            .is_some_and(|(ns, _)| namespaces.contains(&ns))
    };
    let tools: Vec<String> = serde_json::from_value(oracle["tools"].clone())?;

    let mut blocks = Map::new();
    for (name, block) in oracle["blocks"].as_object().ok_or("oracle: no blocks")? {
        if !ours(name) {
            continue;
        }
        let Some(default) = registry.default_state(name) else {
            blocks.insert(name.clone(), Value::Null);
            continue;
        };
        let mut states = Map::new();
        for (key, o) in block["states"].as_object().into_iter().flatten() {
            let properties: BTreeMap<String, String> = key
                .split(',')
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.split_once('='))
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect();
            let descriptor = BlockState {
                name: name.clone(),
                properties: properties.into(),
            };
            let Some(id) = registry.id(&descriptor) else {
                states.insert(key.clone(), Value::Null);
                continue;
            };
            let f = facts.get(id);
            let collision = collisions
                .cached_shapes()
                .cached_shape(id)
                .ok()
                .flatten()
                .map(|s| {
                    boxes(s.boxes().iter().map(|b| {
                        [
                            b.min(Axis::X),
                            b.min(Axis::Y),
                            b.min(Axis::Z),
                            b.max(Axis::X),
                            b.max(Axis::Y),
                            b.max(Axis::Z),
                        ]
                    }))
                });
            let mut row = json!({
                "hardness": f.hardness,
                "explosion_resistance": f.explosion_resistance,
                "emission": f.emission,
                "requires_tool": f.requires_tool,
                "friction": f.friction,
                "map_color_id": f.map_color,
                "can_occlude": f.can_occlude,
                "sound": sound(&f.sounds),
                "outline": boxes(f.outline.iter().copied()),
                "collision": collision,
                "classes": f.classes.iter().map(|c| &**c).collect::<Vec<_>>(),
            });
            if o.get("drops").is_some() {
                let mut drops = Map::new();
                for t in &tools {
                    let mut random = LegacyRandom::new(0);
                    let held = tool(t);
                    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
                    let roll = horizon_client::items::BlockLootRoll {
                        held: held.as_ref(),
                        ..Default::default()
                    };
                    for s in items.block_drops(registry, id, roll, &mut random, None) {
                        *counts.entry(s.item.to_string()).or_default() += s.count;
                    }
                    drops.insert(t.clone(), json!(counts));
                }
                row["drops"] = Value::Object(drops);
            }
            states.insert(key.clone(), row);
        }
        let item = facts.get(default).item.to_string();
        blocks.insert(
            name.clone(),
            json!({"states": states, "item": item, "item_known": items.items.contains_key(&*item)}),
        );
    }

    // Vanilla's sounds, by block: the harness maps the oracle's version's
    // sound events to this one's through them.
    let mut vanilla_sounds = Map::new();
    for name in oracle["blocks"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, _)| k)
    {
        if let Some(id) = name
            .starts_with("minecraft:")
            .then(|| registry.default_state(name))
            .flatten()
        {
            vanilla_sounds.insert(name.clone(), sound(&facts.get(id).sounds));
        }
    }

    let mut item_rows = Map::new();
    for name in oracle["items"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, _)| k)
    {
        item_rows.insert(
            name.clone(),
            match items.items.get(name.as_str()) {
                Some(i) => {
                    let props = enchantments.props(name);
                    let repairable: Vec<String> = props
                        .into_iter()
                        .flat_map(|p| &p.repairable)
                        .flat_map(|r| match r.strip_prefix('#') {
                            Some(tag) => items.item_tag_order.get(tag).cloned().unwrap_or_default(),
                            None => vec![r.clone()],
                        })
                        .collect();
                    json!({
                        "max_stack_size": i.max_stack,
                        "block": i.block.map(|b| b.0),
                        "max_damage": i.max_damage,
                        "modifiers": i.modifiers.iter().map(|m| {
                            json!({"type": m.attribute, "id": m.id, "amount": m.amount, "operation": m.operation, "slot": m.slot})
                        }).collect::<Vec<_>>(),
                        "tool": i.tool_damage_per_block.map(|per_block| json!({
                            "rules": i.tool.iter().map(|r| json!({"blocks": r.blocks, "speed": r.speed, "correct_for_drops": r.correct})).collect::<Vec<_>>(),
                            "default_mining_speed": i.tool_default_speed,
                            "damage_per_block": per_block,
                        })),
                        "enchantable": props.map_or(0, |p| p.enchantable),
                        "repairable": repairable,
                        "equip_slot": i.equip_slot,
                        "equip_asset": i.equip_asset,
                        "equipment": i.equip_asset.as_deref().map(|a| equipment_textures(&root, a)),
                        "food": i.food.map(|(nutrition, saturation)| json!({"nutrition": nutrition, "saturation": saturation})),
                    })
                }
                None => Value::Null,
            },
        );
    }

    let sorted = |list: &[String]| {
        let mut v = list.to_vec();
        v.sort();
        v.dedup();
        v
    };
    let mut recipe_rows = Map::new();
    for (name, recipe) in &recipes.crafting {
        if !ours(name) {
            continue;
        }
        let row = match recipe {
            Recipe::Shaped { cells, result, .. } => json!({
                "result": result.item.as_deref(), "count": result.count,
                "ingredients": cells.iter().flatten().map(|i| sorted(&i.order)).collect::<Vec<_>>()}),
            Recipe::Shapeless {
                ingredients,
                result,
            } => json!({
                "result": result.item.as_deref(), "count": result.count,
                "ingredients": ingredients.iter().map(|i| sorted(&i.order)).collect::<Vec<_>>()}),
            Recipe::Special(_) | Recipe::Mod(_) => json!({"special": true}),
        };
        recipe_rows.insert(name.clone(), row);
    }
    for c in recipes.cooking.iter().filter(|c| ours(&c.id)) {
        recipe_rows.insert(
            c.id.clone(),
            json!({"result": &*c.result.item, "count": c.result.count,
                "ingredients": [sorted(&c.ingredient.order)]}),
        );
    }
    for s in stations.stonecutting.iter().filter(|s| ours(&s.id)) {
        recipe_rows.insert(
            s.id.clone(),
            json!({"result": &*s.result.item, "count": s.result.count,
                "ingredients": [sorted(&s.input.order)]}),
        );
    }

    let mut block_tags = Map::new();
    for (tag, members) in &items.block_tags {
        let mut ids: Vec<&String> = members.iter().filter(|m| ours(m)).collect();
        if !ids.is_empty() {
            ids.sort();
            block_tags.insert(tag.clone(), json!(ids));
        }
    }

    let report = json!({
        "states": registry.len(),
        "build_seconds": built,
        "load_seconds": loaded,
        "blocks": blocks,
        "items": item_rows,
        "recipes": recipe_rows,
        "block_tags": block_tags,
        "vanilla_sounds": vanilla_sounds,
    });
    fs::write(out, serde_json::to_vec(&report)?)?;
    println!(
        "{out}: {} blocks, {} items, {} recipes; root built in {built:.2} s, loaded in {loaded:.2} s",
        blocks.len(),
        item_rows.len(),
        recipe_rows.len()
    );
    Ok(())
}

/// Names the load step an error came from.
fn step(what: &str) -> impl FnOnce(Box<dyn std::error::Error>) -> String {
    let what = what.to_owned();
    move |e| format!("{what}: {e}")
}

/// Where the content root comes from.
enum Source {
    /// Built here from WORK's carried content packages (`MOD...`, or all of them).
    Packages { wanted: Vec<String> },
    /// A root the game already built, with the folders of the packages it holds.
    Built {
        root: std::path::PathBuf,
        mods: Vec<std::path::PathBuf>,
    },
}

struct Args {
    source: Source,
    base: String,
    work: String,
    out: String,
}

impl Args {
    const USAGE: &str = "usage: mod_port_probe PREPARED WORK OUT.json [MOD...]\n       \
                         mod_port_probe --root ROOT PREPARED WORK OUT.json --mod DIR...";

    fn parse(args: impl Iterator<Item = String>) -> Result<Args> {
        let (mut root, mut mods, mut positional) = (None, Vec::new(), Vec::new());
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--root" => root = Some(args.next().ok_or(Self::USAGE)?),
                "--mod" => mods.push(args.next().ok_or(Self::USAGE)?.into()),
                _ => positional.push(arg),
            }
        }
        let (source, rest) = match (root, positional.as_slice()) {
            (Some(root), [base, work, out]) if !mods.is_empty() => (
                Source::Built {
                    root: root.into(),
                    mods,
                },
                [base, work, out],
            ),
            (None, [base, work, out, wanted @ ..]) if mods.is_empty() => (
                Source::Packages {
                    wanted: wanted.to_vec(),
                },
                [base, work, out],
            ),
            _ => return Err(Self::USAGE.into()),
        };
        let [base, work, out] = rest.map(Clone::clone);
        Ok(Args {
            source,
            base,
            work,
            out,
        })
    }
}

/// One package folder as the game reads it. `server` keeps only the jars a
/// server loads: a root a headless game built never fetched client-only ones.
fn read_mod(dir: &Path, server: bool) -> Result<ContentMod> {
    let package = horizon_mod_package::Package::read(dir)?;
    let content = horizon_mod_package::Content::read(dir, &package.manifest)?;
    let jars = (package.manifest.jar.into_iter())
        .filter(|jar| !server || jar.needed_on(horizon_mod_package::manifest::Side::Server))
        .collect();
    Ok(ContentMod {
        id: package.manifest.id,
        content,
        jars,
    })
}

/// WORK's carried content packages (`MOD...`, or all of them), by id, with
/// each one's side.
fn carried(
    work: &Path,
    wanted: &[String],
) -> Result<Vec<(ContentMod, horizon_mod_package::manifest::Side)>> {
    let mut mods = Vec::new();
    for entry in fs::read_dir(work.join("mods"))?.flatten() {
        let id = entry.file_name().to_string_lossy().into_owned();
        let dir = entry.path().join("carry");
        if !(wanted.is_empty() || wanted.contains(&id)) || !dir.is_dir() {
            continue;
        }
        let side = horizon_mod_package::Package::read(&dir)?.manifest.side;
        mods.push((read_mod(&dir, false)?, side));
    }
    mods.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    Ok(mods)
}

/// The data files the game's tables load over ROOT: each mod's `data/` with
/// what the root's builder generated on top (`resolved_resources`'s data
/// changes, the content pack of the game's pack stack).
fn data_overlay(
    base: &Path,
    root: &Path,
    jar_store: &Path,
    mods: &[ContentMod],
    work: &Path,
) -> Result<Vec<(String, Vec<Vec<u8>>)>> {
    use horizon_client::datapack::{DataPack, DataStore};
    let resources = mod_content::resolved_resources(
        base,
        root.to_path_buf(),
        jar_store,
        mods,
        work.join("probe-cache"),
    )?;
    let content = DataPack::from_source("horizon-content".into(), resources.data_changes().clone());
    Ok(DataStore::load(&[&content]).files.into_iter().collect())
}

fn server_load() -> horizon_world::native_load_context::LoadContext {
    use horizon_world::native_load_context::{DataLoadOrigin, LoadContext, LoadInputs};
    LoadContext::new(
        DataLoadOrigin::ServerDataLoad,
        std::sync::Arc::new(LoadInputs::new(Default::default(), Default::default())),
        [],
    )
}
