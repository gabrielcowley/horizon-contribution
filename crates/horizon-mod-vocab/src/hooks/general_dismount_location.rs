//! The `general_dismount_location` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Where `rider` gets off a general entity row of type `kind`
    /// (`mymod:seat`), the vehicle `vehicle`
    /// (`Entity.getDismountLocationForPassenger`): asked once each time a
    /// rider gets off, never per tick. `location` holds vanilla's default,
    /// the vehicle's x and z at the top of its box; change it to let the
    /// rider off elsewhere. Not asked when the vehicle has been removed or
    /// stands in a portal block: then, as vanilla's
    /// `LivingEntity.dismountVehicle` does, the rider stays at its own x and
    /// z, at the higher of its own and the vehicle's height. Read the world,
    /// don't change it. Native mods only for now: a wasm vehicle lets its
    /// riders off at the default. Server-side.
    #[hook(
        side = "server",
        contract = "collect",
        out = "location",
        scope = "entity",
        key = "kind",
        neoforge = "none (vanilla Entity.getDismountLocationForPassenger, overridden per entity class)"
    )]
    fn general_dismount_location();
}
