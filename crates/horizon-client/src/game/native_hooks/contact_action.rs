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
    /// whole once full. It can also write a block through the host and then panic.
    #[derive(Clone, Default)]
    struct DepotPart {
        held: Arc<Mutex<usize>>,
        called: Arc<Mutex<usize>>,
        panic_after_write: bool,
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
            host: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::ContactActionEvent<'_>,
        ) {
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
}
