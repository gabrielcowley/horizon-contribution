//! Fluid interactions: what a fluid turns into when it touches another fluid or a block, as
//! NeoForge's `FluidInteractionRegistry` holds them. `LiquidBlock.shouldSpreadLiquid` reads one
//! ordered list per converting fluid: vanilla's two lava rules first (`#minecraft:water` makes
//! obsidian or cobblestone, blue ice over soul soil makes basalt), then what the mods declared, in
//! mod load order and then declaration order.
//!
//! A mod declares an interaction as data ([`DeclaredFluidInteraction`]), never as code: the
//! converting fluid, one condition on the neighbour (a fluid, matched in either form, or a block),
//! an optional block that must lie under the converting fluid, and the block state it becomes as a
//! source and as a flowing fluid. [`set_declared`] records them before the block tables load;
//! [`crate::level::Behaviours::resolve_fluid_interactions`] resolves them against the registry,
//! refusing any that name something unknown.
use crate::{
    chunk::BlockState,
    fluid::FluidKind,
    level::{Behaviours, BlockId, Named},
    registry::{BlockRegistry, BlockStateId},
};
use std::sync::RwLock;

/// One fluid interaction as a mod declared it, by name. Ids are `namespace:path`; a block state
/// is written as `/setblock` takes it, `minecraft:basalt[axis=x]`, the properties left out at
/// their default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredFluidInteraction {
    /// The mod that declared it, named in a refusal; `minecraft` for vanilla's own.
    pub owner: String,
    /// The fluid that converts: `minecraft:lava`, `minecraft:water` or a mod's world fluid, in
    /// either form.
    pub fluid: String,
    /// The neighbour is this fluid, source or flowing. Exactly one of this and `touching_block`.
    /// Vanilla's own water rule reads back as `#minecraft:water`, the live tag; a declaration
    /// can't name a tag.
    pub touching_fluid: Option<String>,
    /// The neighbour is this block.
    pub touching_block: Option<String>,
    /// The block under the converting fluid must be this one.
    pub block_below: Option<String>,
    /// What it becomes when the converting fluid is a source.
    pub source: String,
    /// What it becomes when the converting fluid is flowing.
    pub flowing: String,
}

impl DeclaredFluidInteraction {
    /// Refuses a declaration whose shape is wrong, before anything is looked up: a condition
    /// missing or doubled, a result missing, an id that is not `namespace:path`.
    pub fn check(&self) -> Result<(), String> {
        match (&self.touching_fluid, &self.touching_block) {
            (None, None) => return Err("when: names no condition".into()),
            (Some(_), Some(_)) => {
                return Err("when: names both touching_fluid and touching_block".into());
            }
            _ => {}
        }
        let id = |field: &str, value: &str| {
            if namespaced(value) {
                Ok(())
            } else {
                Err(format!("{field}: {value:?} is not a namespace:path id"))
            }
        };
        id("fluid", &self.fluid)?;
        if let Some(fluid) = &self.touching_fluid {
            id("when.touching_fluid", fluid)?;
        }
        if let Some(block) = &self.touching_block {
            id("when.touching_block", block)?;
        }
        if let Some(block) = &self.block_below {
            id("when.block_below", block)?;
        }
        for (field, state) in [
            ("becomes.source", &self.source),
            ("becomes.flowing", &self.flowing),
        ] {
            if state.is_empty() {
                return Err(format!("{field}: is missing"));
            }
            id(field, state.split('[').next().unwrap_or(""))?;
        }
        Ok(())
    }
}

fn namespaced(id: &str) -> bool {
    id.split_once(':').is_some_and(|(namespace, path)| {
        !namespace.is_empty()
            && !path.is_empty()
            && namespace
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'))
            && path
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/'))
    })
}

static DECLARED: RwLock<Vec<DeclaredFluidInteraction>> = RwLock::new(Vec::new());

/// Records the mods' declarations, in mod load order and then declaration order, for the next
/// [`Behaviours::resolve_fluid_interactions`]. Replaces what was recorded before.
pub fn set_declared(interactions: Vec<DeclaredFluidInteraction>) {
    *DECLARED.write().unwrap_or_else(|e| e.into_inner()) = interactions;
}

/// What [`set_declared`] recorded.
pub fn declared() -> Vec<DeclaredFluidInteraction> {
    DECLARED.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// What a resolved interaction asks of the neighbour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Touching {
    /// Vanilla's lava rule: the neighbour's fluid is in `#minecraft:water`, read live.
    WaterTag,
    /// The neighbour's fluid is of this family ([`family`]), source or flowing.
    Fluid(usize),
    Block(BlockId),
}

/// One interaction, resolved against the block registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FluidInteraction {
    touching: Touching,
    below: Option<BlockId>,
    source: BlockStateId,
    flowing: BlockStateId,
    declared: DeclaredFluidInteraction,
}
impl FluidInteraction {
    /// The declaration it was resolved from, with its ids as the registry names them.
    pub fn declared(&self) -> &DeclaredFluidInteraction {
        &self.declared
    }
    /// What it places for a converting fluid that is (or isn't) a source.
    pub fn result(&self, source: bool) -> BlockStateId {
        if source { self.source } else { self.flowing }
    }
}

/// Every fluid's interactions, resolved: vanilla's lava rules, which apply to any fluid in
/// `#minecraft:lava` as `LiquidBlock` reads that tag, then each fluid family's own list.
#[derive(Clone, Debug, Default)]
pub struct FluidInteractions {
    lava_tag: Vec<FluidInteraction>,
    /// By [`family`].
    by_family: Vec<Vec<FluidInteraction>>,
}

/// A fluid's family: water 0, lava 1, a mod's world fluid 2 and on. Source and flowing share it.
fn family(kind: FluidKind) -> Option<usize> {
    match kind {
        FluidKind::Empty => None,
        FluidKind::Water | FluidKind::FlowingWater => Some(0),
        FluidKind::Lava | FluidKind::FlowingLava => Some(1),
        FluidKind::Mod(fluid) | FluidKind::ModFlowing(fluid) => Some(2 + fluid.index()),
    }
}

/// The source form's name of a family's fluid.
fn source_name(kind: FluidKind) -> String {
    match kind {
        FluidKind::Mod(fluid) | FluidKind::ModFlowing(fluid) => fluid.get().id.clone(),
        FluidKind::Water | FluidKind::FlowingWater => "minecraft:water".into(),
        _ => "minecraft:lava".into(),
    }
}

impl FluidInteractions {
    /// Vanilla's two rules alone, as `LiquidBlock.shouldSpreadLiquid` has them.
    pub(crate) fn vanilla(named: &Named, table: &crate::level::StateTable) -> Self {
        let vanilla = |touching, below: Option<BlockId>, source: BlockId, flowing: BlockId| {
            let name = |block: BlockId| table.info(block).name.to_string();
            FluidInteraction {
                touching,
                below,
                source: table.default_state(source),
                flowing: table.default_state(flowing),
                declared: DeclaredFluidInteraction {
                    owner: "minecraft".into(),
                    fluid: "minecraft:lava".into(),
                    touching_fluid: (touching == Touching::WaterTag)
                        .then(|| "#minecraft:water".into()),
                    touching_block: match touching {
                        Touching::Block(block) => Some(name(block)),
                        _ => None,
                    },
                    block_below: below.map(name),
                    source: name(source),
                    flowing: name(flowing),
                },
            }
        };
        Self {
            lava_tag: vec![
                vanilla(Touching::WaterTag, None, named.obsidian, named.cobblestone),
                vanilla(
                    Touching::Block(named.blue_ice),
                    Some(named.soul_soil),
                    named.basalt,
                    named.basalt,
                ),
            ],
            by_family: Vec::new(),
        }
    }

    fn of_family(&self, kind: FluidKind) -> &[FluidInteraction] {
        family(kind)
            .and_then(|family| self.by_family.get(family))
            .map_or(&[], Vec::as_slice)
    }
}

/// Why a declaration was refused when it was resolved.
fn refusal(declared: &DeclaredFluidInteraction, reason: String) -> String {
    format!(
        "mod {}: fluid interaction of {}: {reason}",
        declared.owner, declared.fluid
    )
}

/// A block state written as `/setblock` takes it: the block, then any properties, the rest at
/// their default.
fn parse_state(registry: &BlockRegistry, text: &str) -> Result<BlockStateId, String> {
    let (name, properties) = match text.split_once('[') {
        Some((name, rest)) => (
            name,
            Some(
                rest.strip_suffix(']')
                    .ok_or_else(|| format!("{text:?} has no closing ]"))?,
            ),
        ),
        None => (text, None),
    };
    let default = registry
        .default_state(name)
        .ok_or_else(|| format!("{name} is not a block"))?;
    let Some(properties) = properties.filter(|p| !p.trim().is_empty()) else {
        return Ok(default);
    };
    let mut state: BlockState = registry
        .state(default)
        .cloned()
        .ok_or_else(|| format!("{name} has no states"))?;
    for pair in properties.split(',') {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("{text:?}: {pair:?} is not key=value"))?;
        let (key, value) = (key.trim(), value.trim());
        if !state.properties.contains_key(key) {
            return Err(format!("{name} has no property {key}"));
        }
        state.properties.insert(key.into(), value.into());
    }
    registry
        .id(&state)
        .ok_or_else(|| format!("{text:?} is not a state of {name}"))
}

impl Behaviours {
    /// Resolves the mods' declarations against the registry and puts them after vanilla's rules,
    /// in the order given. A declaration naming an unknown or tank-only fluid, an unknown block or
    /// a state its block doesn't have is left out; the refusals, each naming the mod and the
    /// field, are returned.
    pub fn resolve_fluid_interactions(
        &mut self,
        registry: &BlockRegistry,
        declared: &[DeclaredFluidInteraction],
    ) -> Vec<String> {
        let mut table = FluidInteractions::vanilla(&self.named, &self.table);
        let mut refused = Vec::new();
        for declared in declared {
            match self.resolve_one(registry, declared) {
                Ok((family, interaction)) => {
                    if table.by_family.len() <= family {
                        table.by_family.resize_with(family + 1, Vec::new);
                    }
                    table.by_family[family].push(interaction);
                }
                Err(reason) => refused.push(refusal(declared, reason)),
            }
        }
        self.fluid_interactions = table;
        refused
    }

    fn resolve_one(
        &self,
        registry: &BlockRegistry,
        declared: &DeclaredFluidInteraction,
    ) -> Result<(usize, FluidInteraction), String> {
        declared.check()?;
        let world_fluid = |field: &str, id: &str| {
            FluidKind::from_name(id)
                .filter(|kind| *kind != FluidKind::Empty)
                .ok_or_else(|| format!("{field}: {id} is not a fluid in the world"))
        };
        let block = |field: &str, id: &str| {
            self.table
                .block_by_name(id)
                .ok_or_else(|| format!("{field}: {id} is not a block"))
        };
        let converting = world_fluid("fluid", &declared.fluid)?;
        let family = family(converting).expect("not the empty fluid");
        let (touching, touching_fluid, touching_block) =
            match (&declared.touching_fluid, &declared.touching_block) {
                (Some(fluid), _) => {
                    let kind = world_fluid("when.touching_fluid", fluid)?;
                    (
                        Touching::Fluid(family_of_world(kind)),
                        Some(source_name(kind)),
                        None,
                    )
                }
                (None, Some(id)) => (
                    Touching::Block(block("when.touching_block", id)?),
                    None,
                    Some(id.clone()),
                ),
                (None, None) => unreachable!("checked"),
            };
        let below = declared
            .block_below
            .as_deref()
            .map(|id| block("when.block_below", id))
            .transpose()?;
        let source =
            parse_state(registry, &declared.source).map_err(|e| format!("becomes.source: {e}"))?;
        let flowing = parse_state(registry, &declared.flowing)
            .map_err(|e| format!("becomes.flowing: {e}"))?;
        Ok((
            family,
            FluidInteraction {
                touching,
                below,
                source,
                flowing,
                declared: DeclaredFluidInteraction {
                    fluid: source_name(converting),
                    touching_fluid,
                    touching_block,
                    ..declared.clone()
                },
            },
        ))
    }

    /// The interactions `kind` converts by, in the order they are tried: vanilla's when `kind`
    /// is in `#minecraft:lava`, then those declared for its family. Empty for a fluid with none.
    pub fn fluid_interactions_of(
        &self,
        kind: FluidKind,
    ) -> impl Iterator<Item = &FluidInteraction> + '_ {
        let vanilla = if self.is_lava_fluid(kind) {
            &self.fluid_interactions.lava_tag[..]
        } else {
            &[]
        };
        vanilla
            .iter()
            .chain(self.fluid_interactions.of_family(kind))
    }

    /// Whether `kind` has any interaction: one indexed load and a tag test.
    pub fn has_fluid_interactions(&self, kind: FluidKind) -> bool {
        self.is_lava_fluid(kind) || !self.fluid_interactions.of_family(kind).is_empty()
    }

    /// The first of `kind`'s interactions, in order, that `neighbour` (the state beside the
    /// converting fluid) and `below` (the state under it, read only when an interaction asks)
    /// satisfy, and the block state it places for a converting fluid that is a `source` or not.
    /// `LiquidBlock.shouldSpreadLiquid` asks it for each neighbour in turn; a mod asks it for one.
    pub fn fluid_interaction(
        &self,
        kind: FluidKind,
        source: bool,
        neighbour: BlockStateId,
        below: impl Fn() -> BlockStateId,
    ) -> Option<(&FluidInteraction, BlockStateId)> {
        let neighbour_fluid = self.fluid(neighbour).kind();
        self.fluid_interactions_of(kind)
            .find(|interaction| {
                let touches = match interaction.touching {
                    Touching::WaterTag => self.is_water_fluid(neighbour_fluid),
                    Touching::Fluid(family) => self::family(neighbour_fluid) == Some(family),
                    Touching::Block(block) => self.is_block(neighbour, block),
                };
                touches
                    && interaction
                        .below
                        .is_none_or(|block| self.is_block(below(), block))
            })
            .map(|interaction| (interaction, interaction.result(source)))
    }
}

fn family_of_world(kind: FluidKind) -> usize {
    family(kind).expect("not the empty fluid")
}
