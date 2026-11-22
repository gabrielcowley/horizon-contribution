//! The `entity_after_fall_on` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// An entity's move ended on top of a block state this part is attached to, after vanilla's own response to the landing (`Entity.restituteMovementAfterCollisions`, which `Entity.move` runs on a vertical collision; the default stops the fall: the entity's vertical movement is zero). Vanilla 26.3 gives the block no call at this moment: `Block.bounceOn` (the `bounce_on` hook) hears only a bounce. It runs inside the entity's move, on the level random host: it reads blocks and draws the level's random in vanilla's order, and writes nothing. `event.queue(action)` queues a [`ContactAction`], the part's own typed value: inside the swept entity's own tick, after its planned writes, the game calls the part's `contact_action` with the server write host and `ContactMoment::AfterFallOn`, if the block is still the state the entity landed on and the entity is alive, before the actions its block sweep queued after the move. Dropped items, mobs and players are told: an item and a mob where their move's vertical collision lands them, a player on each of its moves the server checks that ends on the ground. That one is Horizon's own moment: vanilla's server runs no landing response for a player's move, only its fall-damage check (`ServerGamePacketListenerImpl.handlePlayerPositionChange`), while the player's client runs `Entity.move`. It supersedes `item_after_vertical_collision`, which tells dropped items alone and gives no write host.
    #[hook(
        side = "server",
        contract = "collect",
        out = "actions",
        scope = "block",
        key = "state",
        host = "level",
        target = "horizon_client::game::native_hooks::entity_after_fall_on::HitLevel<'_>::native_entity_after_fall_on",
        neoforge = "none (the landing response of vanilla Entity.restituteMovementAfterCollisions, with no block call; NeoForge adds no event or extension for it)"
    )]
    fn entity_after_fall_on(
        dimension: &str,
        entity: i32,
        kind: &str,
        state: BlockState,
        pos: BlockPos,
        actions: &mut Vec<ContactAction>,
    );
}
