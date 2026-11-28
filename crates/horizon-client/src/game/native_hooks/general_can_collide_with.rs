use super::*;

impl Game<'_> {
    /// `Entity.canCollideWith` of general entity row `entity` of type `kind`
    /// with a living body, for `other`, whose box meets its move. The host is
    /// read-only.
    pub(in crate::game) fn native_general_can_collide_with(
        &self,
        kind: &str,
        entity: EntityRef,
        other: EntityRef,
        collides: &mut Option<bool>,
    ) {
        self.fire_general_can_collide_with(kind, entity, other, collides);
    }
}
