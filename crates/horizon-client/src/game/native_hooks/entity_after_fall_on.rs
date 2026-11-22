use super::*;

impl HitLevel<'_> {
    /// Entity `entity` of type `kind` ended its move on top of `state` at `pos`, after vanilla's
    /// own response to the landing (`Entity.restituteMovementAfterCollisions`). Told mid-move on
    /// the level host a hit or a landing uses: the part reads and draws the level's random, and
    /// queues contact actions the entity's own tick calls back with the server write host.
    pub(in crate::game) fn native_entity_after_fall_on(
        &mut self,
        call: super::fire::EntityAfterFallOnCall<'_>,
    ) {
        self.fire_entity_after_fall_on(call);
    }
}
