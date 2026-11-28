//! The `general_passenger_removed` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// `rider` has left a general entity row of type `kind` (`mymod:seat`),
    /// the vehicle `vehicle`, for `reason` (`Entity.removePassenger`): it got
    /// off, the vehicle was removed, or the rider was removed, died or left
    /// the level. Told once per rider and ride, after the rider is off and
    /// placed: undo here what seating it did, such as a tamed animal's
    /// sitting pose (`living_set_in_sitting_pose`). Native mods only for
    /// now. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        scope = "entity",
        key = "kind",
        neoforge = "none (vanilla Entity.removePassenger, overridden per entity class)"
    )]
    fn general_passenger_removed();
}
