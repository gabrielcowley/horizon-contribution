//! The `general_can_collide_with` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Whether a general entity row of type `kind` with a living body, with
    /// ID `entity`, is stopped by the entity `other` when it moves: vanilla's
    /// `Entity.canCollideWith` override. Asked only for an `other` whose box
    /// already meets the row's move. Answer `Some(collides)`, or `None` for
    /// vanilla's rule (another entity that `canBeCollidedWith`: a boat, a
    /// minecart, a shulker). The host is read-only: answer fast, from the
    /// inputs. A panicking part is contained and vanilla's rule is used.
    #[hook(
        side = "both",
        contract = "replace",
        out = "collides",
        scope = "entity",
        key = "kind",
        host = "read",
        neoforge = "none (vanilla Entity.canCollideWith override)"
    )]
    fn general_can_collide_with();
}
