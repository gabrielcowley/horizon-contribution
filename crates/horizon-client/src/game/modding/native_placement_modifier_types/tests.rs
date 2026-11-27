use super::*;
use crate::game::api_values::{install_parts, resolve_block_definitions};
use horizon_world::registry::{BlockRegistry, BlockStateId};
use horizon_world::worldgen::mod_features::ModFeatureRandom;
use mod_api::Tag;
use mod_api::{FeatureDecode, FeatureError, Hooks, ModEntry, Mods, Registrar, RegistrationContext};

const STONE: BlockStateId = BlockStateId(1);
const FACTS: &str = r#"{"defaults":{"minecraft:air":0,"minecraft:stone":1,"minecraft:granite":2},"states":[["a",true,false,"","minecraft:air"],["s",false,false,"","minecraft:stone"],["s",false,false,"","minecraft:granite"]]}"#;

/// Where the block at the origin is `on`, emits the origin and the heightmap's top above it,
/// a random 0..3 blocks east; or panics when its config says so.
#[derive(Debug)]
struct OnBlock {
    on: BlockState,
    panics: bool,
}
impl PlacementModifier for OnBlock {
    fn modify(
        &self,
        context: &mut dyn PlacementModifierContext,
        [x, y, z]: [i32; 3],
    ) -> Result<(), FeatureError> {
        assert!(!self.panics, "the filter broke");
        if context.block([x, y, z]) != Some(self.on) || y <= context.min_y() {
            return Ok(());
        }
        context.emit([x, y, z]);
        let dx = context.random().next_int_bound(3);
        let top = context
            .height(HeightmapType::WorldSurfaceWg, x, z)
            .ok_or_else(|| FeatureError("no height".into()))?;
        context.emit([x + dx, top, z]);
        Ok(())
    }
}
struct OnBlockType;
impl PlacementModifierType for OnBlockType {
    fn decode(
        &self,
        config: &Tag,
        decode: &mut dyn FeatureDecode,
    ) -> Result<Arc<dyn PlacementModifier>, FeatureError> {
        let Tag::Compound(config) = config else {
            return Err(FeatureError("not an object".into()));
        };
        let on = config
            .get(&"on".into())
            .ok_or_else(|| FeatureError("no on".into()))?;
        Ok(Arc::new(OnBlock {
            on: decode.block_state(on)?,
            panics: config.get(&"panics".into()).is_some(),
        }))
    }
}
struct Filters;
#[mod_api::__subscriptions]
impl Hooks for Filters {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        r.register_placement_modifier_type("on_block", OnBlockType);
    }
}

/// The engine's side: a stone world under a heightmap at 64, its decoder, and a random that
/// answers 2 to every bound.
struct Engine {
    registry: &'static BlockRegistry,
}
impl ModFeatureParts for Engine {
    fn registry(&self) -> &BlockRegistry {
        self.registry
    }
    fn block_state(&mut self, value: &serde_json::Value) -> Result<BlockStateId, String> {
        let name = value["id"].as_str().ok_or("no id")?;
        self.registry.default_state(name).ok_or("unknown".into())
    }
    fn rule_test(&mut self, _: &serde_json::Value) -> Result<u32, String> {
        Err("no rules here".into())
    }
}
impl ModFeatureRandom for Engine {
    fn int(&mut self, _: i32) -> i32 {
        2
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
impl ModPlacementTarget for Engine {
    fn min_y(&mut self) -> i32 {
        0
    }
    fn random(&mut self) -> &mut dyn ModFeatureRandom {
        self
    }
    fn height(
        &mut self,
        kind: horizon_world::heightmap::HeightmapType,
        _: i32,
        _: i32,
    ) -> Option<i32> {
        (kind == horizon_world::heightmap::HeightmapType::WorldSurfaceWg).then_some(64)
    }
    fn block(&mut self, _: [i32; 3]) -> Option<BlockStateId> {
        Some(STONE)
    }
}

fn with_types(check: impl FnOnce(&ModPlacementModifierTypes, &mut Engine)) {
    let mods: Mods = vec![ModEntry::new("filters", Box::new(Filters))].into();
    mod_platform::testing::with_mods(mods.clone(), || {
        install_parts(&mods).unwrap();
        let registry: &'static BlockRegistry = Box::leak(Box::new(
            BlockRegistry::from_facts_json(FACTS.as_bytes()).unwrap(),
        ));
        resolve_block_definitions(registry).unwrap();
        check(
            &installed_placement_modifier_types(),
            &mut Engine { registry },
        );
    });
}

#[test]
fn a_mods_modifier_decodes_a_block_state_and_reads_through_the_engine_target() {
    with_types(|types, engine| {
        let ty = &types["filters:on_block"];
        let on_stone = ty
            .decode(
                &serde_json::json!({"on": {"id": "minecraft:stone"}}),
                engine,
            )
            .unwrap();
        let mut out = Vec::new();
        on_stone
            .modify(engine, [4, 10, 4], &mut |p| out.push(p))
            .unwrap();
        // The origin, then the heightmap's top two blocks east, in the order it pushed them.
        assert_eq!(out, [[4, 10, 4], [6, 64, 4]]);

        let on_granite = ty
            .decode(
                &serde_json::json!({"on": {"id": "minecraft:granite"}}),
                engine,
            )
            .unwrap();
        let mut out = Vec::new();
        on_granite
            .modify(engine, [4, 10, 4], &mut |p| out.push(p))
            .unwrap();
        assert!(out.is_empty());

        let refusal = ty.decode(&serde_json::json!({}), engine).err();
        assert_eq!(refusal.as_deref(), Some("no on"));
    });
}

#[test]
fn a_panicking_modifier_fails_and_switches_its_type_off() {
    with_types(|types, engine| {
        let ty = &types["filters:on_block"];
        let stone = serde_json::json!({"id": "minecraft:stone"});
        let sound = ty
            .decode(&serde_json::json!({ "on": stone }), engine)
            .unwrap();
        let broken = ty
            .decode(&serde_json::json!({ "on": stone, "panics": 1 }), engine)
            .unwrap();
        let failure = broken.modify(engine, [4, 10, 4], &mut |_| {}).unwrap_err();
        assert!(failure.contains("the filter broke"), "{failure}");
        // Every modifier of the type now fails without running, the sound one too.
        let mut out = Vec::new();
        let off = sound
            .modify(engine, [4, 10, 4], &mut |p| out.push(p))
            .unwrap_err();
        assert!(off.contains("switched off"), "{off}");
        assert!(out.is_empty());
    });
}
