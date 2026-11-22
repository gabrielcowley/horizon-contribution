use super::*;

impl Game<'_> {
    /// The landing (after `Entity.restituteMovementAfterCollisions`) on the client, for the
    /// local player's predicted move:
    /// the part reads through the client host and queues contact actions.
    pub(in crate::game) fn native_client_entity_after_fall_on(
        &self,
        call: super::fire::ClientEntityAfterFallOnCall<'_>,
    ) {
        self.fire_client_entity_after_fall_on(call);
    }
}
