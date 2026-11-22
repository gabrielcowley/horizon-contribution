//! The `client_entity_after_fall_on` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The client's own player's predicted move ended on top of a block state this part is attached to (after the landing response of `Entity.restituteMovementAfterCollisions`, as the client's `LocalPlayer` runs it in `Entity.move`). It is the client side of `entity_after_fall_on`, for the local player only. The part reads through the client host and writes nothing; `event.queue(action)` queues a [`ContactAction`], and once the predicted tick is done the game calls the part's `contact_action` with the client host and `ContactMoment::AfterFallOn`, before the actions the tick's block sweep queued, if the block is still the state the player landed on. A replayed tick tells nothing.
    #[hook(
        side = "client",
        contract = "collect",
        out = "actions",
        scope = "block",
        key = "state",
        neoforge = "none (the landing response of vanilla Entity.restituteMovementAfterCollisions on the client, with no block call; NeoForge adds no event or extension for it)"
    )]
    fn client_entity_after_fall_on(
        entity: i32,
        state: BlockState,
        pos: BlockPos,
        actions: &mut Vec<ContactAction>,
    );
}
