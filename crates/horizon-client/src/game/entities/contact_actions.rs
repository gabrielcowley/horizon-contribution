//! Contact actions called back with the write host, in the swept entity's own tick.
//!
//! A part's `entity_inside` and `entity_after_fall_on` queue [`QueuedAction`]s mid-move; each
//! kind's tick hands them here once its planned inside writes are applied and before the next
//! entity moves. One loop serves every kind: a [`ContactSwept`] says how the game reads the
//! swept entity into a [`mod_api::ContactEntity`] and keeps what the part left there.
use super::Game;
use horizon_client::block_contact::QueuedAction;
use horizon_client::entities::ItemEntity;
use horizon_client::projectiles::Projectile;
use horizon_core::geometry::Vec3;

/// The swept entity a contact action is called back for.
pub(in crate::game) trait ContactSwept {
    /// Its record as the part sees it, or None when it is gone (dead, discarded or no
    /// longer in the level): the rest of its actions are dropped.
    fn view(&self, game: &Game<'_>) -> Option<mod_api::ContactEntity>;
    /// Keep what the part left in `entity`. True when the entity is now gone.
    fn keep(&mut self, game: &mut Game<'_>, entity: mod_api::ContactEntity) -> bool;
}

fn finite(v: [f64; 3]) -> Option<Vec3> {
    v.iter()
        .all(|c| c.is_finite())
        .then(|| Vec3::new(v[0], v[1], v[2]))
}

fn record(
    id: i32,
    kind: &str,
    position: Vec3,
    movement: Vec3,
    on_ground: bool,
) -> mod_api::ContactEntity {
    mod_api::ContactEntity {
        id,
        kind: kind.to_owned(),
        position: [position.x, position.y, position.z],
        movement: [movement.x, movement.y, movement.z],
        on_ground,
        ..mod_api::ContactEntity::default()
    }
}

/// A dropped item in its own tick, lifted out of the level's list.
pub(in crate::game) struct SweptItem<'a>(pub(in crate::game) &'a mut ItemEntity);

impl ContactSwept for SweptItem<'_> {
    fn view(&self, game: &Game<'_>) -> Option<mod_api::ContactEntity> {
        use crate::game::api_values::TryIntoApi;
        let item = &self.0;
        if item.health <= 0 || item.stack.count == 0 {
            return None;
        }
        let _ = game;
        let mut entity = record(
            item.id as i32,
            "minecraft:item",
            item.position,
            item.velocity,
            item.on_ground,
        );
        entity.item = Some(item.stack.try_into_api().ok()?);
        Some(entity)
    }
    fn keep(&mut self, game: &mut Game<'_>, entity: mod_api::ContactEntity) -> bool {
        use crate::game::api_values::TryIntoEngine;
        let item = &mut *self.0;
        let before = (item.position, item.velocity);
        if let Some(at) = finite(entity.position) {
            item.position = at;
        }
        if let Some(movement) = finite(entity.movement) {
            item.velocity = movement;
        }
        item.on_ground = entity.on_ground;
        if let Some(factors) = entity.stuck_in_block.and_then(finite) {
            // An item keeps no stuck multiplier: its movement is scaled once, as
            // `EntityWrite::StuckIn` scales it.
            item.velocity = Vec3::new(
                item.velocity.x * factors.x,
                item.velocity.y * factors.y,
                item.velocity.z * factors.z,
            );
            item.fall_distance = 0.0;
        }
        // `hasImpulse`: a part's push goes to clients this tick. The item's own tick has
        // already made its `> 0.1` check, and a client runs no contact action of its own,
        // so without this a conveyor's carry shows only at the 20-tick position update.
        if (item.position, item.velocity) != before {
            item.needs_sync = true;
        }
        match entity.item.map(|stack| stack.try_into_engine(game.items())) {
            Some(Ok(stack)) if stack.count > 0 => item.stack = stack,
            Some(Ok(_)) | None => return true,
            // A stack the game can't hold is refused: the item keeps its own.
            Some(Err(_)) => {}
        }
        entity.discard
    }
}

/// A projectile in its own batch tick, lifted out of the level's list.
pub(in crate::game) struct SweptProjectile<'a>(pub(in crate::game) &'a mut Projectile);

impl ContactSwept for SweptProjectile<'_> {
    fn view(&self, _: &Game<'_>) -> Option<mod_api::ContactEntity> {
        let shot = &self.0;
        if shot.arrow.as_ref().is_some_and(|a| a.removed) {
            return None;
        }
        Some(record(
            shot.id as i32,
            shot.entity_type(),
            shot.position,
            shot.velocity,
            shot.on_ground,
        ))
    }
    fn keep(&mut self, _: &mut Game<'_>, entity: mod_api::ContactEntity) -> bool {
        let shot = &mut *self.0;
        let before = (shot.position, shot.velocity);
        if let Some(at) = finite(entity.position) {
            shot.position = at;
        }
        if let Some(movement) = finite(entity.movement) {
            shot.velocity = movement;
        }
        shot.on_ground = entity.on_ground;
        if let Some(factors) = entity.stuck_in_block.and_then(finite) {
            shot.velocity = Vec3::new(
                shot.velocity.x * factors.x,
                shot.velocity.y * factors.y,
                shot.velocity.z * factors.z,
            );
            shot.fall_distance = 0.0;
        }
        // `hasImpulse`, as for an item: a part's push goes to clients this tick.
        if (shot.position, shot.velocity) != before {
            shot.needs_sync = true;
        }
        if entity.discard
            && let Some(arrow) = shot.arrow.as_mut()
        {
            arrow.removed = true;
            return true;
        }
        false
    }
}

/// A mob between its tick and the next mob's, its row live in the level's list.
pub(in crate::game) struct SweptMob(pub(in crate::game) u32);

impl ContactSwept for SweptMob {
    fn view(&self, game: &Game<'_>) -> Option<mod_api::ContactEntity> {
        let mob = game.mobs.iter().find(|m| m.id == self.0)?;
        if !mob.alive() || mob.removed {
            return None;
        }
        Some(record(
            mob.id as i32,
            &mob.kind.id,
            mob.position,
            mob.velocity,
            mob.on_ground,
        ))
    }
    fn keep(&mut self, game: &mut Game<'_>, entity: mod_api::ContactEntity) -> bool {
        let Some(mob) = game.mobs.iter_mut().find(|m| m.id == self.0) else {
            return true;
        };
        if let Some(at) = finite(entity.position) {
            mob.position = at;
        }
        if let Some(movement) = finite(entity.movement) {
            mob.velocity = movement;
        }
        mob.on_ground = entity.on_ground;
        if let Some(factors) = entity.stuck_in_block.and_then(finite) {
            // `Entity.makeStuckInBlock`: the next move is scaled, and the fall forgotten.
            mob.fall_distance = 0.0;
            mob.stuck_multiplier = factors;
        }
        mob.needs_sync = true;
        if entity.discard {
            if let Some(epoch) = game.native_lifecycle.epoch() {
                game.entity_discard(mod_api::EntityRef::new(epoch, self.0));
            }
            return true;
        }
        false
    }
}

/// The player whose turn it is: the window's own, or a guest swapped in for its turn.
pub(in crate::game) struct SweptPlayer;

impl ContactSwept for SweptPlayer {
    fn view(&self, game: &Game<'_>) -> Option<mod_api::ContactEntity> {
        let player = game.player.as_ref()?;
        if player.health.dead_or_dying() {
            return None;
        }
        Some(record(
            player.entity_id,
            "minecraft:player",
            player.position,
            player.velocity,
            player.ground.on_ground,
        ))
    }
    fn keep(&mut self, game: &mut Game<'_>, entity: mod_api::ContactEntity) -> bool {
        let Some(player) = game.player.as_mut() else {
            return true;
        };
        if let Some(at) = finite(entity.position) {
            player.position = at;
        }
        if let Some(movement) = finite(entity.movement) {
            player.velocity = movement;
        }
        player.ground.on_ground = entity.on_ground;
        if let Some(factors) = entity.stuck_in_block.and_then(finite)
            && !player.flying
        {
            player.fall_distance = 0.0;
            player.stuck_multiplier = factors;
        }
        // A player is never discarded: `discard` is ignored for it.
        false
    }
}

impl Game<'_> {
    /// Call back the contact actions a move queued for `swept`, in order, with the server
    /// write host. An action is skipped when its block is no longer the state it touched,
    /// and the rest stop once the entity is gone. Each call runs in a transaction: a native
    /// panic or a wasm trap rolls back what it wrote and leaves the entity as it was (the
    /// dispatch has already switched the hook off for the session for the part's whole mod).
    /// Whether the entity is gone afterwards: taken whole or discarded.
    pub(in crate::game) fn run_contact_actions(
        &mut self,
        swept: &mut dyn ContactSwept,
        actions: Vec<QueuedAction>,
    ) -> bool {
        if actions.is_empty() {
            return false;
        }
        // A server calls back in its level's scope; the client, for its own player, in the
        // level it shows.
        let client = !self.is_logical_server();
        let dimension = if client {
            self.dim.id()
        } else {
            match horizon_client::block_contact::current_level() {
                Some(dimension) => dimension,
                None => return false,
            }
        };
        for queued in actions {
            let Some(mut entity) = swept.view(self) else {
                return true;
            };
            let pos = queued.position;
            if self.world.block(pos.x, pos.y, pos.z) != queued.state {
                continue;
            }
            let failures = self.wasm_dispatch_failures.get();
            // `contain`, not a bare `catch_unwind`: the panic hook runs before any catch, and
            // only a panic inside `contain` is logged instead of ending the server.
            let outcome = horizon_client::crash::contain(|| {
                let mut call = |game: &mut Game<'_>| {
                    game.native_contact_action(crate::game::native_hooks::ContactActionCall {
                        dimension,
                        moment: queued.moment,
                        state: queued.state,
                        pos,
                        action: &queued.action,
                        entity: &mut entity,
                    });
                    (game.wasm_dispatch_failures.get() == failures)
                        .then_some(())
                        .ok_or(())
                };
                // The client makes no authoritative writes: nothing there to roll back.
                if client {
                    call(self)
                } else {
                    mod_api::run_transaction(self, |game, _| {
                        game.keep_mod_blocks();
                        call(game)
                    })
                }
            });
            match outcome {
                Ok(Ok(())) => {
                    if swept.keep(self, entity) {
                        return true;
                    }
                }
                Ok(Err(())) => log::error!(
                    "a contact action at {pos:?}: its writes were rolled back after a wasm dispatch failure"
                ),
                Err(panic) => {
                    log::error!(
                        "a contact action at {pos:?}: its writes were rolled back after a native mod panicked: {panic}"
                    );
                }
            }
        }
        false
    }

    /// The contact actions a dropped item's move queued, in its own tick.
    pub(in crate::game) fn run_item_contact_actions(
        &mut self,
        item: &mut ItemEntity,
        actions: Vec<QueuedAction>,
    ) -> bool {
        self.run_contact_actions(&mut SweptItem(item), actions)
    }

    /// The contact actions the current player's moves queued, in its own turn.
    pub(in crate::game) fn run_player_contact_actions(&mut self, actions: Vec<QueuedAction>) {
        self.run_contact_actions(&mut SweptPlayer, actions);
    }

    /// The contact actions mob `id`'s tick queued, between its tick and the next mob's, with
    /// its row live in the level's list.
    pub(in crate::game) fn run_mob_contact_actions(&mut self, id: u32) {
        let Some(actions) = self
            .mobs
            .iter_mut()
            .find(|m| m.id == id)
            .map(|m| std::mem::take(&mut m.contact_actions))
        else {
            return;
        };
        self.run_contact_actions(&mut SweptMob(id), actions);
    }

    /// The client's own predicted move's contacts: each one's client contact hook is asked
    /// for contact actions, in the order the move made them, and the actions are called back
    /// with the client host and the local player.
    pub(in crate::game) fn run_client_contacts(&mut self) {
        let Some((entity, contacts)) = self.player.as_mut().map(|player| {
            (
                player.entity_id,
                std::mem::take(&mut player.client_contacts),
            )
        }) else {
            return;
        };
        if contacts.is_empty() || self.is_logical_server() {
            return;
        }
        let mut queued = Vec::new();
        for (moment, state, pos) in contacts {
            let mut actions = Vec::new();
            let asked = horizon_client::crash::contain(|| match moment {
                mod_api::ContactMoment::Inside => {
                    self.native_client_entity_inside(
                        crate::game::native_hooks::ClientEntityInsideCall {
                            entity,
                            state,
                            pos,
                            actions: &mut actions,
                        },
                    );
                }
                mod_api::ContactMoment::AfterFallOn => {
                    self.native_client_entity_after_fall_on(
                        crate::game::native_hooks::ClientEntityAfterFallOnCall {
                            entity,
                            state,
                            pos,
                            actions: &mut actions,
                        },
                    );
                }
            });
            if asked.is_ok() {
                horizon_client::block_contact::queue_actions(
                    &mut queued,
                    moment,
                    state,
                    pos,
                    actions,
                );
            } else {
                log::error!("a client contact hook at {pos:?} panicked; its actions are dropped");
            }
        }
        self.run_contact_actions(&mut SweptPlayer, queued);
    }
}
