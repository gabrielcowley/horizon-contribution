//! What the rider moments of a general entity vehicle (`general_rider_placement`,
//! `general_dismount_location`, `general_passenger_removed`) are told and answer.

use crate::EntityRef;

/// A rider about to be placed on a general entity vehicle, as
/// `general_rider_placement` is told it: the entity, its type, its box and
/// the point on it that sits on the vehicle (`Entity.getVehicleAttachmentPoint`).
#[derive(Clone, Debug, PartialEq, horizon_mod_api_macros::Wire)]
pub struct RiderInfo {
    /// The rider.
    pub rider: EntityRef,
    /// The rider's entity type (`minecraft:player`, `mymod:crate`).
    pub kind: String,
    /// The width of the rider's box, in blocks.
    pub width: f64,
    /// The height of the rider's box, in blocks.
    pub height: f64,
    /// Where the rider sits on its vehicle, relative to its own position
    /// (`Entity.getVehicleAttachmentPoint`): `(0, 0.6, 0)` for a standing
    /// player, `(0, 0, 0)` for most other entities.
    pub vehicle_attachment: [f64; 3],
}

impl RiderInfo {
    // hidden: the game's own construction, not part of the mod surface
    #[doc(hidden)]
    pub fn new(
        rider: EntityRef,
        kind: String,
        width: f64,
        height: f64,
        vehicle_attachment: [f64; 3],
    ) -> Self {
        Self {
            rider,
            kind,
            width,
            height,
            vehicle_attachment,
        }
    }
}

/// Where a rider sits on its vehicle: `vehicle position + R(yaw) · local +
/// world`, where `R(yaw)` turns `local` by the vehicle's yaw about the
/// vertical axis. The same two-offset shape as a carrier seat's placement.
#[derive(Clone, Copy, Debug, Default, PartialEq, horizon_mod_api_macros::Wire)]
pub struct RiderPlacement {
    /// The offset in the vehicle's own frame, turned with the vehicle's yaw.
    pub local: [f64; 3],
    /// The offset in the world's frame, added after the turn.
    pub world: [f64; 3],
}

impl RiderPlacement {
    /// A placement from its two offsets.
    pub fn new(local: [f64; 3], world: [f64; 3]) -> Self {
        Self { local, world }
    }

    /// Where the rider's position lands for a vehicle at `position`
    /// turned to `yaw` degrees, as vanilla turns a passenger attachment
    /// (`Vec3.yRot(-yaw · π/180)`).
    pub fn seat(&self, position: [f64; 3], yaw: f32) -> [f64; 3] {
        let angle = -f64::from(yaw).to_radians();
        let (sin, cos) = angle.sin_cos();
        let [x, y, z] = self.local;
        let turned = [x * cos + z * sin, y, z * cos - x * sin];
        [
            position[0] + turned[0] + self.world[0],
            position[1] + turned[1] + self.world[1],
            position[2] + turned[2] + self.world[2],
        ]
    }
}

/// Why a rider left a general entity vehicle, as `general_passenger_removed`
/// is told it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, horizon_mod_api_macros::Wire)]
#[non_exhaustive]
pub enum PassengerRemoval {
    /// The rider got off (`Entity.stopRiding`): it sneaked, a mod or a
    /// command took it off, or it boarded something else.
    Dismounted,
    /// The vehicle was removed, and its riders got off with it.
    VehicleRemoved,
    /// The rider itself was removed, died or left the level.
    RiderGone,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placement_turns_its_local_offset_with_the_vehicle_and_adds_the_world_one() {
        let placement = RiderPlacement::new([0.0, 1.0, 0.5], [0.0, -0.6, 0.0]);
        // Facing south (yaw 0) the local +z stays +z, as vanilla's yRot(0).
        let at = placement.seat([10.0, 64.0, -3.0], 0.0);
        assert!((at[0] - 10.0).abs() < 1e-12 && (at[2] - -2.5).abs() < 1e-12);
        assert!((at[1] - 64.4).abs() < 1e-12);
        // Facing west (yaw 90) vanilla's yRot(-π/2) turns +z to -x.
        let at = placement.seat([10.0, 64.0, -3.0], 90.0);
        assert!((at[0] - 9.5).abs() < 1e-12 && (at[2] - -3.0).abs() < 1e-12);
    }
}
