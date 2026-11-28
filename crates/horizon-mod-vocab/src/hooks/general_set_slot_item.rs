//! The `general_set_slot_item` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// A write of `stack` (`None`: empty) into equipment slot `slot` of a general entity row
    /// of type `kind` with a living body, with ID `entity`: vanilla's
    /// `LivingEntity.setItemSlot` override, reached by
    /// `living_set_equipment` and `/item replace entity`. Answer `Some(true)`
    /// once the part has kept the stack, `Some(false)` to refuse it, or `None`
    /// for the next part and then vanilla's refusal (a row has no slots of its
    /// own). Server-side; told only for general entity types.
    #[hook(
        side = "server",
        contract = "replace",
        out = "accepted",
        scope = "entity",
        key = "kind",
        neoforge = "none (vanilla LivingEntity.setItemSlot override)"
    )]
    fn general_set_slot_item();
}
