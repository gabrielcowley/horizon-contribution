//! The `general_hurt` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// A hit on one general entity row of type `kind` with a living body
    /// (`GeneralBody::Living`), with ID `entity`, before vanilla's
    /// `LivingEntity.hurtServer` runs on it: the Original's `hurt` override.
    /// `damage_type` is the source's damage type (`minecraft:on_fire`),
    /// `amount` the damage, `causing` and `direct` the source's entities.
    /// Answer `Some(taken)` to handle the hit yourself (vanilla's hurt does not
    /// run; `taken` is what the hit reports to its source: an arrow that hits
    /// a row which answers `false` passes through), or `None` to leave it to
    /// the next part and then to vanilla's. The row is in place and the host is
    /// the full server host, so the part may discard the row, drop items, set
    /// its fire or health. A fire-immune row hears no fire hit. A panicking
    /// part is contained and vanilla's hurt runs. Server-side; told only for
    /// general entity types.
    #[hook(
        side = "server",
        contract = "replace",
        out = "taken",
        scope = "entity",
        key = "kind",
        target = "horizon_client::game::native_hooks::general_hurt::Game<'_>::native_general_hurt",
        neoforge = "none (vanilla Entity.hurtServer override; NeoForge's LivingIncomingDamageEvent is the global hook)"
    )]
    fn general_hurt(
        kind: &str,
        entity: EntityRef,
        damage_type: &str,
        amount: f32,
        causing: Option<EntityRef>,
        direct: Option<EntityRef>,
        taken: &mut Option<bool>,
    );
}
