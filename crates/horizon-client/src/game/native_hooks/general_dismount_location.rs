use super::*;

impl Game<'_> {
    /// Where `rider` gets off the general entity row `vehicle` of type `kind`
    /// (`Entity.getDismountLocationForPassenger`): `location` comes in as
    /// vanilla's default, the vehicle's x and z at the top of its box, and
    /// each subscribed part of the type may change it. Asked once per
    /// dismount from a vehicle still in the level. Server-side.
    pub(in crate::game) fn native_general_dismount_location(
        &mut self,
        kind: &str,
        vehicle: mod_api::EntityRef,
        rider: mod_api::EntityRef,
        location: &mut [f64; 3],
    ) {
        self.fire_general_dismount_location(kind, vehicle, rider, location);
    }
}
