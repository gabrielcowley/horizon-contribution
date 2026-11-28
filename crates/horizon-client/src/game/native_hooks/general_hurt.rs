use super::*;

impl Game<'_> {
    /// A hit on general entity row `call.entity` of type `call.kind` with a
    /// living body, before vanilla's `LivingEntity.hurtServer` runs on it: the
    /// parts of the type may handle it (`Some(taken)`) or leave it to vanilla
    /// (`None`).
    pub(in crate::game) fn native_general_hurt(
        &mut self,
        call: mod_api::__private::GeneralHurtCall<'_>,
    ) {
        self.fire_general_hurt(call);
    }
}
