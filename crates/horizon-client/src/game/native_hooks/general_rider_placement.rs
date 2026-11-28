use super::*;

impl Game<'_> {
    /// Where `rider` sits on the general entity row `vehicle` of type `kind`
    /// (`gap/general-entity-sync`, riders): `placement` comes in as vanilla's
    /// `Entity.positionRider` default and each subscribed part of the type may
    /// change it. Asked on seating and when the rider's box or attachment
    /// point changes, never per tick. Server-side.
    pub(in crate::game) fn native_general_rider_placement(
        &mut self,
        kind: &str,
        vehicle: mod_api::EntityRef,
        rider: mod_api::RiderInfo,
        placement: &mut mod_api::RiderPlacement,
    ) {
        self.fire_general_rider_placement(kind, vehicle, rider, placement);
    }
}
