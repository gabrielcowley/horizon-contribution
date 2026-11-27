//! Placement-modifier types a mod registers (vanilla's `PlacementModifier` with its own
//! `modify`), handed over once at startup and looked up when an entry of a placed feature's
//! `placement` list names one (`{"type": "modid:filter", ...}`). The type decodes its settings
//! once, when the placed feature loads; the decoded modifier then runs inside the placement
//! chain (`placement/chain.rs`), in vanilla's depth-first order, wherever a placed feature
//! places: decoration on the worldgen workers, nested placed features and growth on the server
//! thread.
//!
//! A modifier sees what vanilla's `PlacementContext` gives a built-in one and no more: the
//! caller's exact random, the blocks, the heightmaps and the lowest block Y. It pushes each
//! output position in order (vanilla's `Consumer<BlockPos>`); the chain then runs the next
//! modifier, or the feature, on them. The positions of one call reach the chain only once the
//! call returns successfully. A failed call (a native unwind or a wasm trap the adapter caught,
//! the mod's own error, or the work budget running out) passes none of them on and fails the
//! placed feature explicitly, as a failed built-in query does.
use super::{
    DecodeError,
    block_feature::FeatureBlockBindings,
    mod_features::{Draws, ModFeatureParts, ModFeatureRandom, Parts},
    placement::{PlacementContext, PlacementError, PlacementQueryError},
};
use crate::heightmap::HeightmapType;
use crate::registry::BlockStateId;
use horizon_core::random::RandomSource;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

/// The reads one call of a mod's modifier may make, and separately the output positions it may
/// push. Vanilla's widest built-in modifier, `count` at its codec's maximum of 4096, emits far
/// fewer.
pub const MODIFIER_WORK: u32 = 65_536;

/// A placement-modifier type's engine side, as the executable's adapter sees it.
pub trait ModPlacementModifierType: Send + Sync {
    /// The modifier's settings (its JSON object without `type`) into a modifier; `Err` refuses
    /// the placed feature, as a malformed built-in modifier is refused.
    fn decode(
        &self,
        config: &serde_json::Value,
        parts: &mut dyn ModFeatureParts,
    ) -> Result<Arc<dyn ModPlacementModifier>, String>;
}

/// A decoded modifier: vanilla's `PlacementModifier.modify(context, random, origin, output)`.
pub trait ModPlacementModifier: Send + Sync {
    /// Push each output position for `origin` to `emit`, in order: `Ok` when the call
    /// succeeded, `Err` when it failed (its positions are then dropped).
    fn modify(
        &self,
        target: &mut dyn ModPlacementTarget,
        origin: [i32; 3],
        emit: &mut dyn FnMut([i32; 3]),
    ) -> Result<(), String>;
}

/// What a mod's modifier reads: vanilla's `PlacementContext` without its level and generator.
/// A world error or an exhausted budget ends the call once the mod returns; until then reads
/// answer `None` and outputs are dropped.
pub trait ModPlacementTarget {
    /// `PlacementContext.getMinY`: the level's lowest block Y.
    fn min_y(&mut self) -> i32;
    /// The caller's random.
    fn random(&mut self) -> &mut dyn ModFeatureRandom;
    /// `PlacementContext.getHeight` of the heightmap kind at (x, z).
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32>;
    /// `PlacementContext.getBlockState`, or `None` outside the world's sections.
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockStateId>;
}

/// The installed placement-modifier types, `modid:filter` → type. Set once, before any world
/// opens (the executable hands the mods' registrations over).
pub type ModPlacementModifierTypes = BTreeMap<String, Arc<dyn ModPlacementModifierType>>;

static MOD_MODIFIER_TYPES: OnceLock<ModPlacementModifierTypes> = OnceLock::new();
static EMPTY: ModPlacementModifierTypes = BTreeMap::new();

#[cfg(test)]
thread_local! {
    /// The types a test installs, in place of the process-wide table.
    static TEST_TYPES: std::cell::Cell<Option<&'static ModPlacementModifierTypes>> =
        const { std::cell::Cell::new(None) };
}

/// Install the process-wide table once; a second installation keeps the first.
pub fn install_mod_placement_modifier_types(
    types: ModPlacementModifierTypes,
) -> Result<(), ModPlacementModifierTypes> {
    MOD_MODIFIER_TYPES.set(types)
}

/// The installed placement-modifier types: empty until the executable installs the mods', and
/// a test's own while one is installed.
pub fn mod_placement_modifier_types() -> &'static ModPlacementModifierTypes {
    #[cfg(test)]
    if let Some(types) = TEST_TYPES.get() {
        return types;
    }
    MOD_MODIFIER_TYPES.get().unwrap_or(&EMPTY)
}

#[cfg(test)]
/// Runs `with` over a test's own types, as the placement decoder reads them.
pub(crate) fn with_test_types<T>(types: &ModPlacementModifierTypes, with: impl FnOnce() -> T) -> T {
    let leaked: &'static ModPlacementModifierTypes = Box::leak(Box::new(types.clone()));
    let prev = TEST_TYPES.replace(Some(leaked));
    let out = with();
    TEST_TYPES.set(prev);
    out
}

/// A mod's decoded modifier in a placed feature's chain: the type id it came from and the
/// mod's modifier.
#[derive(Clone)]
pub struct ModPlacement {
    id: String,
    modifier: Arc<dyn ModPlacementModifier>,
}
impl std::fmt::Debug for ModPlacement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ModPlacement({})", self.id)
    }
}

/// Why one call failed.
enum Failure {
    Query(PlacementQueryError),
    WorkLimit,
}

impl ModPlacement {
    /// Whether `kind` names a mod's placement-modifier type.
    pub(crate) fn is_mod_type(kind: &str) -> bool {
        mod_placement_modifier_types().contains_key(kind)
    }
    /// Decode the modifier `json` (its `type` the mod's) once, at load.
    pub(crate) fn decode(
        kind: &str,
        json: &str,
        bindings: &FeatureBlockBindings<'_>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Self, DecodeError> {
        let ty = mod_placement_modifier_types()
            .get(kind)
            .ok_or(DecodeError::Field("mod placement modifier type"))?;
        let mut config: serde_json::Map<String, serde_json::Value> = serde_json::from_str(json)?;
        config.remove("type");
        let mut parts = Parts {
            bindings,
            depth: depth + 1,
            remaining,
            rules: Vec::new(),
        };
        let modifier = ty
            .decode(&serde_json::Value::Object(config), &mut parts)
            .map_err(|message| {
                DecodeError::Registry(format!("placement modifier {kind}: {message}"))
            })?;
        Ok(Self {
            id: kind.to_owned(),
            modifier,
        })
    }
    /// `PlacementModifier.modify` at `origin`, with the chain's context and random; the call's
    /// positions go to `emit` in order once it succeeded.
    pub(crate) fn modify<C: PlacementContext, R: RandomSource, E>(
        &self,
        context: &mut C,
        random: &mut R,
        origin: [i32; 3],
        emit: &mut impl FnMut(&mut R, [i32; 3]) -> Result<(), E>,
    ) -> Result<(), PlacementError<E>> {
        let mut target = Target {
            context,
            draws: Draws {
                random,
                invalid: None,
            },
            work: MODIFIER_WORK,
            failure: None,
        };
        let mut outputs = Vec::new();
        let mut overflow = false;
        let called = self.modifier.modify(&mut target, origin, &mut |p| {
            if outputs.len() >= MODIFIER_WORK as usize {
                overflow = true;
            } else {
                outputs.push(p);
            }
        });
        let Target { draws, failure, .. } = target;
        let failure = match (called, failure, draws.invalid) {
            (_, Some(Failure::Query(e)), _) => Some(PlacementError::World(e)),
            (_, Some(Failure::WorkLimit), _) => Some(PlacementError::WorkLimit),
            _ if overflow => Some(PlacementError::WorkLimit),
            (Err(message), None, _) => Some(PlacementError::World(PlacementQueryError::Failed(
                format!("mod placement modifier {}: {message}", self.id),
            ))),
            (Ok(()), None, Some(invalid)) => Some(PlacementError::Random(invalid)),
            (Ok(()), None, None) => None,
        };
        if let Some(error) = failure {
            let reason = match &error {
                PlacementError::WorkLimit => {
                    format!("more than {MODIFIER_WORK} reads or output positions")
                }
                PlacementError::World(e) => e.to_string(),
                PlacementError::Random(e) => e.to_string(),
                PlacementError::Output(_) => "its output was refused".to_owned(),
            };
            log::error!(
                "Horizon: mod placement modifier {} failed at {origin:?}, none of its positions \
                 were used: {reason}",
                self.id
            );
            return Err(error);
        }
        for p in outputs {
            emit(draws.random, p).map_err(PlacementError::Output)?;
        }
        Ok(())
    }
}

/// The chain's context, as a mod's modifier sees it during one call.
struct Target<'t, C, R> {
    context: &'t mut C,
    draws: Draws<'t, R>,
    work: u32,
    failure: Option<Failure>,
}
impl<C: PlacementContext, R> Target<'_, C, R> {
    /// Charge one read; false once the call failed or its budget ran out.
    fn charge(&mut self) -> bool {
        if self.failure.is_some() {
            return false;
        }
        if self.work == 0 {
            self.failure = Some(Failure::WorkLimit);
            return false;
        }
        self.work -= 1;
        true
    }
    fn query<T>(&mut self, answer: Result<T, PlacementQueryError>) -> Option<T> {
        answer
            .map_err(|e| {
                self.failure.get_or_insert(Failure::Query(e));
            })
            .ok()
    }
}
impl<C: PlacementContext, R: RandomSource> ModPlacementTarget for Target<'_, C, R> {
    fn min_y(&mut self) -> i32 {
        let y = self.context.min_y();
        self.query(y).unwrap_or(0)
    }
    fn random(&mut self) -> &mut dyn ModFeatureRandom {
        &mut self.draws
    }
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32> {
        if !self.charge() {
            return None;
        }
        let height = self.context.height(kind, x, z);
        self.query(height)
    }
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockStateId> {
        if !self.charge() {
            return None;
        }
        let state = self.context.section_block(pos);
        self.query(state).flatten()
    }
}

#[cfg(test)]
mod tests;
