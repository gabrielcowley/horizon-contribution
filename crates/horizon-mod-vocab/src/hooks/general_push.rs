//! The `general_push` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Whether the entity `by` pushes a general entity row of type `kind`
    /// with a living body, with ID `entity`, as their boxes meet: vanilla's
    /// `Entity.push(Entity)` override, asked before the push moves anything.
    /// Answer `Some(true)` for vanilla's push, `Some(false)` to stay put, or
    /// `None` to leave it to the next part and then push. The host is the
    /// full server host, so the part may change the row as the push meets it.
    /// Asked only for pairs whose boxes already meet. Server-side; told only
    /// for general entity types.
    #[hook(
        side = "server",
        contract = "replace",
        out = "pushed",
        scope = "entity",
        key = "kind",
        neoforge = "none (vanilla Entity.push(Entity) override)"
    )]
    fn general_push();
}
