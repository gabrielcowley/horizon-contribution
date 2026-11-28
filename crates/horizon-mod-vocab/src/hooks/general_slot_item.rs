//! The `general_slot_item` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The stack in equipment slot `slot` of a general entity row of type
    /// `kind` with a living body, with ID `entity`: vanilla's
    /// `LivingEntity.getItemBySlot` override, read by `living_equipment` and
    /// `/item ... entity`. Answer `Some(stack)` (an empty stack is an empty
    /// slot), or `None` for the next part and then vanilla's empty slot. The
    /// host is read-only. Server-side; told only for general entity types.
    #[hook(
        side = "server",
        contract = "replace",
        out = "item",
        scope = "entity",
        key = "kind",
        host = "read",
        neoforge = "none (vanilla LivingEntity.getItemBySlot override)"
    )]
    fn general_slot_item();
}
