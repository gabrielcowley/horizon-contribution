use super::*;

impl Game<'_> {
    /// A contact action queued at `state` at `pos`, called back with the server write host
    /// inside the swept entity's own tick. The caller holds the transaction that owns the
    /// callback's writes and writes `entity` back to the swept entity.
    pub(in crate::game) fn native_contact_action(
        &mut self,
        call: super::fire::ContactActionCall<'_>,
    ) {
        self.fire_contact_action(call);
    }
}

#[cfg(test)]
mod contact_action_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A one-slot depot on stone: its `entity_inside` queues one action a contact, and
    /// its `contact_action` takes the item whole while its slot is empty and leaves it
    /// whole once full. It can also write a block through the host and then panic, or
    /// carry an item along +x as a conveyor does instead of taking it.
    #[derive(Clone, Default)]
    struct DepotPart {
        held: Arc<Mutex<usize>>,
        called: Arc<Mutex<usize>>,
        panic_after_write: bool,
        carry: bool,
    }
    #[mod_api::horizon_part]
    impl mod_api::BlockPart for DepotPart {
        fn entity_inside(
            &self,
            _: &mut dyn mod_api::LevelRandomHost,
            event: &mut mod_api::EntityInsideEvent<'_>,
        ) {
            event.queue(mod_api::ContactAction::new(&1u8).unwrap());
        }
        fn contact_action(
            &self,
            host: mod_api::HostSide<'_>,
            event: &mut mod_api::ContactActionEvent<'_>,
        ) {
            let mod_api::HostSide::Server(host) = host else {
                return;
            };
            *self.called.lock().unwrap() += 1;
            assert_eq!(event.action.decode::<u8>().unwrap(), 1);
            if self.panic_after_write {
                let level = host.level(event.dimension).unwrap();
                let _ = level.set_block(
                    host,
                    [0, 101, 0],
                    event.state,
                    mod_api::BlockUpdateFlags::CLIENTS,
                );
                panic!("a contact action panics after a write");
            }
            if self.carry {
                event.entity.movement[0] = 0.15;
                return;
            }
            if !event.entity.is_item() {
                // Anything but an item is lifted half a block a tick.
                event.entity.movement[1] = 0.5;
                return;
            }
            let mut held = self.held.lock().unwrap();
            if *held == 0 && event.entity.item.take().is_some() {
                *held += 1;
                event.entity.movement = [0.0; 3];
            }
        }
    }
    struct DepotMod(DepotPart);
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for DepotMod {
        fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
            let stone = r.block_key("minecraft:stone");
            r.block_part(stone, self.0.clone());
        }
    }
    fn install(name: &'static str, part: &DepotPart) -> mod_api::Mods {
        let entry =
            |part: &DepotPart| mod_api::ModEntry::new(name, Box::new(DepotMod(part.clone())));
        crate::game::api_values::install_parts(&[entry(part)]).unwrap();
        vec![entry(part)].into()
    }

    /// The depot block at (0, 100, 0) and an item whose move crossed it.
    fn depot_and_item(game: &mut Game<'_>, id: u32) -> horizon_client::entities::ItemEntity {
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        game.world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
        );
        game.world.set(0, 100, 0, stone).unwrap();
        let mut item = horizon_client::entities::ItemEntity::new(
            id,
            horizon_client::items::ItemStack::new("minecraft:dirt", 3),
            horizon_core::geometry::Vec3::new(0.5, 100.2, 0.5),
            horizon_core::geometry::Vec3::new(0.0, -0.2, 0.0),
            0.0,
        );
        item.previous = horizon_core::geometry::Vec3::new(0.5, 100.4, 0.5);
        item
    }

    /// One item's tick: its sweep queues the depot's action, then the action is called.
    fn tick(game: &mut Game<'_>, item: &mut horizon_client::entities::ItemEntity) -> bool {
        horizon_client::block_contact::in_level(Some("minecraft:overworld"), || {
            let mut actions = Vec::new();
            game.sweep_item_inside(item, &mut actions);
            game.run_item_contact_actions(item, actions)
        })
    }

    /// Two items reach a one-slot depot in the same tick, each in its own tick in turn:
    /// the first is taken whole, and the second, seeing the slot full, stays whole.
    #[test]
    #[ignore = "daily"]
    fn the_second_item_on_a_full_depot_stays_whole() {
        let part = DepotPart::default();
        let mods = install("contact-depot", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = horizon_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let mut first = depot_and_item(game, 7);
                let mut second = depot_and_item(game, 8);
                assert!(tick(game, &mut first), "the first item is taken whole");
                assert!(!tick(game, &mut second), "the second item stays");
                assert_eq!(second.stack.count, 3, "and keeps its whole stack");
                assert!(!second.needs_sync, "an item the part leaves as it was isn't resent");
                assert_eq!(*part.held.lock().unwrap(), 1, "the depot holds one item");
                assert_eq!(*part.called.lock().unwrap(), 2);
            });
        });
    }

    /// The block is replaced between the sweep and the action: the action is not called.
    #[test]
    #[ignore = "daily"]
    fn an_action_on_a_replaced_block_is_not_called() {
        let part = DepotPart::default();
        let mods = install("contact-replaced", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = horizon_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let mut item = depot_and_item(game, 7);
                let dirt = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:dirt")
                    .unwrap();
                let gone =
                    horizon_client::block_contact::in_level(Some("minecraft:overworld"), || {
                        let mut actions = Vec::new();
                        game.sweep_item_inside(&mut item, &mut actions);
                        assert_eq!(actions.len(), 1, "the sweep queued the action");
                        game.world.set(0, 100, 0, dirt).unwrap();
                        game.run_item_contact_actions(&mut item, actions)
                    });
                assert!(!gone);
                assert_eq!(
                    *part.called.lock().unwrap(),
                    0,
                    "the stale action is dropped"
                );
                assert_eq!(item.stack.count, 3);
            });
        });
    }

    /// A part that writes a block and then panics: the write is rolled back, the item
    /// is left as it was, and the part is not called again this session.
    #[test]
    #[ignore = "daily"]
    fn a_panicking_action_rolls_back_and_is_switched_off() {
        let part = DepotPart {
            panic_after_write: true,
            ..DepotPart::default()
        };
        let mods = install("contact-panic", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                // A server's game: a client's would refuse the write, and there would be
                // nothing to roll back.
                game.options.dedicated = Some(Vec::new());
                let _ = horizon_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let air = game.world.block(0, 101, 0);
                let mut item = depot_and_item(game, 7);
                let velocity = item.velocity;
                assert!(!tick(game, &mut item));
                assert_eq!(game.world.block(0, 101, 0), air, "the write is rolled back");
                assert_eq!(item.stack.count, 3, "the item is left as it was");
                assert_eq!(item.velocity, velocity);
                assert!(!tick(game, &mut item));
                assert_eq!(
                    *part.called.lock().unwrap(),
                    1,
                    "the part is switched off after its panic"
                );
            });
        });
    }

    /// A part that carries an item marks it for sync (`hasImpulse`): its new movement goes
    /// to clients this tick, not at the item's next 20-tick update, since a client runs no
    /// contact action of its own and would stop the item with friction meanwhile.
    #[test]
    #[ignore = "daily"]
    fn a_carried_item_goes_to_clients_this_tick() {
        let part = DepotPart {
            carry: true,
            ..DepotPart::default()
        };
        let mods = install("contact-carry", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = horizon_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let mut item = depot_and_item(game, 7);
                assert!(!item.needs_sync);
                assert!(!tick(game, &mut item));
                assert_eq!(item.velocity.x, 0.15, "the part's carry is kept");
                assert!(item.needs_sync, "and goes to clients this tick");
            });
        });
    }

    /// A player's contact action runs in its own turn: the part's movement write is what
    /// the player keeps, and a dead player's actions are dropped.
    #[test]
    #[ignore = "daily"]
    fn a_players_contact_action_moves_it_and_skips_the_dead() {
        let part = DepotPart::default();
        let mods = install("contact-player", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = depot_and_item(game, 7);
                let stone = game.world.block(0, 100, 0);
                if game.player.is_none() {
                    game.player = Some(
                        horizon_client::motion::PlayerMotion::new(
                            game.motion,
                            horizon_core::geometry::Vec3::new(0.5, 101.0, 0.5),
                        )
                        .unwrap(),
                    );
                }
                let queued = || horizon_client::block_contact::QueuedAction {
                    moment: mod_api::ContactMoment::AfterFallOn,
                    state: stone,
                    position: horizon_core::position::BlockPos::new(0, 100, 0),
                    action: mod_api::ContactAction::new(&1u8).unwrap(),
                };
                horizon_client::block_contact::in_level(Some("minecraft:overworld"), || {
                    game.run_player_contact_actions(vec![queued()]);
                });
                assert_eq!(game.player.as_ref().unwrap().velocity.y, 0.5);
                assert_eq!(*part.called.lock().unwrap(), 1);
                game.player.as_mut().unwrap().health.dead = true;
                horizon_client::block_contact::in_level(Some("minecraft:overworld"), || {
                    game.run_player_contact_actions(vec![queued()]);
                });
                assert_eq!(
                    *part.called.lock().unwrap(),
                    1,
                    "a dead player's action is dropped"
                );
            });
        });
    }

    /// A client part: its `client_entity_inside` queues an action, and its `contact_action`
    /// lifts the local player when it runs with the client host.
    #[derive(Clone, Default)]
    struct ClientConveyor(Arc<Mutex<Vec<&'static str>>>);
    #[mod_api::horizon_part]
    impl mod_api::BlockPart for ClientConveyor {
        fn client_entity_inside(
            &self,
            _: &dyn mod_api::ClientHost,
            event: &mut mod_api::ClientEntityInsideEvent<'_>,
        ) {
            self.0.lock().unwrap().push("asked");
            event
                .actions
                .push(mod_api::ContactAction::new(&2u8).unwrap());
        }
        fn contact_action(
            &self,
            host: mod_api::HostSide<'_>,
            event: &mut mod_api::ContactActionEvent<'_>,
        ) {
            if let mod_api::HostSide::Client(_) = host {
                self.0.lock().unwrap().push("called on the client");
                event.entity.movement[1] = 0.5;
            }
        }
    }
    struct ClientConveyorMod(ClientConveyor);
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for ClientConveyorMod {
        fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
            let stone = r.block_key("minecraft:stone");
            r.block_part(stone, self.0.clone());
        }
    }

    /// The client's own move noted a contact with a client part's block: once the tick is
    /// done the part is asked for actions, and the action runs with the client host and
    /// moves the local player.
    #[test]
    #[ignore = "daily"]
    fn the_local_players_client_contact_runs_with_the_client_host() {
        let part = ClientConveyor::default();
        let entry =
            || mod_api::ModEntry::new("contact-client", Box::new(ClientConveyorMod(part.clone())));
        crate::game::api_values::install_parts(&[entry()]).unwrap();
        let mods: mod_api::Mods = vec![entry()].into();
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_client_game(|game| {
                let _ = depot_and_item(game, 7);
                let stone = game.world.block(0, 100, 0);
                let mut player = horizon_client::motion::PlayerMotion::new(
                    game.motion,
                    horizon_core::geometry::Vec3::new(0.5, 100.2, 0.5),
                )
                .unwrap();
                player.client_contacts.push((
                    mod_api::ContactMoment::Inside,
                    stone,
                    horizon_core::position::BlockPos::new(0, 100, 0),
                ));
                game.player = Some(player);
                game.run_client_contacts();
                assert_eq!(
                    *part.0.lock().unwrap(),
                    vec!["asked", "called on the client"]
                );
                let player = game.player.as_ref().unwrap();
                assert_eq!(player.velocity.y, 0.5);
                assert!(
                    player.client_contacts.is_empty(),
                    "the contacts are used up"
                );
            });
        });
    }
}
