//! Native adapters for API-owned worldgen feature types: a mod's `FeatureType` as the engine's,
//! its configuration converted to NBT once at load, its decoded features placing through the
//! engine's one feature target with block states converted through the registry's own index.
//! A feature whose call fails (its error, a caught unwind or a wasm trap) keeps none of its
//! writes, and its type is switched off for the session.
use super::api_values::block_state::Index;
use horizon_core::json_nbt::{json_to_tag, tag_to_json};
use horizon_mod_runtime::block_definitions::Blocks;
use horizon_world::worldgen::mod_features::{
    ModConfiguredFeature, ModFeatureParts, ModFeatureTarget, ModFeatureType, ModFeatureTypes,
    install_mod_feature_types,
};
use mod_api::{
    BlockKey, BlockMetadata, BlockState, ConfiguredFeature, FeatureContext, FeatureDecode,
    FeatureError, FeatureType, HeightmapType, RuleTestHandle, StructureRandom, Tag,
    TargetBlockState,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn engine_heightmap(kind: HeightmapType) -> horizon_world::heightmap::HeightmapType {
    use horizon_world::heightmap::HeightmapType as Engine;
    match kind {
        HeightmapType::WorldSurfaceWg => Engine::WorldSurfaceWg,
        HeightmapType::WorldSurface => Engine::WorldSurface,
        HeightmapType::OceanFloorWg => Engine::OceanFloorWg,
        HeightmapType::OceanFloor => Engine::OceanFloor,
        HeightmapType::MotionBlocking => Engine::MotionBlocking,
        HeightmapType::MotionBlockingNoLeaves => Engine::MotionBlockingNoLeaves,
        _ => Engine::WorldSurfaceWg,
    }
}

/// The block snapshot a decoded feature converts its states through: the registry's own
/// worker index, kept with the feature.
pub(super) struct Held {
    pub(super) index: Arc<Index>,
    pub(super) metadata: &'static Blocks,
    air: [Option<BlockKey>; 3],
}
impl Held {
    pub(super) fn of(registry: &horizon_world::registry::BlockRegistry) -> Result<Self, String> {
        let index = super::native_worldgen::prepare(registry)?;
        let metadata = mod_api::__private::block_definitions()
            .ok_or_else(|| "block metadata is unavailable".to_owned())?;
        let air = ["minecraft:air", "minecraft:cave_air", "minecraft:void_air"].map(|name| {
            registry
                .default_state(name)
                .and_then(|state| index.to_api(state).ok())
                .map(BlockState::block)
        });
        Ok(Self {
            index,
            metadata,
            air,
        })
    }
    pub(super) fn to_api(
        &self,
        state: horizon_world::registry::BlockStateId,
    ) -> Option<BlockState> {
        self.index.to_api(state).ok()
    }
}

fn refused(message: impl std::fmt::Display) -> FeatureError {
    FeatureError(message.to_string())
}
fn json(value: &Tag) -> Result<serde_json::Value, FeatureError> {
    tag_to_json(value).map_err(refused)
}

/// The engine's decoders, as a [`FeatureDecode`].
pub(super) struct Decoder<'a> {
    pub(super) parts: &'a mut dyn ModFeatureParts,
    pub(super) held: &'a Held,
}
impl Decoder<'_> {
    fn state(&mut self, value: &serde_json::Value) -> Result<BlockState, FeatureError> {
        let state = self.parts.block_state(value).map_err(refused)?;
        self.held
            .to_api(state)
            .ok_or_else(|| refused("block state outside the mod API's index"))
    }
    fn rule(&mut self, value: &serde_json::Value) -> Result<RuleTestHandle, FeatureError> {
        let index = self.parts.rule_test(value).map_err(refused)?;
        Ok(mod_api::__private::rule_test_handle(index))
    }
}
impl FeatureDecode for Decoder<'_> {
    fn metadata(&self) -> &dyn BlockMetadata {
        self.held.metadata
    }
    fn block_state(&mut self, value: &Tag) -> Result<BlockState, FeatureError> {
        self.state(&json(value)?)
    }
    fn rule_test(&mut self, value: &Tag) -> Result<RuleTestHandle, FeatureError> {
        self.rule(&json(value)?)
    }
    fn target_block_states(&mut self, value: &Tag) -> Result<Vec<TargetBlockState>, FeatureError> {
        let serde_json::Value::Array(entries) = json(value)? else {
            return Err(refused("targets: not a list"));
        };
        entries
            .iter()
            .map(|entry| {
                let field = |name: &str| {
                    entry
                        .get(name)
                        .ok_or_else(|| refused(format!("targets: no {name}")))
                };
                let target = self.rule(field("target")?)?;
                let state = self.state(field("state")?)?;
                Ok(mod_api::__private::target_block_state(target, state))
            })
            .collect()
    }
}

/// The engine's feature target, as a [`FeatureContext`].
struct Context<'a> {
    inner: &'a mut dyn ModFeatureTarget,
    held: &'a Held,
}
impl StructureRandom for Context<'_> {
    fn next_int_bound(&mut self, bound: i32) -> i32 {
        self.inner.random().int(bound)
    }
    fn next_long(&mut self) -> i64 {
        self.inner.random().long()
    }
    fn next_float(&mut self) -> f32 {
        self.inner.random().float()
    }
    fn next_double(&mut self) -> f64 {
        self.inner.random().double()
    }
    fn next_boolean(&mut self) -> bool {
        self.inner.random().boolean()
    }
}
impl FeatureContext for Context<'_> {
    fn metadata(&self) -> &dyn BlockMetadata {
        self.held.metadata
    }
    fn origin(&self) -> [i32; 3] {
        self.inner.origin()
    }
    fn min_y(&mut self) -> i32 {
        self.inner.min_y()
    }
    fn max_y(&mut self) -> i32 {
        self.inner.max_y()
    }
    fn random(&mut self) -> &mut dyn StructureRandom {
        self
    }
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32> {
        self.inner.height(engine_heightmap(kind), x, z)
    }
    fn can_write(&mut self, pos: [i32; 3]) -> bool {
        self.inner.can_write(pos)
    }
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockState> {
        let state = self.inner.block(pos)?;
        self.held.to_api(state)
    }
    fn set_section_block(&mut self, pos: [i32; 3], state: BlockState) -> bool {
        match self.held.index.to_engine(state) {
            Ok(state) => self.inner.set_section_block(pos, state),
            Err(_) => false,
        }
    }
    fn set_block(&mut self, pos: [i32; 3], state: BlockState, flags: i32) -> bool {
        match self.held.index.to_engine(state) {
            Ok(state) => self.inner.set_block(pos, state, flags),
            Err(_) => false,
        }
    }
    fn is_air(&self, state: BlockState) -> bool {
        self.held.index.to_engine(state).is_ok() && self.held.air.contains(&Some(state.block()))
    }
    fn rule_test(&mut self, rule: RuleTestHandle, state: BlockState, pos: [i32; 3]) -> bool {
        match self.held.index.to_engine(state) {
            Ok(state) => {
                self.inner
                    .rule_test(mod_api::__private::rule_test_index(rule), state, pos)
            }
            Err(_) => false,
        }
    }
}

/// The engine's feature type around a mod's.
struct TypeAdapter {
    id: String,
    ty: Arc<dyn FeatureType>,
    /// Set once one of the type's features failed: every later call fails explicitly.
    off: Arc<AtomicBool>,
}
impl ModFeatureType for TypeAdapter {
    fn decode(
        &self,
        config: &serde_json::Value,
        parts: &mut dyn ModFeatureParts,
    ) -> Result<Arc<dyn ModConfiguredFeature>, String> {
        let held = Held::of(parts.registry())?;
        let config = json_to_tag(config).map_err(|e| e.to_string())?;
        let mut decoder = Decoder { parts, held: &held };
        let feature = horizon_client::crash::contain(|| self.ty.decode(&config, &mut decoder))
            .map_err(|panic| format!("decoding panicked: {panic}"))?
            .map_err(|error| error.0)?;
        Ok(Arc::new(FeatureAdapter {
            id: self.id.clone(),
            inner: feature,
            held,
            off: self.off.clone(),
        }))
    }
}

/// The engine's feature around a mod's decoded one.
struct FeatureAdapter {
    id: String,
    inner: Arc<dyn ConfiguredFeature>,
    held: Held,
    off: Arc<AtomicBool>,
}
impl ModConfiguredFeature for FeatureAdapter {
    fn place(&self, target: &mut dyn ModFeatureTarget) -> Result<bool, String> {
        if self.off.load(Ordering::Acquire) {
            return Err(format!(
                "feature type {} is switched off after an earlier failure",
                self.id
            ));
        }
        let mut context = Context {
            inner: target,
            held: &self.held,
        };
        let placed = match horizon_client::crash::contain(|| self.inner.place(&mut context)) {
            Ok(Ok(placed)) => return Ok(placed),
            Ok(Err(error)) => error.0,
            Err(panic) => format!("panicked: {panic}"),
        };
        self.off.store(true, Ordering::Release);
        Err(format!(
            "{placed}; feature type {} is switched off for this session",
            self.id
        ))
    }
}

fn installed_feature_types() -> ModFeatureTypes {
    mod_api::__private::feature_type_declarations()
        .into_iter()
        .map(|(id, ty)| {
            let adapter = TypeAdapter {
                id: id.clone(),
                ty,
                off: Arc::default(),
            };
            (id, Arc::new(adapter) as Arc<dyn ModFeatureType>)
        })
        .collect()
}

/// Hand the feature types the mods registered to the engine's feature decoder, before any world
/// opens.
pub(in crate::game) fn install() {
    if install_mod_feature_types(installed_feature_types()).is_err() {
        log::warn!("Mod feature types already installed; keeping the first table");
    }
}

#[cfg(test)]
#[path = "native_feature_types/tests.rs"]
mod tests;
