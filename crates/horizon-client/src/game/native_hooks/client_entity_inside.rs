use super::*;

impl Game<'_> {
    /// `Block.entityInside` on the client, for the local player's predicted move: the part
    /// reads through the client host and queues contact actions.
    pub(in crate::game) fn native_client_entity_inside(
        &self,
        call: super::fire::ClientEntityInsideCall<'_>,
    ) {
        self.fire_client_entity_inside(call);
    }
}
