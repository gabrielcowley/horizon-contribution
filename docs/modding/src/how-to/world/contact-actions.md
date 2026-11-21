# Taking entities that touch a block

A machine that takes the items dropped on it, or carries the entities that stand on it, needs to write its own state when an entity's move touches it: a depot inserting the item that landed, and the item keeping the leftover. Vanilla does this in `Block.entityInside` and `Block.updateEntityAfterFallOn`, in the middle of the entity's move. Here those moments only read and queue; the write comes a moment later, in a **contact action**.

1. In `entity_inside` (the entity's block sweep crossed the block) or `entity_after_fall_on` (its move ended on top of the block), queue a `ContactAction` with `event.queue(...)`. Its bytes are your own typed value, encoded with `Wire`.
2. Inside the swept entity's own tick, once its planned inside writes are applied and before the next entity moves, the game calls your part's `contact_action` with the server write host: any loaded block entity, its capabilities, spawns and payloads, as in `block_entity_tick`. `event.entity` is the swept entity, writable: its position, movement, on-ground flag, `stuck_in_block`, a dropped item's stack and `discard`.

```rust,ignore
#[derive(Clone, Copy, mod_api::Wire)]
enum DepotAction {
    Take,
}

struct Depot;

impl mod_api::BlockPart for Depot {
    fn entity_after_fall_on(&self, _: &mut dyn mod_api::LevelRandomHost, event: &mut mod_api::EntityAfterFallOnEvent<'_>) {
        if event.kind == "minecraft:item" {
            event.queue(mod_api::ContactAction::new(&DepotAction::Take).unwrap());
        }
    }
    fn contact_action(&self, host: mod_api::HostSide<'_>, event: &mut mod_api::ContactActionEvent<'_>) {
        let mod_api::HostSide::Server(host) = host else { return };
        let Ok(DepotAction::Take) = event.action.decode() else { return };
        let Some(stack) = event.entity.item.take() else { return };
        // Insert into the depot's block entity at `event.pos` through `host`, then leave the
        // leftover (or `None` when all of it went in) where the item can keep it.
        event.entity.item = insert_into_depot(host, event.pos, stack);
    }
}
```

The insert is real, with no simulate step, and it sees the state of the tick so far: a second item landing on a full depot in the same tick finds it full and stays whole.

- **Order.** Actions run in the order the move queued them: `entity_after_fall_on` first, then `entity_inside` in the order the sweep reached the blocks.
- **Stale actions.** An action whose block is no longer the state it touched, or whose entity has died or been discarded, is not called.
- **Failures.** The callback runs in a transaction. A native panic or a wasm trap rolls back every write it made, leaves the entity as it was, and switches `contact_action` off for the session for every part of that mod, not only the one that failed (a wasm trap stops its mod's group). A mod with several parts loses all their contact actions at once: keep a part that may fail in a mod of its own.
- **Sharing a block.** Every part attached to the block state hears every action queued there: ignore one you cannot decode.
- **Which entities.** Players, mobs, dropped items and projectiles, on the server. A dropped item or a projectile is called back in its own tick; a mob straight after its tick, with its row back in the level's list, before the next mob ticks; a player in its own turn, after the server has checked its moves. A player is never discarded, and only a dropped item has a stack.

## On the client

Other entities need nothing on the client: when your action changes a mob's, a dropped item's or a projectile's position or movement, the server sends the change to clients that tick (as vanilla does for a pushed entity), so a carried item glides instead of jumping at its next position update.

A belt that carries the player must move the local player in the client's own prediction too, or the server's corrections jerk it along. The client side has its own two hooks, for the local player only: `client_entity_inside` and `client_entity_after_fall_on`. They read through the client host and queue actions as their server twins do. Once the client's predicted tick is done, the game asks them for each contact the move made, in order, and calls `contact_action` with `HostSide::Client` and the local player as `event.entity`. There the part may move the player, write the client's copy of a block entity, or send a payload to the server. It makes no authoritative writes, so nothing is rolled back; a native panic still switches the mod's `contact_action` off. A replayed tick (prediction catching up after a server correction) tells nothing.

See the hook notes in [`contact_action.rs`](https://github.com/horizon-rust/horizon/blob/main/crates/horizon-mod-vocab/src/hooks/contact_action.rs) and [`entity_after_fall_on.rs`](https://github.com/horizon-rust/horizon/blob/main/crates/horizon-mod-vocab/src/hooks/entity_after_fall_on.rs).
