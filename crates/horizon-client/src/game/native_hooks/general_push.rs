use super::*;

impl Game<'_> {
    /// `Entity.push(Entity)` of general entity row `entity` of type `kind`
    /// with a living body, by `by`, whose box meets it.
    pub(in crate::game) fn native_general_push(
        &mut self,
        kind: &str,
        entity: EntityRef,
        by: EntityRef,
        pushed: &mut Option<bool>,
    ) {
        self.fire_general_push(kind, entity, by, pushed);
    }
}
