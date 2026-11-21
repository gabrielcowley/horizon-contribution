//! Feature types a trait mod registers: vanilla's `Feature` with its own `place`, as
//! [`Registrar::register_feature_type`](crate::Registrar::register_feature_type) hands it over
//! once at startup (NeoForge's registry `FEATURE`). A data pack configured feature with
//! `"type": "modid:vein"` then places by the mod's code, wherever the game places features:
//! biome decoration, nested placed features (random selectors and the like), growth and
//! `/place feature`.
//!
//! There is no `Host` here: decoration runs on the worldgen worker threads, so a feature sees
//! only its [`FeatureContext`], the same object on the workers and on the server thread. A type
//! keeps no mutable state and is deterministic: the same random and blocks give the same
//! writes. Everything around the feature stays the game's own: its placed feature's modifiers,
//! the decoration step and seed, and the random it draws from.
use crate::wire::{Reader, Wire, WireError, Writer};
use crate::{BlockMetadata, BlockState, HeightmapType, StructureRandom, Tag};
use std::sync::Arc;

/// Why a feature type refused a configuration, or a feature failed while placing.
#[derive(Debug)]
// exhaustive: mods build it themselves as decode's and place's error answer
#[derive(horizon_mod_api_macros::Wire)]
pub struct FeatureError(pub String);
impl std::fmt::Display for FeatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for FeatureError {}
impl From<(crate::Identifier, String)> for FeatureError {
    fn from((source, reason): (crate::Identifier, String)) -> Self {
        Self(format!("{source}: {reason}"))
    }
}

/// A vanilla `RuleTest` the game decoded for a feature (an ore's `target`), named by this
/// handle. It belongs to the configured feature that decoded it: test it with
/// [`FeatureContext::rule_test`] while that feature places, and don't keep it past a reload.
#[derive(horizon_mod_api_macros::Wire)]
#[wire(internal_field_layouts)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RuleTestHandle(pub(crate) u32);

/// Construct a rule handle at the hidden integration boundary.
// hidden: only the game's adapter builds a handle; mods receive one from the decoder.
#[doc(hidden)]
pub fn rule_test_handle(index: u32) -> RuleTestHandle {
    RuleTestHandle(index)
}
/// The index behind a rule handle, at the hidden integration boundary.
// hidden: only the game's adapter reads a handle back.
#[doc(hidden)]
pub fn rule_test_index(handle: RuleTestHandle) -> u32 {
    handle.0
}

/// One entry of an ore's `targets` list: vanilla's `OreConfiguration.TargetBlockState`, the
/// rule a block must pass and the state that replaces it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct TargetBlockState {
    /// `target`: the rule the block in place must pass.
    pub target: RuleTestHandle,
    /// `state`: what replaces it.
    pub state: BlockState,
}

/// Construct a target entry at the hidden integration boundary.
// hidden: only the game's decoder builds an entry; mods receive them from `target_block_states`.
#[doc(hidden)]
pub fn target_block_state(target: RuleTestHandle, state: BlockState) -> TargetBlockState {
    TargetBlockState { target, state }
}

/// A mod's feature type: vanilla's `Feature` codec, whose code answers for a data pack
/// configured feature with `"type": "modid:vein"`. Implemented once, kept by the game.
pub trait FeatureType: Send + Sync + 'static {
    /// The codec: the configured feature `config` describes (26.3's inline shape, its settings
    /// beside `type`, converted to NBT once, without `type`), decoded once when the data pack
    /// loads and again on `/reload`. Decode the vanilla values it embeds with `decode`'s
    /// decoders. An `Err` refuses the feature at load, naming the type and the message, as a
    /// data pack's malformed built-in feature is refused; nothing fails later, when a chunk
    /// first reaches it.
    fn decode(
        &self,
        config: &Tag,
        decode: &mut dyn FeatureDecode,
    ) -> Result<Arc<dyn ConfiguredFeature>, FeatureError>;
}
crate::wire::wire_handlers::handler!(dyn FeatureType, "FeatureType");

/// One decoded feature: vanilla's `Feature.place(level, generator, random, origin)`.
pub trait ConfiguredFeature: std::fmt::Debug + Send + Sync {
    /// Place at `context.origin()`: `Ok(true)` when something was placed (`/place feature`
    /// fails on `false`), `Ok(false)` when nothing was. An `Err` fails the feature: nothing it
    /// wrote in this call is kept, the game reports the type and the origin, and the type is
    /// switched off for the session, every later call failing explicitly. A native unwind and
    /// a wasm trap count as an `Err`.
    fn place(&self, context: &mut dyn FeatureContext) -> Result<bool, FeatureError>;
}
crate::wire::wire_handlers::handler!(dyn ConfiguredFeature, "ConfiguredFeature");

/// The decoders [`FeatureType::decode`] borrows for the vanilla values a configuration
/// embeds. Each checks the value against the blocks of the world being loaded.
pub trait FeatureDecode {
    /// Immutable, load-owned block metadata.
    fn metadata(&self) -> &dyn BlockMetadata;
    /// `BlockState.CODEC`: a block id, or `{"id": ..., "properties": {...}}` (missing
    /// properties keep their defaults).
    fn block_state(&mut self, value: &Tag) -> Result<BlockState, FeatureError>;
    /// `RuleTest.CODEC`: a rule (`{"predicate_type": "minecraft:block_match", ...}`), kept
    /// with the feature being decoded.
    fn rule_test(&mut self, value: &Tag) -> Result<RuleTestHandle, FeatureError>;
    /// `OreConfiguration.TargetBlockState`'s list codec: an ore's `targets`, each
    /// `{"target": <rule>, "state": <block state>}`.
    fn target_block_states(&mut self, value: &Tag) -> Result<Vec<TargetBlockState>, FeatureError>;
}

/// What a feature's [`place`](ConfiguredFeature::place) reaches: vanilla's `WorldGenLevel`
/// around one origin, its random and the generation bounds. The same on the worldgen workers
/// and on the server thread. Writes are kept only when the call succeeds, and reads see the
/// call's own writes.
pub trait FeatureContext {
    /// Immutable block metadata for this worker's resource snapshot.
    fn metadata(&self) -> &dyn BlockMetadata;
    /// `origin`: where the feature places.
    fn origin(&self) -> [i32; 3];
    /// `WorldGenLevel.getMinY`: the lowest block Y.
    fn min_y(&mut self) -> i32;
    /// `WorldGenLevel.getMaxY`: the highest block Y, inclusive.
    fn max_y(&mut self) -> i32;
    /// The caller's exact random (`RandomSource` of `place`), never reseeded: decoration's
    /// seeded random, the nested feature's caller's, the growing block's or the level's for
    /// `/place feature`.
    fn random(&mut self) -> &mut dyn StructureRandom;
    /// `WorldGenLevel.getHeight` of the heightmap kind at (x, z), or `None` where the level
    /// can't answer.
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32>;
    /// `WorldGenLevel.ensureCanWrite`: whether `pos` is in the writable region.
    fn can_write(&mut self, pos: [i32; 3]) -> bool;
    /// The block at `pos` (`BulkSectionAccess.getBlockState`), or `None` outside the world's
    /// sections or the readable region; never a chunk load.
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockState>;
    /// `LevelChunkSection.setBlockState` through `BulkSectionAccess`, as vanilla's `ore` writes:
    /// no updates, heightmaps or block entities. False outside the writable region or the
    /// build height.
    fn set_section_block(&mut self, pos: [i32; 3], state: BlockState) -> bool;
    /// `WorldGenLevel.setBlock(pos, state, flags)`. False outside the writable region or the
    /// build height.
    fn set_block(&mut self, pos: [i32; 3], state: BlockState, flags: i32) -> bool;
    /// `BlockState.isAir`.
    fn is_air(&self, state: BlockState) -> bool;
    /// `RuleTest.test(state, random)` of a rule this feature decoded, drawing from
    /// [`random`](Self::random) as `random_block_match` does. False for another feature's
    /// handle.
    fn rule_test(&mut self, rule: RuleTestHandle, state: BlockState, pos: [i32; 3]) -> bool;
}

/// A registered feature type; the game builds its engine adapter at startup.
pub(crate) struct FeatureTypeInfo {
    pub(crate) id: Box<str>,
    pub(crate) ty: Arc<dyn FeatureType>,
}

/// Registered feature types, for the hidden game integration boundary.
// hidden: the game's startup adapter reads the registered types; mods declare them through the registrar.
#[doc(hidden)]
pub fn feature_type_declarations() -> Vec<(String, Arc<dyn FeatureType>)> {
    crate::parts::installed_feature_type_list()
        .iter()
        .map(|info| (info.id.to_string(), info.ty.clone()))
        .collect()
}
