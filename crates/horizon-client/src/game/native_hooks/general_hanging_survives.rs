use super::*;

impl Game<'_> {
    /// `HangingEntity.survives` for the hanging general row `entity` of type `kind`: `check`
    /// holds vanilla's three conditions, and `survives` left `None` keeps vanilla's answer.
    /// Server-side.
    pub(in crate::game) fn native_general_hanging_survives(
        &mut self,
        kind: &str,
        entity: mod_api::EntityRef,
        check: mod_api::HangingCheck,
        survives: &mut Option<bool>,
    ) {
        self.fire_general_hanging_survives(kind, entity, check, survives);
    }
}
