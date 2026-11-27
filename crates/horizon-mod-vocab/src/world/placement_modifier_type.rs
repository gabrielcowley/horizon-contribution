//! Placement-modifier types a trait mod registers: vanilla's `PlacementModifier` with its own
//! `modify`, as
//! [`Registrar::register_placement_modifier_type`](crate::Registrar::register_placement_modifier_type)
//! hands it over once at startup (NeoForge's registry `PLACEMENT_MODIFIER_TYPE`). An entry of a
//! data pack placed feature's `placement` list with `"type": "modid:filter"` then runs the mod's
//! code wherever the game places that placed feature: biome decoration, nested placed features
//! (random selectors and the like) and growth.
//!
//! There is no `Host` here: decoration runs on the worldgen worker threads, so a modifier sees
//! only its [`PlacementModifierContext`], the same object on the workers and on the server
//! thread. A type keeps no mutable state of its own and is deterministic for the same random,
//! blocks and the values it reads that its mod keeps (a config switch held in an atomic, say).
//! Everything around the modifier stays the game's own: the chain's order, the feature it
//! leads to, and the random it draws from.
use crate::wire::{Reader, Wire, WireError, Writer};
use crate::{
    BlockMetadata, BlockState, FeatureDecode, FeatureError, HeightmapType, StructureRandom, Tag,
};
use std::sync::Arc;

/// A mod's placement-modifier type: vanilla's `PlacementModifier` codec, whose code answers for
/// a placed feature's `placement` entry with `"type": "modid:filter"`. Implemented once, kept by
/// the game.
pub trait PlacementModifierType: Send + Sync + 'static {
    /// The codec: the modifier `config` describes (its JSON object converted to NBT once,
    /// without `type`), decoded once when the data pack loads and again on `/reload`. A JSON
    /// number arrives in the narrowest tag that holds it (`32` as a `Tag::Byte`), as vanilla's
    /// `JsonOps` to `NbtOps` gives it. Decode the vanilla values it embeds with `decode`'s
    /// decoders; a rule test decoded here has no use, since a modifier's context has no rule
    /// test. An `Err` refuses the placed feature at load, naming the type and the message, as a
    /// data pack's malformed built-in modifier is refused; nothing fails later, when a chunk
    /// first reaches it.
    fn decode(
        &self,
        config: &Tag,
        decode: &mut dyn FeatureDecode,
    ) -> Result<Arc<dyn PlacementModifier>, FeatureError>;
}
crate::wire::wire_handlers::handler!(dyn PlacementModifierType, "PlacementModifierType");

/// One decoded modifier: vanilla's `PlacementModifier.modify(context, random, origin, output)`.
pub trait PlacementModifier: std::fmt::Debug + Send + Sync {
    /// Push each position this modifier makes of `origin` with
    /// [`context.emit`](PlacementModifierContext::emit), in order: none filters the origin out,
    /// several fan it out. The game runs the chain's next modifier, or the feature, on each,
    /// depth first, as vanilla's `FeaturePlacer` does; draws from
    /// [`context.random()`](PlacementModifierContext::random) interleave with theirs. An `Err`
    /// fails the call: none of the positions it pushed are used, the placed feature fails, the
    /// game reports the type and the origin, and the type is switched off for the session,
    /// every later call failing explicitly. A native unwind and a wasm trap count as an `Err`.
    fn modify(
        &self,
        context: &mut dyn PlacementModifierContext,
        origin: [i32; 3],
    ) -> Result<(), FeatureError>;
}
crate::wire::wire_handlers::handler!(dyn PlacementModifier, "PlacementModifier");

/// What a modifier's [`modify`](PlacementModifier::modify) reads, and where it pushes its
/// positions: vanilla's `PlacementContext` without its level and generator, and the
/// `Consumer<BlockPos>` beside it. The same on the worldgen workers and on the server thread.
/// A modifier writes nothing.
pub trait PlacementModifierContext {
    /// Push one output position (the `Consumer<BlockPos>` of `modify`). More than 65,536 in one
    /// call fail it.
    fn emit(&mut self, pos: [i32; 3]);
    /// Immutable block metadata for this worker's resource snapshot.
    fn metadata(&self) -> &dyn BlockMetadata;
    /// The chain's exact random (`RandomSource` of `modify`), never reseeded: decoration's
    /// seeded random, the nested placed feature's caller's or the growing block's.
    fn random(&mut self) -> &mut dyn StructureRandom;
    /// `PlacementContext.getMinY`: the lowest block Y.
    fn min_y(&mut self) -> i32;
    /// `PlacementContext.getHeight` of the heightmap kind at (x, z), or `None` where the level
    /// can't answer.
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32>;
    /// `PlacementContext.getBlockState` at `pos`, or `None` outside the world's sections or the
    /// readable region; never a chunk load.
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockState>;
}

/// A registered placement-modifier type; the game builds its engine adapter at startup.
pub(crate) struct PlacementModifierTypeInfo {
    pub(crate) id: Box<str>,
    pub(crate) ty: Arc<dyn PlacementModifierType>,
}

/// Registered placement-modifier types, for the hidden game integration boundary.
// hidden: the game's startup adapter reads the registered types; mods declare them through the registrar.
#[doc(hidden)]
pub fn placement_modifier_type_declarations() -> Vec<(String, Arc<dyn PlacementModifierType>)> {
    crate::parts::installed_placement_modifier_type_list()
        .iter()
        .map(|info| (info.id.to_string(), info.ty.clone()))
        .collect()
}
