use super::*;

/// The bounds for general entity types, declared
/// once so the registrar, the codecs and the tests quote the same numbers.
/// The payload bound applies to mod-owned payload bytes only, never to a
/// record of a vanilla type. The quarantine bounds (with no vanilla
/// analogue) cap what the game preserves of a missing mod's records.
// exhaustive: the design's limits are one block; no further constants join it
pub mod limits {
    /// The most payload bytes one general entity record may carry. A payload
    /// one byte larger is refused, never truncated into a valid-looking
    /// entity.
    pub const PAYLOAD_LIMIT: usize = 65_536;
    /// The deepest nesting of a general entity's decoded data. The
    /// payload decode checks it; the record tree itself has the stricter
    /// passenger-depth bound.
    pub const MAX_NESTING: u32 = 32;
    /// The deepest chain of passengers a general entity tree may carry.
    pub const MAX_PASSENGER_DEPTH: u32 = 16;
    /// The longest side, in blocks, of a box a general entity is given with
    /// `entity_set_bounds` or `spawn_general`: the design's size bound.
    pub const MAX_BOX_EXTENT: f64 = 64.0;
    /// The largest record a missing mod's quarantined general entity may
    /// occupy, as its encoded NBT size. A larger record is skipped with a
    /// warning rather than preserved. There is no vanilla analogue:
    /// vanilla deletes unknown-mod entities outright, and Horizon's quarantine
    /// preserves records for missing mods.
    pub const QUARANTINE_RECORD_LIMIT: u64 = 69_632;
    /// The most quarantined general entity records one chunk may carry. The
    /// next one is skipped with a warning. There is no vanilla analogue:
    /// vanilla has no quarantine to bound.
    pub const QUARANTINE_CHUNK_LIMIT: usize = 256;
}

/// A general entity type's spawn category (supplied at
/// declaration; general types start as `MISC`, which never gains natural
/// spawning implicitly).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub enum GeneralEntityCategory {
    /// Neither monster nor creature: no natural-spawn policy of its own.
    Misc,
}

/// Whether instances of a declared general type persist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub enum GeneralSavePolicy {
    /// Instances save like ordinary entities do.
    Save,
    /// `noSave`: instances produce zero persisted records.
    NoSave,
}

/// A general entity type a mod declares, as the game reads it when it freezes
/// the type registry. The registry is data: no mod type adds an entry
/// to any hand-kept engine list.
#[derive(Clone, Debug)]
// exhaustive: only registration builds it; the game reads it
#[derive(horizon_mod_api_macros::Wire)]
pub struct GeneralEntityTypeDeclaration {
    /// The declaring mod's id: the first key of the registry's sort order.
    pub mod_id: String,
    /// The type's namespaced id (`mymod:robot`).
    pub id: String,
    /// The type's spawn category.
    pub category: GeneralEntityCategory,
    /// The type's finite width in blocks, as supplied to the vanilla builder.
    pub width: f64,
    /// The type's finite height in blocks, as supplied to the vanilla builder.
    pub height: f64,
    /// Whether instances of the type persist.
    pub save_policy: GeneralSavePolicy,
    /// Whether the type may be summoned by name.
    pub summonable: bool,
    /// Tracking distance in chunks, stored directly like vanilla's signed integer.
    pub tracking_chunks: i32,
    /// Update interval in ticks, stored directly like vanilla's signed integer.
    /// `i32::MAX` is vanilla's no-update-interval value.
    pub update_interval: i32,
    /// Whether trackers hear the instance's velocity whenever it changes
    /// (vanilla's `setShouldReceiveVelocityUpdates`, `true` by default).
    /// Without it, the velocity reaches clients only in the spawn packet.
    pub sends_velocity: bool,
    /// The vanilla behaviour set the type's rows run ([`GeneralBody`]):
    /// [`GeneralBody::None`] until `general_entity_body` sets one.
    pub body: GeneralBody,
    /// Whether fire and lava leave the type's rows unhurt and unlit (vanilla's
    /// `EntityType.Builder.fireImmune`), `false` until `general_entity_fire_immune`
    /// sets it. A row may change its own with `entity_set_fire_immune`.
    pub fire_immune: bool,
    /// The payload schema version instances of the type carry.
    pub payload_version: u16,
    /// The ids of loaded mods this type requires, every one of which must be
    /// present for the declaration to stand.
    pub required_features: Vec<String>,
}

/// A placeholder to build a declaration from: empty ids, a miscellaneous type that saves, vanilla's
/// default player-sized box and no requirements. Registration validates what a mod sets.
impl Default for GeneralEntityTypeDeclaration {
    fn default() -> Self {
        Self {
            mod_id: String::new(),
            id: String::new(),
            category: GeneralEntityCategory::Misc,
            width: 0.6,
            height: 1.8,
            save_policy: GeneralSavePolicy::Save,
            summonable: true,
            tracking_chunks: 5,
            update_interval: 3,
            sends_velocity: true,
            body: GeneralBody::None,
            fire_immune: false,
            payload_version: 1,
            required_features: Vec::new(),
        }
    }
}

/// The vanilla behaviour set a general entity type's rows run, lifted out of
/// the vanilla family that has it, so a mod's type moves, burns and takes
/// hits as that family does.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub enum GeneralBody {
    /// No physics: the row stays where it is put and nothing hurts it.
    None,
    /// Vanilla's `LivingEntity` without AI, the armor stand's path: gravity,
    /// fluids, block collision, pushing, health from its attribute set
    /// (`declare_entity_attributes` with the `minecraft:armor_stand`
    /// template), fire, lava, drowning, falls, suffocation, the void,
    /// explosions, `/kill` and melee.
    Living(LivingSpec),
}

/// What a [`GeneralBody::Living`] type sounds like and whether potions
/// reach it. A sound is a sound event id (`minecraft:entity.generic.hurt`);
/// `None` is silence.
#[derive(Clone, Debug, PartialEq)]
// exhaustive: built with `LivingSpec::default()` and field updates
#[derive(horizon_mod_api_macros::Wire)]
pub struct LivingSpec {
    /// `LivingEntity.getHurtSound`.
    pub hurt_sound: Option<String>,
    /// `LivingEntity.getDeathSound`.
    pub death_sound: Option<String>,
    /// `LivingEntity.getFallSounds().small()`: a fall of at most 4 blocks of damage.
    pub small_fall_sound: Option<String>,
    /// `LivingEntity.getFallSounds().big()`.
    pub big_fall_sound: Option<String>,
    /// `LivingEntity.isAffectedByPotions`.
    pub affected_by_potions: bool,
}

/// `LivingEntity`'s own sounds (`entity.generic.*`), and potions reach it.
impl Default for LivingSpec {
    fn default() -> Self {
        Self {
            hurt_sound: Some("minecraft:entity.generic.hurt".to_owned()),
            death_sound: Some("minecraft:entity.generic.death".to_owned()),
            small_fall_sound: Some("minecraft:entity.generic.small_fall".to_owned()),
            big_fall_sound: Some("minecraft:entity.generic.big_fall".to_owned()),
            affected_by_potions: true,
        }
    }
}

/// Why a mod's general entity type declaration was refused
/// ([`Registrar::declare_general_entity`](GeneralTypesRegistrar::declare_general_entity)).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub enum GeneralEntityRegistrationError {
    /// The registry is frozen: declarations are only accepted while the game
    /// is still in its registration phase, before any world or connection
    /// opens.
    Frozen,
    /// Only both-side packages may declare entity types, which the network
    /// names.
    WrongSide,
    /// The type's id is malformed, empty or in a namespace the declaring mod
    /// does not own (a vanilla type's included).
    InvalidId,
    /// Another declaration already covers the type.
    Duplicate,
    /// A dimension is NaN or infinite.
    NotFinite,
    /// A required feature names no loaded mod.
    UnknownDependency,
}

/// The general entity types mods declared.
pub fn general_entity_type_declarations() -> &'static [GeneralEntityTypeDeclaration] {
    installed().map_or(&[], |installed| &installed.general_entity_types)
}

/// Registration of general entity types: one frozen registry of
/// namespaced types, filled before worlds or connections open.
pub trait GeneralTypesRegistrar {
    /// Declare a general entity type of the mod's own (vanilla's
    /// `EntityType` builder, NeoForge's `RegisterEvent`): `path` in the mod's
    /// namespace (`robot` of `mymod` is `mymod:robot`), its category, its
    /// finite `width` and `height` in blocks, its save and summon
    /// policy, its signed integer tracking distance and update interval, its payload
    /// schema version and the loaded mods it requires.
    ///
    /// An id outside the mod's manifest-owned namespaces, a malformed id, a
    /// duplicate of this mod's or another's, a non-finite dimension, a required feature naming no
    /// loaded mod, or a declaration after the registry froze (the
    /// registration phase has ended) refuses, naming the mod, without
    /// panicking. Invalid input is refused, never clamped.
    fn declare_general_entity(
        &mut self,
        path: &str,
        category: GeneralEntityCategory,
        width: f64,
        height: f64,
        save_policy: GeneralSavePolicy,
        summonable: bool,
        tracking_chunks: i32,
        update_interval: i32,
        payload_version: u16,
        required_features: &[&str],
    ) -> Result<(), GeneralEntityRegistrationError>;

    /// Whether the trackers of the mod's general type `path` (declared by
    /// [`declare_general_entity`](Self::declare_general_entity) before this
    /// call) hear its velocity each time it changes: vanilla's
    /// `setShouldReceiveVelocityUpdates`, `true` until this sets it. Without
    /// it, a client gets the velocity only in the spawn packet.
    ///
    /// A path that names no type this mod declared refuses with `InvalidId`,
    /// and a call after the registry froze with `Frozen`, naming the mod,
    /// without panicking.
    fn general_entity_sends_velocity(
        &mut self,
        path: &str,
        sends_velocity: bool,
    ) -> Result<(), GeneralEntityRegistrationError>;

    /// The vanilla behaviour set the rows of the mod's general type `path`
    /// (declared before this call) run: vanilla's `LivingEntity` with
    /// [`GeneralBody::Living`]. [`GeneralBody::None`] until this sets one.
    ///
    /// A path that names no type this mod declared refuses with `InvalidId`,
    /// and a call after the registry froze with `Frozen`, naming the mod,
    /// without panicking.
    fn general_entity_body(
        &mut self,
        path: &str,
        body: GeneralBody,
    ) -> Result<(), GeneralEntityRegistrationError>;

    /// Whether the rows of the mod's general type `path` (declared before
    /// this call) are immune to fire (vanilla's `EntityType.Builder.fireImmune`):
    /// fire and lava neither light nor hurt them. `false` until this sets it.
    ///
    /// A path that names no type this mod declared refuses with `InvalidId`,
    /// and a call after the registry froze with `Frozen`, naming the mod,
    /// without panicking.
    fn general_entity_fire_immune(
        &mut self,
        path: &str,
        fire_immune: bool,
    ) -> Result<(), GeneralEntityRegistrationError>;
}
