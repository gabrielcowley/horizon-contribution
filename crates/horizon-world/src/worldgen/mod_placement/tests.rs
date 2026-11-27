use super::*;
use crate::registry::BlockRegistry;
use crate::worldgen::{
    anchor::WorldGenerationContext,
    feature_graph::PlacedFeatureDefinition,
    placement::{FeaturePlacementError, FeaturePlacer, PlacementModifier},
};
use horizon_core::random::LegacyRandom;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

fn registry() -> &'static BlockRegistry {
    static REGISTRY: OnceLock<BlockRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let names = [
            "minecraft:air",
            "minecraft:cave_air",
            "minecraft:void_air",
            "minecraft:stone",
            "minecraft:granite",
        ];
        let rows: Vec<_> = names
            .iter()
            .map(|name| {
                serde_json::json!([name, name.ends_with("air"), false, "", {"id": name, "properties": {}}])
            })
            .collect();
        let defaults: BTreeMap<_, _> = names.iter().enumerate().map(|(i, n)| (*n, i)).collect();
        let facts = serde_json::json!({"states": rows, "defaults": defaults});
        BlockRegistry::from_facts_json(facts.to_string().as_bytes()).unwrap()
    })
}
fn state(name: &str) -> BlockStateId {
    registry().default_state(name).unwrap()
}

/// Stone from y 0 to 15 under air; every heightmap answers 16.
struct Field;
impl PlacementContext for Field {
    fn block_state(&mut self, pos: [i32; 3]) -> Result<BlockStateId, PlacementQueryError> {
        Ok(state(if (0..16).contains(&pos[1]) {
            "minecraft:stone"
        } else {
            "minecraft:air"
        }))
    }
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(0, 64, 63)
    }
    fn min_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(0)
    }
    fn max_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(63)
    }
    fn height(&mut self, _: HeightmapType, _: i32, _: i32) -> Result<i32, PlacementQueryError> {
        Ok(16)
    }
}

/// Emits two positions a random 0..4 blocks up, side by side along +x, where the block at the
/// origin is `needs` and the origin is above the lowest Y; with `surface`, a third at the
/// heightmap's top.
struct Pair {
    needs: BlockStateId,
    surface: bool,
}
impl ModPlacementModifier for Pair {
    fn modify(
        &self,
        target: &mut dyn ModPlacementTarget,
        [x, y, z]: [i32; 3],
        emit: &mut dyn FnMut([i32; 3]),
    ) -> Result<(), String> {
        if target.block([x, y, z]) != Some(self.needs) || y <= target.min_y() {
            return Ok(());
        }
        let dy = target.random().int(4);
        emit([x, y + dy, z]);
        emit([x + 1, y + dy, z]);
        if self.surface {
            let top = target
                .height(HeightmapType::WorldSurfaceWg, x, z)
                .ok_or("no height")?;
            emit([x, top, z]);
        }
        Ok(())
    }
}

/// The `config_filter` shape: passes the origin through while a flag the mod keeps is on.
static FLAG: AtomicBool = AtomicBool::new(true);
struct Filter;
impl ModPlacementModifier for Filter {
    fn modify(
        &self,
        _: &mut dyn ModPlacementTarget,
        origin: [i32; 3],
        emit: &mut dyn FnMut([i32; 3]),
    ) -> Result<(), String> {
        if FLAG.load(Ordering::Relaxed) {
            emit(origin);
        }
        Ok(())
    }
}

/// Emits the origin, then fails; or emits more positions than a call may.
struct Broken {
    flood: bool,
}
impl ModPlacementModifier for Broken {
    fn modify(
        &self,
        _: &mut dyn ModPlacementTarget,
        origin: [i32; 3],
        emit: &mut dyn FnMut([i32; 3]),
    ) -> Result<(), String> {
        if self.flood {
            for _ in 0..=MODIFIER_WORK {
                emit(origin);
            }
            return Ok(());
        }
        emit(origin);
        Err("gave up".into())
    }
}

struct Types;
impl ModPlacementModifierType for Types {
    fn decode(
        &self,
        config: &serde_json::Value,
        parts: &mut dyn ModFeatureParts,
    ) -> Result<Arc<dyn ModPlacementModifier>, String> {
        assert!(config.get("type").is_none(), "the type is the game's");
        Ok(match config.get("kind").and_then(|k| k.as_str()) {
            Some("pair") => Arc::new(Pair {
                needs: parts.block_state(config.get("needs").ok_or("no needs")?)?,
                surface: config.get("surface").is_some(),
            }),
            Some("filter") => Arc::new(Filter),
            Some("broken") => Arc::new(Broken {
                flood: config.get("flood").is_some(),
            }),
            _ => return Err("unknown kind".into()),
        })
    }
}

fn types() -> ModPlacementModifierTypes {
    BTreeMap::from([(
        "test:modifier".to_owned(),
        Arc::new(Types) as Arc<dyn ModPlacementModifierType>,
    )])
}

fn bindings() -> FeatureBlockBindings<'static> {
    FeatureBlockBindings::new(registry(), BTreeSet::new()).unwrap()
}

fn modifier(config: serde_json::Value) -> PlacementModifier {
    let mut json = serde_json::json!({"type": "test:modifier"});
    json.as_object_mut()
        .unwrap()
        .extend(config.as_object().unwrap().clone());
    with_test_types(&types(), || {
        PlacementModifier::from_json_with_blocks(&json.to_string(), &bindings())
    })
    .unwrap()
}

/// Runs the chain from [8, 4, 8] with `new Random(seed)`; the feature records each position
/// and draws `nextInt(2)`. Answers the positions, the result and the random's next `nextLong`.
fn place(
    chain: &[PlacementModifier],
    seed: i64,
) -> (Vec<[i32; 3]>, Result<bool, FeaturePlacementError<()>>, i64) {
    let mut random = LegacyRandom::new(seed);
    let mut placed = Vec::new();
    let result =
        FeaturePlacer::new().place_in(chain, &mut Field, &mut random, [8, 4, 8], |r, p| {
            placed.push(p);
            r.next_int_bound(2).unwrap();
            Ok(true)
        });
    (placed, result, random.next_long())
}

#[test]
fn a_mods_modifier_runs_depth_first_in_the_chain_with_the_callers_random() {
    let pair = modifier(serde_json::json!({"kind": "pair", "needs": "minecraft:stone"}));
    for seed in [0, 42, -7] {
        let (placed, result, next) = place(&[pair.clone(), PlacementModifier::InSquare], seed);
        assert!(result.unwrap());
        // Vanilla's FeaturePlacer: the pair draws its offset; then for each of its outputs in
        // order, in_square draws x then z and the feature runs (and draws) before the next.
        let mut java = LegacyRandom::new(seed);
        let dy = java.next_int_bound(4).unwrap();
        let mut expected = Vec::new();
        for x in [8, 9] {
            let dx = java.next_int_bound(16).unwrap();
            let dz = java.next_int_bound(16).unwrap();
            expected.push([x + dx, 4 + dy, 8 + dz]);
            java.next_int_bound(2).unwrap();
        }
        assert_eq!(placed, expected, "seed {seed}");
        assert_eq!(next, java.next_long(), "seed {seed}");
    }
    // It reads the blocks, the heightmap and the lowest Y it is given: on granite it emits
    // nothing; with `surface`, its third output is the heightmap's top.
    let granite = modifier(serde_json::json!({"kind": "pair", "needs": "minecraft:granite"}));
    assert_eq!(place(&[granite], 1).0, Vec::<[i32; 3]>::new());
    let surface = modifier(serde_json::json!({
        "kind": "pair", "needs": "minecraft:stone", "surface": true,
    }));
    assert_eq!(place(&[surface], 1).0[2], [8, 16, 8]);
}

#[test]
fn a_filter_reads_its_flag_on_each_placement() {
    let filter = modifier(serde_json::json!({"kind": "filter"}));
    let (placed, result, _) = place(std::slice::from_ref(&filter), 3);
    assert_eq!((placed, result.unwrap()), (vec![[8, 4, 8]], true));
    FLAG.store(false, Ordering::Relaxed);
    let (placed, result, next) = place(std::slice::from_ref(&filter), 3);
    FLAG.store(true, Ordering::Relaxed);
    assert_eq!((placed, result.unwrap()), (vec![], false));
    // Filtering out draws nothing.
    assert_eq!(next, LegacyRandom::new(3).next_long());
}

#[test]
fn a_placed_feature_decodes_a_mods_modifier_and_a_bad_config_refuses_it_at_load() {
    let placed = |config: serde_json::Value| {
        let json = serde_json::json!({
            "feature": {"type": "minecraft:no_op"},
            "placement": [{"type": "minecraft:in_square"}, config],
        });
        with_test_types(&types(), || {
            PlacedFeatureDefinition::from_json_with_blocks(&json.to_string(), &bindings())
        })
    };
    assert!(placed(serde_json::json!({"type": "test:modifier", "kind": "filter"})).is_ok());
    for refused in [
        serde_json::json!({"type": "test:modifier", "kind": "pair", "needs": "minecraft:nope"}),
        serde_json::json!({"type": "test:modifier", "kind": "unknown"}),
    ] {
        let error = placed(refused).unwrap_err();
        assert!(error.to_string().contains("test:modifier"), "{error}");
    }
    // A type no mod registered is refused as before.
    assert!(placed(serde_json::json!({"type": "other:modifier"})).is_err());
}

#[test]
fn a_failed_call_passes_none_of_its_positions_on() {
    let (placed, result, _) = place(&[modifier(serde_json::json!({"kind": "broken"}))], 5);
    assert!(placed.is_empty());
    assert!(
        matches!(&result, Err(FeaturePlacementError::World(PlacementQueryError::Failed(m)))
            if m.contains("test:modifier") && m.contains("gave up")),
        "{result:?}"
    );
    let flood = modifier(serde_json::json!({"kind": "broken", "flood": true}));
    let (placed, result, _) = place(&[flood], 5);
    assert!(placed.is_empty());
    assert!(
        matches!(result, Err(FeaturePlacementError::WorkLimit)),
        "{result:?}"
    );
}
