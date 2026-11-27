//! Native adapters for API-owned placement-modifier types: a mod's `PlacementModifierType` as
//! the engine's, its configuration converted to NBT once at load, its decoded modifiers running
//! inside the placement chain with block states converted through the registry's own index. A
//! modifier whose call fails (its error, a caught unwind or a wasm trap) passes none of its
//! positions on, and its type is switched off for the session.
use super::native_feature_types::{Decoder, Held, engine_heightmap};
use horizon_core::json_nbt::json_to_tag;
use horizon_world::worldgen::mod_features::ModFeatureParts;
use horizon_world::worldgen::mod_placement::{
    ModPlacementModifier, ModPlacementModifierType, ModPlacementModifierTypes, ModPlacementTarget,
    install_mod_placement_modifier_types,
};
use mod_api::{
    BlockMetadata, BlockState, HeightmapType, PlacementModifier, PlacementModifierContext,
    PlacementModifierType, StructureRandom,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// The engine's placement target and output sink, as a [`PlacementModifierContext`].
struct Context<'a> {
    inner: &'a mut dyn ModPlacementTarget,
    emit: &'a mut dyn FnMut([i32; 3]),
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
impl PlacementModifierContext for Context<'_> {
    fn emit(&mut self, pos: [i32; 3]) {
        (self.emit)(pos);
    }
    fn metadata(&self) -> &dyn BlockMetadata {
        self.held.metadata
    }
    fn random(&mut self) -> &mut dyn StructureRandom {
        self
    }
    fn min_y(&mut self) -> i32 {
        self.inner.min_y()
    }
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32> {
        self.inner.height(engine_heightmap(kind), x, z)
    }
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockState> {
        let state = self.inner.block(pos)?;
        self.held.to_api(state)
    }
}

/// The engine's placement-modifier type around a mod's.
struct TypeAdapter {
    id: String,
    ty: Arc<dyn PlacementModifierType>,
    /// Set once one of the type's modifiers failed: every later call fails explicitly.
    off: Arc<AtomicBool>,
}
impl ModPlacementModifierType for TypeAdapter {
    fn decode(
        &self,
        config: &serde_json::Value,
        parts: &mut dyn ModFeatureParts,
    ) -> Result<Arc<dyn ModPlacementModifier>, String> {
        let held = Held::of(parts.registry())?;
        let config = json_to_tag(config).map_err(|e| e.to_string())?;
        let mut decoder = Decoder { parts, held: &held };
        let modifier = horizon_client::crash::contain(|| self.ty.decode(&config, &mut decoder))
            .map_err(|panic| format!("decoding panicked: {panic}"))?
            .map_err(|error| error.0)?;
        Ok(Arc::new(ModifierAdapter {
            id: self.id.clone(),
            inner: modifier,
            held,
            off: self.off.clone(),
        }))
    }
}

/// The engine's modifier around a mod's decoded one.
struct ModifierAdapter {
    id: String,
    inner: Arc<dyn PlacementModifier>,
    held: Held,
    off: Arc<AtomicBool>,
}
impl ModPlacementModifier for ModifierAdapter {
    fn modify(
        &self,
        target: &mut dyn ModPlacementTarget,
        origin: [i32; 3],
        emit: &mut dyn FnMut([i32; 3]),
    ) -> Result<(), String> {
        if self.off.load(Ordering::Acquire) {
            return Err(format!(
                "placement modifier type {} is switched off after an earlier failure",
                self.id
            ));
        }
        let mut context = Context {
            inner: target,
            emit,
            held: &self.held,
        };
        let failed =
            match horizon_client::crash::contain(|| self.inner.modify(&mut context, origin)) {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(error)) => error.0,
                Err(panic) => format!("panicked: {panic}"),
            };
        self.off.store(true, Ordering::Release);
        Err(format!(
            "{failed}; placement modifier type {} is switched off for this session",
            self.id
        ))
    }
}

fn installed_placement_modifier_types() -> ModPlacementModifierTypes {
    mod_api::__private::placement_modifier_type_declarations()
        .into_iter()
        .map(|(id, ty)| {
            let adapter = TypeAdapter {
                id: id.clone(),
                ty,
                off: Arc::default(),
            };
            (id, Arc::new(adapter) as Arc<dyn ModPlacementModifierType>)
        })
        .collect()
}

/// Hand the placement-modifier types the mods registered to the engine's placement decoder,
/// before any world opens.
pub(in crate::game) fn install() {
    if install_mod_placement_modifier_types(installed_placement_modifier_types()).is_err() {
        log::warn!("Mod placement modifier types already installed; keeping the first table");
    }
}

#[cfg(test)]
#[path = "native_placement_modifier_types/tests.rs"]
mod tests;
