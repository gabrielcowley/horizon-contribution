use super::*;
use crate::worldgen::{
    anchor::WorldGenerationContext,
    feature::FeatureBlocks,
    feature_graph::{FeatureDefinition, FeatureExecutionError},
};
use horizon_core::random::LegacyRandom;
use std::collections::{BTreeSet, HashMap};

fn registry() -> &'static BlockRegistry {
    static REGISTRY: OnceLock<BlockRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let names = [
            "minecraft:air",
            "minecraft:cave_air",
            "minecraft:void_air",
            "minecraft:stone",
            "minecraft:cobblestone",
            "minecraft:end_stone",
            "minecraft:obsidian",
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

/// Stone from y 0 to 15 under air, writable only in x and z 0..32 and y 0..=15.
#[derive(Default)]
struct Quarry {
    blocks: HashMap<[i32; 3], BlockStateId>,
    writes: Vec<[i32; 3]>,
}
impl Quarry {
    fn at(&self, pos: [i32; 3]) -> BlockStateId {
        self.blocks.get(&pos).copied().unwrap_or_else(|| {
            if (0..16).contains(&pos[1]) {
                state("minecraft:stone")
            } else {
                state("minecraft:air")
            }
        })
    }
}
impl FeatureWorld for Quarry {
    type Error = ();
    fn block(&mut self, pos: [i32; 3]) -> Result<BlockStateId, ()> {
        Ok(self.at(pos))
    }
    fn set_block(&mut self, pos: [i32; 3], state: BlockStateId, _: i32) -> Result<bool, ()> {
        self.blocks.insert(pos, state);
        self.writes.push(pos);
        Ok(true)
    }
}
impl PlacementContext for Quarry {
    fn block_state(&mut self, pos: [i32; 3]) -> Result<BlockStateId, PlacementQueryError> {
        Ok(self.at(pos))
    }
    fn ensure_can_write(&mut self, pos: [i32; 3]) -> Result<bool, PlacementQueryError> {
        Ok((0..32).contains(&pos[0]) && (0..32).contains(&pos[2]))
    }
    fn set_section_block(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
    ) -> Result<(), PlacementQueryError> {
        self.blocks.insert(pos, state);
        self.writes.push(pos);
        Ok(())
    }
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(0, 16, 63)
    }
    fn min_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(0)
    }
    fn max_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(15)
    }
    fn height(&mut self, _: HeightmapType, _: i32, _: i32) -> Result<i32, PlacementQueryError> {
        Ok(16)
    }
}

/// A test feature: `count` times, it draws `nextInt(4)` for an x offset and writes `ore` where
/// `rule` matches, reading its own earlier writes back. `fail` answers an error after writing;
/// `reach` writes that far past the origin first; `reads` reads that many blocks first.
#[derive(Clone, Copy)]
struct Vein {
    ore: BlockStateId,
    rule: u32,
    count: i32,
    fail: bool,
    reach: Option<[i32; 3]>,
    reads: u32,
}
impl ModConfiguredFeature for Vein {
    fn place(&self, target: &mut dyn ModFeatureTarget) -> Result<bool, String> {
        let [x, y, z] = target.origin();
        if let Some([dx, dy, dz]) = self.reach {
            assert!(!target.set_section_block([x + dx, y + dy, z + dz], self.ore));
        }
        for _ in 0..self.reads {
            target.block([x, y, z]);
        }
        let mut placed = false;
        for _ in 0..self.count {
            let pos = [x + target.random().int(4), y, z];
            let Some(current) = target.block(pos) else {
                continue;
            };
            if target.rule_test(self.rule, current, pos) && target.set_section_block(pos, self.ore)
            {
                placed = true;
            }
        }
        if self.fail {
            return Err("vein gave up".into());
        }
        Ok(placed)
    }
}
struct VeinType;
impl ModFeatureType for VeinType {
    fn decode(
        &self,
        config: &serde_json::Value,
        parts: &mut dyn ModFeatureParts,
    ) -> Result<Arc<dyn ModConfiguredFeature>, String> {
        assert!(config.get("type").is_none(), "the type is the game's");
        let int = |name: &str| config.get(name).and_then(serde_json::Value::as_i64);
        let reach = config
            .get("reach")
            .map(|r| serde_json::from_value::<[i32; 3]>(r.clone()).map_err(|e| e.to_string()))
            .transpose()?;
        Ok(Arc::new(Vein {
            ore: parts.block_state(config.get("state").ok_or("no state")?)?,
            rule: parts.rule_test(config.get("target").ok_or("no target")?)?,
            count: int("count").ok_or("no count")? as i32,
            fail: config.get("fail").is_some(),
            reach,
            reads: int("reads").unwrap_or(0) as u32,
        }))
    }
}

fn types() -> ModFeatureTypes {
    BTreeMap::from([(
        "test:vein".to_owned(),
        Arc::new(VeinType) as Arc<dyn ModFeatureType>,
    )])
}

fn decode(json: serde_json::Value) -> Result<FeatureDefinition, DecodeError> {
    let bindings = FeatureBlockBindings::new(registry(), BTreeSet::new()).unwrap();
    with_test_types(&types(), || {
        FeatureDefinition::from_json_with_blocks(&json.to_string(), &bindings)
    })
}

fn vein(extra: serde_json::Value) -> FeatureDefinition {
    let mut json = serde_json::json!({
        "type": "test:vein",
        "state": {"id": "minecraft:granite"},
        "target": {"predicate_type": "minecraft:block_match", "block": "minecraft:stone"},
        "count": 3,
    });
    json.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    decode(json).unwrap()
}

fn place(
    feature: &FeatureDefinition,
    world: &mut Quarry,
    seed: i64,
) -> (Result<bool, FeatureExecutionError<()>>, i64) {
    let mut random = LegacyRandom::new(seed);
    let blocks = FeatureBlocks::from_registry(registry()).unwrap();
    let placed = feature.place(world, &mut random, [8, 4, 8], blocks);
    (placed, random.next_long())
}

/// Java's `new Random(seed)`: three `nextInt(4)`, then `nextLong`.
fn java_draws(seed: i64) -> (Vec<i32>, i64) {
    let mut random = LegacyRandom::new(seed);
    let offsets = (0..3).map(|_| random.next_int_bound(4).unwrap()).collect();
    (offsets, random.next_long())
}

#[test]
fn a_mods_feature_decodes_its_settings_and_writes_in_order_with_the_callers_random() {
    let feature = vein(serde_json::json!({}));
    let mut world = Quarry::default();
    let (placed, next) = place(&feature, &mut world, 42);
    assert!(placed.unwrap());
    let (offsets, java_next) = java_draws(42);
    // Each distinct offset was granite once; a repeated one reads its own write back, which no
    // longer matches stone, so it isn't written again.
    let mut expected: Vec<[i32; 3]> = Vec::new();
    for dx in offsets {
        let pos = [8 + dx, 4, 8];
        if !expected.contains(&pos) {
            expected.push(pos);
        }
    }
    assert_eq!(world.writes, expected);
    assert!(
        expected
            .iter()
            .all(|p| world.at(*p) == state("minecraft:granite"))
    );
    // Nothing drew besides the feature's own three draws.
    assert_eq!(next, java_next);
}

#[test]
fn a_bad_config_refuses_the_feature_at_load() {
    let error = decode(serde_json::json!({
        "type": "test:vein",
        "state": {"id": "minecraft:no_such_block"},
        "target": {"predicate_type": "minecraft:always_true"},
        "count": 1,
    }))
    .unwrap_err();
    assert!(error.to_string().contains("test:vein"), "{error}");
    // A built-in type is untouched by the mod's table.
    let bindings = FeatureBlockBindings::new(registry(), BTreeSet::new()).unwrap();
    let no_op = with_test_types(&types(), || {
        FeatureDefinition::from_json_with_blocks(r#"{"type":"minecraft:no_op"}"#, &bindings)
    });
    assert!(matches!(no_op, Ok(FeatureDefinition::NoOp)));
}

#[test]
fn a_failed_call_keeps_none_of_its_writes() {
    let feature = vein(serde_json::json!({"fail": true}));
    let mut world = Quarry::default();
    let (placed, next) = place(&feature, &mut world, 42);
    assert!(matches!(placed, Err(FeatureExecutionError::Mod(m)) if m == "vein gave up"));
    assert!(world.writes.is_empty());
    // The draws it made stay drawn, as vanilla's do when a feature throws.
    assert_eq!(next, java_draws(42).1);
}

#[test]
fn a_write_past_the_writable_region_or_the_build_height_is_refused() {
    for reach in [[24, 0, 0], [0, 12, 0], [0, -5, 0]] {
        let feature = vein(serde_json::json!({ "reach": reach }));
        let mut world = Quarry::default();
        let (placed, _) = place(&feature, &mut world, 7);
        assert!(placed.unwrap(), "{reach:?}");
        assert!(world.writes.iter().all(|p| p[1] == 4 && p[0] < 32));
    }
}

#[test]
fn running_out_of_work_fails_the_call_and_keeps_nothing() {
    let feature = vein(serde_json::json!({ "reads": FEATURE_WORK }));
    let mut world = Quarry::default();
    let (placed, _) = place(&feature, &mut world, 42);
    assert!(matches!(placed, Err(FeatureExecutionError::WorkLimit)));
    assert!(world.writes.is_empty());
    // Leaving room for its three reads and at most three writes, it places.
    let feature = vein(serde_json::json!({ "reads": FEATURE_WORK - 6 }));
    let (placed, _) = place(&feature, &mut Quarry::default(), 42);
    assert!(placed.unwrap());
}
