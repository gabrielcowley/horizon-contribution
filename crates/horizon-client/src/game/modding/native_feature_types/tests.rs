use super::*;
use crate::game::api_values::{install_parts, resolve_block_definitions};
use horizon_world::registry::{BlockRegistry, BlockStateId};
use horizon_world::worldgen::mod_features::ModFeatureRandom;
use mod_api::{Hooks, ModEntry, Mods, Registrar, RegistrationContext};

const AIR: BlockStateId = BlockStateId(0);
const STONE: BlockStateId = BlockStateId(1);
const GRANITE: BlockStateId = BlockStateId(2);
const FACTS: &str = r#"{"defaults":{"minecraft:air":0,"minecraft:stone":1,"minecraft:granite":2},"states":[["a",true,false,"","minecraft:air"],["s",false,false,"","minecraft:stone"],["s",false,false,"","minecraft:granite"]]}"#;

/// A vein that replaces the origin and the block east of it by its targets, or panics when its
/// config says so.
#[derive(Debug)]
struct Vein {
    targets: Vec<TargetBlockState>,
    panics: bool,
}
impl ConfiguredFeature for Vein {
    fn place(&self, context: &mut dyn FeatureContext) -> Result<bool, FeatureError> {
        assert!(!self.panics, "the vein collapsed");
        let [x, y, z] = context.origin();
        let mut placed = false;
        for pos in [[x, y, z], [x + 1, y, z]] {
            let Some(current) = context.block(pos) else {
                continue;
            };
            for target in &self.targets {
                if context.rule_test(target.target, current, pos)
                    && !context.is_air(target.state)
                    && context.set_section_block(pos, target.state)
                {
                    placed = true;
                    break;
                }
            }
        }
        Ok(placed)
    }
}
struct VeinType;
impl FeatureType for VeinType {
    fn decode(
        &self,
        config: &Tag,
        decode: &mut dyn FeatureDecode,
    ) -> Result<Arc<dyn ConfiguredFeature>, FeatureError> {
        let Tag::Compound(config) = config else {
            return Err(FeatureError("not an object".into()));
        };
        let targets = config
            .get("targets")
            .ok_or_else(|| FeatureError("no targets".into()))?;
        Ok(Arc::new(Vein {
            targets: decode.target_block_states(targets)?,
            panics: config.get("panics").is_some(),
        }))
    }
}
struct Quarry;
#[mod_api::__subscriptions]
impl Hooks for Quarry {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        r.register_feature_type("vein", VeinType);
    }
}

/// The engine's side: a stone world, its decoders and every write and rule test.
struct Engine {
    registry: &'static BlockRegistry,
    rules: Vec<serde_json::Value>,
    writes: Vec<([i32; 3], BlockStateId)>,
    tested: Vec<u32>,
}
impl ModFeatureParts for Engine {
    fn registry(&self) -> &BlockRegistry {
        self.registry
    }
    fn block_state(&mut self, value: &serde_json::Value) -> Result<BlockStateId, String> {
        let name = value["id"].as_str().ok_or("no id")?;
        self.registry.default_state(name).ok_or("unknown".into())
    }
    fn rule_test(&mut self, value: &serde_json::Value) -> Result<u32, String> {
        self.rules.push(value.clone());
        Ok(self.rules.len() as u32 - 1)
    }
}
impl ModFeatureRandom for Engine {
    fn int(&mut self, _: i32) -> i32 {
        0
    }
    fn long(&mut self) -> i64 {
        0
    }
    fn float(&mut self) -> f32 {
        0.0
    }
    fn double(&mut self) -> f64 {
        0.0
    }
    fn boolean(&mut self) -> bool {
        false
    }
}
impl ModFeatureTarget for Engine {
    fn origin(&self) -> [i32; 3] {
        [4, 10, 4]
    }
    fn min_y(&mut self) -> i32 {
        0
    }
    fn max_y(&mut self) -> i32 {
        63
    }
    fn random(&mut self) -> &mut dyn ModFeatureRandom {
        self
    }
    fn height(
        &mut self,
        _: horizon_world::heightmap::HeightmapType,
        _: i32,
        _: i32,
    ) -> Option<i32> {
        Some(64)
    }
    fn can_write(&mut self, _: [i32; 3]) -> bool {
        true
    }
    fn block(&mut self, _: [i32; 3]) -> Option<BlockStateId> {
        Some(STONE)
    }
    fn set_section_block(&mut self, pos: [i32; 3], state: BlockStateId) -> bool {
        self.writes.push((pos, state));
        true
    }
    fn set_block(&mut self, pos: [i32; 3], state: BlockStateId, _: i32) -> bool {
        self.writes.push((pos, state));
        true
    }
    fn is_air(&mut self, state: BlockStateId) -> bool {
        state == AIR
    }
    fn rule_test(&mut self, rule: u32, state: BlockStateId, _: [i32; 3]) -> bool {
        self.tested.push(rule);
        // The second rule matches stone; the first matches nothing.
        rule == 1 && state == STONE
    }
}

#[test]
fn a_mods_feature_decodes_vanilla_targets_and_places_through_the_engine_target() {
    let mods: Mods = vec![ModEntry::new("quarry", Box::new(Quarry))].into();
    mod_platform::testing::with_mods(mods.clone(), || {
        install_parts(&mods).unwrap();
        let registry: &'static BlockRegistry = Box::leak(Box::new(
            BlockRegistry::from_facts_json(FACTS.as_bytes()).unwrap(),
        ));
        resolve_block_definitions(registry).unwrap();
        let types = installed_feature_types();
        let vein = &types["quarry:vein"];
        let mut engine = Engine {
            registry,
            rules: vec![],
            writes: vec![],
            tested: vec![],
        };
        let config = serde_json::json!({"targets": [
            {"target": {"predicate_type": "minecraft:always_true"}, "state": {"id": "minecraft:air"}},
            {"target": {"predicate_type": "minecraft:block_match", "block": "minecraft:stone"},
             "state": {"id": "minecraft:granite"}},
        ]});
        let feature = vein.decode(&config, &mut engine).unwrap();
        // The rules reached the engine's decoder as JSON, unchanged by the trip through NBT.
        assert_eq!(engine.rules[1]["block"], "minecraft:stone");

        assert_eq!(feature.place(&mut engine), Ok(true));
        // Both blocks tested both rules in order: air (rule 0) matched nothing, granite did.
        assert_eq!(engine.tested, [0, 1, 0, 1]);
        assert_eq!(engine.writes, [([4, 10, 4], GRANITE), ([5, 10, 4], GRANITE)]);

        // A config the type refuses refuses the feature, naming its message.
        let refusal = vein.decode(&serde_json::json!({}), &mut engine).err();
        assert_eq!(refusal.as_deref(), Some("no targets"));
    });
}

#[test]
fn a_panicking_feature_fails_and_switches_its_type_off() {
    let mods: Mods = vec![ModEntry::new("quarry", Box::new(Quarry))].into();
    mod_platform::testing::with_mods(mods.clone(), || {
        install_parts(&mods).unwrap();
        let registry: &'static BlockRegistry = Box::leak(Box::new(
            BlockRegistry::from_facts_json(FACTS.as_bytes()).unwrap(),
        ));
        resolve_block_definitions(registry).unwrap();
        let types = installed_feature_types();
        let vein = &types["quarry:vein"];
        let mut engine = Engine {
            registry,
            rules: vec![],
            writes: vec![],
            tested: vec![],
        };
        let targets = serde_json::json!([{"target": {"predicate_type": "minecraft:always_true"},
            "state": {"id": "minecraft:granite"}}]);
        let sound = vein
            .decode(&serde_json::json!({ "targets": targets }), &mut engine)
            .unwrap();
        let collapsing = vein
            .decode(
                &serde_json::json!({ "targets": targets, "panics": 1 }),
                &mut engine,
            )
            .unwrap();
        let failure = collapsing.place(&mut engine).unwrap_err();
        assert!(failure.contains("the vein collapsed"), "{failure}");
        // Every feature of the type now fails without running, the sound one too.
        let off = sound.place(&mut engine).unwrap_err();
        assert!(off.contains("switched off"), "{off}");
        assert!(engine.writes.is_empty());
    });
}
