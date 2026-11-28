use super::*;

impl Game<'_> {
    /// `LivingEntity.setItemSlot` of general entity row `entity` of type
    /// `kind` with a living body: `stack` into `slot` (`None` empties it).
    pub(in crate::game) fn native_general_set_slot_item(
        &mut self,
        kind: &str,
        entity: EntityRef,
        slot: mod_api::EquipmentSlot,
        stack: Option<&mod_api::ItemStack>,
        accepted: &mut Option<bool>,
    ) {
        self.fire_general_set_slot_item(kind, entity, slot, stack, accepted);
    }
}
