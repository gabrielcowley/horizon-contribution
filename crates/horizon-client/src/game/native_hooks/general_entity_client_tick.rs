use super::*;

impl Game<'_> {
    /// A client tick of this client's copy of one general entity row of type
    /// `kind` (`mymod:crate`) with ID `entity`, in this client's level `level`, after
    /// the copy's step toward the server's pose: the client half of `Entity.tick`. It may move only this copy
    /// (`entity_client_set_position`, `entity_client_set_velocity`,
    /// `entity_client_lerp_to`), and the server's next update overwrites what it
    /// wrote. Client-side.
    pub(in crate::game) fn native_general_entity_client_tick(
        &mut self,
        kind: &str,
        entity: mod_api::EntityRef,
        level: mod_api::LevelRef,
    ) {
        self.fire_general_entity_client_tick(kind, entity, level);
    }
}
