//! The `contact_action` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// A [`ContactAction`] queued at a block state this part is attached to, called back inside the swept entity's own tick. On the server (`HostSide::Server`) it gets the write host, for actions `entity_inside` and `entity_after_fall_on` queued. On the client (`HostSide::Client`) it gets the client host and the local player, for actions `client_entity_inside` and `client_entity_after_fall_on` queued; it makes no authoritative writes there, and its writes are not rolled back. On the server: after the entity's planned inside writes are applied, before the next entity moves, in the order the move queued them (`entity_after_fall_on` first, then `entity_inside` in the order the sweep reached the blocks). `event.moment` says which hook queued it, `event.action` is the part's own value (`ContactAction::decode`), and `event.entity` is the swept entity, writable: what the part leaves there (its position, movement, on-ground flag, `stuck_in_block`, a dropped item's stack, `discard`) is what the entity keeps. The host reaches any loaded block entity and its capabilities, spawns and payloads, as `block_entity_tick` does: a depot inserts the item and leaves the leftover in `event.entity.item`, so a second item landing on a full depot the same tick stays whole. The callback's writes are rollback-owned: a native panic or a wasm trap rolls back every write it made and the entity is left as it was, and the part is not called again for the session (a wasm trap stops its mod's group). An action whose block is no longer the state it touched, or whose entity has died or been discarded, is not called. Every part attached to the block state hears every action queued there: ignore one you cannot decode. Where the entity is when it is called: a dropped item or a projectile in its own tick; a mob straight after its tick, its row back in the level's list, before the next mob ticks; a player in its own turn, after the server has checked its moves. A player is never discarded, and only a dropped item has a stack.
    #[hook(
        side = "both",
        contract = "listen",
        scope = "block",
        key = "state",
        isolate = true,
        neoforge = "none (the deferred half of vanilla Block.entityInside, and of the landing in Entity.restituteMovementAfterCollisions)"
    )]
    fn contact_action(
        dimension: &str,
        moment: ContactMoment,
        state: BlockState,
        pos: BlockPos,
        action: &ContactAction,
        entity: &mut ContactEntity,
    );
}
