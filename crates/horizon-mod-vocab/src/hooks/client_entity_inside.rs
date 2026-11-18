//! The `client_entity_inside` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The client's own player's predicted move crossed a block state this part is attached to (`Block.entityInside`, as the client's `LocalPlayer` runs it). It is the client side of `entity_inside`, for the local player only: other entities and every server-side move use `entity_inside`. The part reads through the client host and writes nothing; `event.queue(action)` queues a [`ContactAction`], and once the predicted tick is done the game calls the part's `contact_action` with the client host and the local player as `event.entity`, if the block is still the state it touched. Use it to carry the local player along (its movement, position and on-ground flag) or to update the client's copy of a block entity; the server stays authoritative. A replayed tick (prediction catching up after a server correction) tells nothing.
    #[hook(
        side = "client",
        contract = "collect",
        out = "actions",
        scope = "block",
        key = "state",
        neoforge = "none (vanilla Block.entityInside on the client; NeoForge adds no event or extension for it)"
    )]
    fn client_entity_inside(
        entity: i32,
        state: BlockState,
        pos: BlockPos,
        actions: &mut Vec<ContactAction>,
    );
}
