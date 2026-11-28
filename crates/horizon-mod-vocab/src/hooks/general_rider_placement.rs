//! The `general_rider_placement` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Where a rider sits on a general entity row of type `kind` (`mymod:seat`), the
    /// vehicle `vehicle`: asked when `rider` is seated on it, and again when
    /// the rider's box or vehicle attachment point changes (a player
    /// crouching, a mob growing up) or the row's saved placements no longer
    /// name the rider (after a reload). Never asked per tick: the engine keeps
    /// the answer, sends it to the vehicle's trackers with its passengers,
    /// and both sides place the rider by plain math each tick, at the
    /// vehicle's position plus `placement.local` turned by the vehicle's yaw,
    /// plus `placement.world` (`RiderPlacement::seat`). `placement` holds
    /// vanilla's `Entity.positionRider` for a vehicle that overrides nothing:
    /// `local` the passenger attachment `(0, box height, 0)` and `world` the
    /// rider's vehicle attachment point negated; change it to seat the rider
    /// elsewhere. Read the world, don't change it. A part that panics is
    /// skipped and its riders keep the default. Native mods only for now:
    /// a wasm vehicle's riders sit at the default (#792's stage 4b brings
    /// the wasm adapter). Server-side.
    #[hook(
        side = "server",
        contract = "collect",
        out = "placement",
        scope = "entity",
        key = "kind",
        neoforge = "none (vanilla Entity.positionRider, overridden per entity class)"
    )]
    fn general_rider_placement();
}
