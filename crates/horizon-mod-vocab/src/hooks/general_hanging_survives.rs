//! The `general_hanging_survives` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Whether the hanging general row `entity` of type `kind` (`mymod:board`, a type with a
    /// [`GeneralBody::Hanging`](crate::GeneralBody::Hanging) body) can stay where it hangs:
    /// the type's own `HangingEntity.survives`. `check` holds vanilla's three conditions as
    /// the engine found them (`unobstructed`, `supported`, `alone`); set `survives` to
    /// `Some(answer)` to decide, for example combining `unobstructed` and `alone` with a
    /// per-block rule of the type's own (`level_hanging_support`,
    /// `level_block_face_sturdy`), or leave it `None` for vanilla's, all three. Asked every 100
    /// ticks of the row's own tick and by `entity_hanging_survives`. A row that does not
    /// survive its tick's check drops its item, with no `ENTITY_DIE` event, unless
    /// `hanging_break` keeps it. Told only for the type the part is attached to. Server-side.
    #[hook(
        side = "server",
        contract = "replace",
        out = "survives",
        scope = "entity",
        key = "kind",
        neoforge = "none (vanilla HangingEntity.survives is overridden by subclasses; NeoForge adds no event for it)"
    )]
    fn general_hanging_survives();
}
