use super::*;

impl Game<'_> {
    /// `LivingEntity.getItemBySlot` of general entity row `entity` of type
    /// `kind` with a living body. The host is read-only.
    pub(in crate::game) fn native_general_slot_item(
        &self,
        kind: &str,
        entity: EntityRef,
        slot: mod_api::EquipmentSlot,
        item: &mut Option<mod_api::ItemStack>,
    ) {
        self.fire_general_slot_item(kind, entity, slot, item);
    }
}
