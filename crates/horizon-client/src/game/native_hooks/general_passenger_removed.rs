use super::*;

impl Game<'_> {
    /// `rider` has left the general entity row `vehicle` of type `kind` for
    /// `reason` (`Entity.removePassenger`), once per rider and ride, after it
    /// is off and placed. Server-side.
    pub(in crate::game) fn native_general_passenger_removed(
        &mut self,
        kind: &str,
        vehicle: mod_api::EntityRef,
        rider: mod_api::EntityRef,
        reason: mod_api::PassengerRemoval,
    ) {
        self.fire_general_passenger_removed(kind, vehicle, rider, reason);
    }
}
