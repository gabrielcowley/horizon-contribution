//! Feature types a mod registers (vanilla's `Feature` with its own `place`), handed over once at
//! startup and looked up when a configured feature's `type` names one (`{"type": "modid:vein",
//! ...}`, 26.3's inline shape). The type decodes its settings once, when the feature loads; the
//! decoded feature then places through one target, the same on the worldgen workers (decoration)
//! and on the server thread (nested placed features, growth, `/place feature`).
//!
//! The target borrows the caller's world and its exact random: draws interleave with the
//! placement chain's as a built-in feature's do. Writes are staged and reach the world only when
//! the mod's call returns successfully, in the order it made them; reads see the staged writes.
//! A failed call (a native unwind or a wasm trap the adapter caught, the mod's own error, or the
//! work budget running out) leaves the world as it was and fails the feature explicitly.
use super::{
    DecodeError,
    block_feature::FeatureBlockBindings,
    feature::{FeatureError, FeatureWorld},
    feature_graph::FeatureExecutionError,
    placement::{PlacementContext, PlacementQueryError},
    rule_test::RuleTest,
};
use crate::heightmap::HeightmapType;
use crate::registry::{BlockRegistry, BlockStateId};
use horizon_core::random::{InvalidBound, RandomSource};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock};

/// The block reads and writes one call of a mod's feature may make. `layered_ore`-sized
/// features (a radius-16 ball, one read and at most one write per block) use about half.
pub const FEATURE_WORK: u32 = 65_536;

/// A feature type's engine side, as the executable's adapter sees it.
pub trait ModFeatureType: Send + Sync {
    /// The configured feature's settings (its JSON object without `type`) into a feature; `Err`
    /// refuses the feature, as a malformed built-in one is refused.
    fn decode(
        &self,
        config: &serde_json::Value,
        parts: &mut dyn ModFeatureParts,
    ) -> Result<Arc<dyn ModConfiguredFeature>, String>;
}

/// The decoders a type's `decode` borrows for the vanilla values a configuration embeds.
pub trait ModFeatureParts {
    /// The registry the decoded states belong to.
    fn registry(&self) -> &BlockRegistry;
    /// `BlockState.CODEC`: a block id, or an `{"id": ..., "properties": {...}}` object.
    fn block_state(&mut self, value: &serde_json::Value) -> Result<BlockStateId, String>;
    /// `RuleTest.CODEC`: the rule, kept with the feature and named by the handle it returns.
    fn rule_test(&mut self, value: &serde_json::Value) -> Result<u32, String>;
}

/// A decoded feature: vanilla's `Feature.place`.
pub trait ModConfiguredFeature: Send + Sync {
    /// Place at the target's origin: `Ok(true)` when something was placed, `Ok(false)` when
    /// nothing was, `Err` when the call failed (its writes are then dropped).
    fn place(&self, target: &mut dyn ModFeatureTarget) -> Result<bool, String>;
}

/// The caller's random, as a mod's feature draws from it. A `next_int` bound below 1, which
/// vanilla refuses with an exception, draws nothing and answers 0; the feature then fails once
/// the mod returns.
pub trait ModFeatureRandom {
    fn int(&mut self, bound: i32) -> i32;
    fn long(&mut self) -> i64;
    fn float(&mut self) -> f32;
    fn double(&mut self) -> f64;
    fn boolean(&mut self) -> bool;
}

/// What a mod's feature places through. A world error or an exhausted budget ends the feature
/// once the mod returns; until then reads answer `None` or false and writes are dropped.
pub trait ModFeatureTarget {
    /// The position the feature places at.
    fn origin(&self) -> [i32; 3];
    /// The level's lowest block Y.
    fn min_y(&mut self) -> i32;
    /// The level's highest block Y (inclusive).
    fn max_y(&mut self) -> i32;
    /// The caller's random.
    fn random(&mut self) -> &mut dyn ModFeatureRandom;
    /// `WorldGenLevel.getHeight`.
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32>;
    /// `WorldGenLevel.ensureCanWrite`.
    fn can_write(&mut self, pos: [i32; 3]) -> bool;
    /// The block at `pos` (a `BulkSectionAccess` read, with this call's own writes), or `None`
    /// outside the world's sections.
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockStateId>;
    /// A raw section write, as `BulkSectionAccess` makes for vanilla's `ore`: no updates,
    /// heightmaps or block entities. False where the target can't write.
    fn set_section_block(&mut self, pos: [i32; 3], state: BlockStateId) -> bool;
    /// `WorldGenLevel.setBlock(pos, state, flags)`. False where the target can't write.
    fn set_block(&mut self, pos: [i32; 3], state: BlockStateId, flags: i32) -> bool;
    /// `BlockState.isAir`.
    fn is_air(&mut self, state: BlockStateId) -> bool;
    /// `RuleTest.test(state, random)` of a rule the feature decoded, with the caller's random.
    /// False for a handle the feature didn't decode.
    fn rule_test(&mut self, rule: u32, state: BlockStateId, pos: [i32; 3]) -> bool;
}

/// The installed feature types, `modid:vein` → type. Set once, before any world opens (the
/// executable hands the mods' registrations over).
pub type ModFeatureTypes = BTreeMap<String, Arc<dyn ModFeatureType>>;

static MOD_FEATURE_TYPES: OnceLock<ModFeatureTypes> = OnceLock::new();
static EMPTY: ModFeatureTypes = BTreeMap::new();

#[cfg(test)]
thread_local! {
    /// The types a test installs, in place of the process-wide table.
    static TEST_TYPES: std::cell::Cell<Option<&'static ModFeatureTypes>> =
        const { std::cell::Cell::new(None) };
}

/// Install the process-wide table once; a second installation keeps the first.
pub fn install_mod_feature_types(types: ModFeatureTypes) -> Result<(), ModFeatureTypes> {
    MOD_FEATURE_TYPES.set(types)
}

/// The installed feature types: empty until the executable installs the mods', and a test's own
/// while one is installed.
pub fn mod_feature_types() -> &'static ModFeatureTypes {
    #[cfg(test)]
    if let Some(types) = TEST_TYPES.get() {
        return types;
    }
    MOD_FEATURE_TYPES.get().unwrap_or(&EMPTY)
}

#[cfg(test)]
/// Runs `with` over a test's own types, as the feature decoder reads them.
pub(crate) fn with_test_types<T>(types: &ModFeatureTypes, with: impl FnOnce() -> T) -> T {
    let leaked: &'static ModFeatureTypes = Box::leak(Box::new(types.clone()));
    let prev = TEST_TYPES.replace(Some(leaked));
    let out = with();
    TEST_TYPES.set(prev);
    out
}

/// A mod's decoded feature in the feature graph: the type id it came from, the mod's feature
/// and the rule tests it decoded.
#[derive(Clone)]
pub struct ModFeature {
    id: String,
    feature: Arc<dyn ModConfiguredFeature>,
    rules: Arc<[RuleTest]>,
    air: [BlockStateId; 3],
}
impl std::fmt::Debug for ModFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ModFeature({})", self.id)
    }
}

/// The engine's decoders over the feature's block bindings (a placement modifier's too).
pub(super) struct Parts<'b, 'r> {
    pub(super) bindings: &'b FeatureBlockBindings<'r>,
    pub(super) depth: usize,
    pub(super) remaining: &'b mut usize,
    pub(super) rules: Vec<RuleTest>,
}
impl ModFeatureParts for Parts<'_, '_> {
    fn registry(&self) -> &BlockRegistry {
        self.bindings.registry()
    }
    fn block_state(&mut self, value: &serde_json::Value) -> Result<BlockStateId, String> {
        self.bindings
            .codec
            .decode(&value.to_string(), false)
            .map_err(|e| e.to_string())
    }
    fn rule_test(&mut self, value: &serde_json::Value) -> Result<u32, String> {
        let handle = u32::try_from(self.rules.len()).map_err(|_| "too many rule tests")?;
        let rule = RuleTest::decode(
            &value.to_string(),
            self.bindings,
            self.depth,
            self.remaining,
        )
        .map_err(|e| e.to_string())?;
        self.rules.push(rule);
        Ok(handle)
    }
}

impl ModFeature {
    /// Whether `kind` names a mod's feature type.
    pub(crate) fn is_mod_type(kind: &str) -> bool {
        mod_feature_types().contains_key(kind)
    }
    /// Decode the configured feature `object` (its `type` the mod's) once, at load.
    pub(crate) fn decode(
        kind: &str,
        object: &serde_json::Map<String, serde_json::Value>,
        bindings: &FeatureBlockBindings<'_>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Self, DecodeError> {
        let ty = mod_feature_types()
            .get(kind)
            .ok_or(DecodeError::Field("mod feature type"))?;
        let mut config = object.clone();
        config.remove("type");
        let mut parts = Parts {
            bindings,
            depth: depth + 1,
            remaining,
            rules: Vec::new(),
        };
        let feature = ty
            .decode(&serde_json::Value::Object(config), &mut parts)
            .map_err(|message| DecodeError::Registry(format!("feature {kind}: {message}")))?;
        Ok(Self {
            id: kind.to_owned(),
            feature,
            rules: parts.rules.into(),
            air: bindings.air,
        })
    }
    /// `Feature.place` at `origin`, with the caller's world and random.
    pub(crate) fn place<W: FeatureWorld + PlacementContext>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: [i32; 3],
    ) -> Result<bool, FeatureExecutionError<W::Error>> {
        let mut target = Target {
            world,
            draws: Draws {
                random,
                invalid: None,
            },
            origin,
            rules: &self.rules,
            air: self.air,
            staged: Staged::default(),
            work: FEATURE_WORK,
            error: None,
        };
        let placed = self.feature.place(&mut target);
        let Target {
            world,
            draws,
            staged,
            error,
            ..
        } = target;
        let failure = match (placed, error, draws.invalid) {
            (_, Some(error), _) => Err(error),
            (Err(message), None, _) => Err(FeatureExecutionError::Mod(message)),
            (Ok(_), None, Some(invalid)) => Err(FeatureExecutionError::Random(invalid)),
            (Ok(placed), None, None) => Ok(placed),
        };
        let placed = failure.inspect_err(|error| {
            let reason = match error {
                FeatureExecutionError::Mod(message) => message.clone(),
                FeatureExecutionError::Random(e) => e.to_string(),
                FeatureExecutionError::Query(e) => e.to_string(),
                FeatureExecutionError::WorkLimit => {
                    format!("more than {FEATURE_WORK} reads and writes")
                }
                _ => "the world refused a write".into(),
            };
            log::error!(
                "Horizon: mod feature {} failed at {origin:?}, nothing it wrote was kept: {reason}",
                self.id
            );
        })?;
        staged.commit(world)?;
        Ok(placed)
    }
}

/// The caller's random as a mod's feature (or placement modifier) draws from it.
pub(super) struct Draws<'a, R> {
    pub(super) random: &'a mut R,
    pub(super) invalid: Option<InvalidBound>,
}
impl<R: RandomSource> ModFeatureRandom for Draws<'_, R> {
    fn int(&mut self, bound: i32) -> i32 {
        self.random.next_int_bound(bound).unwrap_or_else(|invalid| {
            self.invalid.get_or_insert(invalid);
            0
        })
    }
    fn long(&mut self) -> i64 {
        self.random.next_long()
    }
    fn float(&mut self) -> f32 {
        self.random.next_float()
    }
    fn double(&mut self) -> f64 {
        self.random.next_double()
    }
    fn boolean(&mut self) -> bool {
        self.random.next_boolean()
    }
}

/// One staged write.
#[derive(Clone, Copy)]
enum Write {
    Section([i32; 3], BlockStateId),
    Block([i32; 3], BlockStateId, i32),
}

/// The writes of one call, in order, and the latest state at each position.
#[derive(Default)]
struct Staged {
    writes: Vec<Write>,
    latest: HashMap<[i32; 3], BlockStateId>,
}
impl Staged {
    fn push(&mut self, write: Write) {
        let (Write::Section(pos, state) | Write::Block(pos, state, _)) = write;
        self.latest.insert(pos, state);
        self.writes.push(write);
    }
    fn commit<W: FeatureWorld + PlacementContext>(
        self,
        world: &mut W,
    ) -> Result<(), FeatureExecutionError<W::Error>> {
        for write in self.writes {
            match write {
                Write::Section(pos, state) => world
                    .set_section_block(pos, state)
                    .map_err(FeatureExecutionError::Query)?,
                Write::Block(pos, state, flags) => {
                    world
                        .set_block(pos, state, flags)
                        .map_err(|e| FeatureExecutionError::Basic(FeatureError::World(e)))?;
                }
            }
        }
        Ok(())
    }
}

/// The world, as a mod's feature sees it during one call.
struct Target<'t, W: FeatureWorld, R> {
    world: &'t mut W,
    draws: Draws<'t, R>,
    origin: [i32; 3],
    rules: &'t [RuleTest],
    air: [BlockStateId; 3],
    staged: Staged,
    work: u32,
    error: Option<FeatureExecutionError<W::Error>>,
}
impl<W: FeatureWorld + PlacementContext, R: RandomSource> Target<'_, W, R> {
    /// Charge one read or write; false once the call failed or its budget ran out.
    fn charge(&mut self) -> bool {
        if self.error.is_some() {
            return false;
        }
        if self.work == 0 {
            self.error = Some(FeatureExecutionError::WorkLimit);
            return false;
        }
        self.work -= 1;
        true
    }
    fn query<T>(&mut self, answer: Result<T, PlacementQueryError>) -> Option<T> {
        answer
            .map_err(|e| {
                self.error.get_or_insert(FeatureExecutionError::Query(e));
            })
            .ok()
    }
    fn writable(&mut self, pos: [i32; 3]) -> bool {
        if !self.charge() {
            return false;
        }
        let inside = self
            .world
            .outside_build_height(pos[1])
            .map(|outside| !outside);
        self.query(inside).unwrap_or(false) && {
            let can = self.world.ensure_can_write(pos);
            self.query(can).unwrap_or(false)
        }
    }
}
impl<W: FeatureWorld + PlacementContext, R: RandomSource> ModFeatureTarget for Target<'_, W, R> {
    fn origin(&self) -> [i32; 3] {
        self.origin
    }
    fn min_y(&mut self) -> i32 {
        let y = self.world.min_y();
        self.query(y).unwrap_or(0)
    }
    fn max_y(&mut self) -> i32 {
        let y = self.world.max_y();
        self.query(y).unwrap_or(-1)
    }
    fn random(&mut self) -> &mut dyn ModFeatureRandom {
        &mut self.draws
    }
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32> {
        let height = self.world.height(kind, x, z);
        self.query(height)
    }
    fn can_write(&mut self, pos: [i32; 3]) -> bool {
        let can = self.world.ensure_can_write(pos);
        self.query(can).unwrap_or(false)
    }
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockStateId> {
        if !self.charge() {
            return None;
        }
        if let Some(&state) = self.staged.latest.get(&pos) {
            return Some(state);
        }
        let state = self.world.section_block(pos);
        self.query(state).flatten()
    }
    fn set_section_block(&mut self, pos: [i32; 3], state: BlockStateId) -> bool {
        if !self.writable(pos) {
            return false;
        }
        self.staged.push(Write::Section(pos, state));
        true
    }
    fn set_block(&mut self, pos: [i32; 3], state: BlockStateId, flags: i32) -> bool {
        if !self.writable(pos) {
            return false;
        }
        self.staged.push(Write::Block(pos, state, flags));
        true
    }
    fn is_air(&mut self, state: BlockStateId) -> bool {
        self.air.contains(&state)
    }
    fn rule_test(&mut self, rule: u32, state: BlockStateId, pos: [i32; 3]) -> bool {
        let rules = self.rules;
        let Some(rule) = usize::try_from(rule).ok().and_then(|i| rules.get(i)) else {
            return false;
        };
        rule.test(state, pos, &mut *self.draws.random)
    }
}

#[cfg(test)]
mod tests;
