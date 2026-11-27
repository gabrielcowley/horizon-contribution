//! Screen scenes' clock and block entities (`native_scene`): a clocked scene poses every
//! block entity on its clock, ticking type or not, and tells it it is in a scene. Daily,
//! with the game's resources.
use mod_api::{Scene, SceneClock, SceneView};

const RECT: [f32; 4] = [10.0, 10.0, 110.0, 90.0];

#[test]
#[ignore = "daily"]
fn every_block_entity_in_a_clocked_scene_is_drawn_on_its_clock_and_knows_it_is_in_a_scene() {
    use crate::game::execution_role::ExecutionRole;
    use crate::game::native_entity_refs::tests::{run_fresh_cases, with_game_role};
    use horizon_client::hud::HudBatch;
    use mod_api::{
        BlockEntityPart, BlockEntitySubmitEvent, BlockPart, BlocksEvent, ClientHost,
        GuiLayerPostEvent, Hooks, Identifier, Registrar, RegistrationContext, SceneBlockEntity,
    };
    use std::sync::{Arc, Mutex};

    const BLOCK: &str = "scene_clock:casing";
    const KIND: &str = "scene_clock:gauge";
    // (game time, partial, animation time, in a scene), as the type's renderer read them.
    static DRAWN: Mutex<Option<(i64, f32, f64, bool)>> = Mutex::new(None);

    // No `block_entity_scene_tick`: nothing ticks this type's copies.
    struct Gauge;
    #[mod_api::horizon_part]
    impl BlockPart for Gauge {}
    #[mod_api::horizon_part]
    impl BlockEntityPart for Gauge {
        fn block_entity_submit(&self, _: &dyn ClientHost, event: &mut BlockEntitySubmitEvent<'_>) {
            let draw = &*event.draw;
            *DRAWN.lock().unwrap() = Some((
                draw.game_time(),
                draw.partial(),
                draw.animation_time(),
                draw.in_scene(),
            ));
        }
    }

    struct Machine {
        scene: Arc<Mutex<Option<Scene>>>,
    }
    #[mod_api::__subscriptions]
    impl Hooks for Machine {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            let kind = r.block_entity_type("gauge").blocks(&[BLOCK]).build();
            r.block_entity_part(&kind, Gauge).unwrap();
        }
        fn blocks(&self, _: &RegistrationContext, event: &mut BlocksEvent<'_>) {
            event
                .blocks
                .push(mod_api::Block::new(BLOCK, "minecraft:stone"));
        }
        fn gui_layer_post(&self, _: &dyn ClientHost, event: &mut GuiLayerPostEvent<'_>) {
            if event.layer == "scene_clock" {
                event
                    .draw
                    .draw_scene(
                        RECT,
                        self.scene.lock().unwrap().as_ref().unwrap(),
                        SceneView::new([0.5, 0.5, 0.5], 0.0, 0.0, 20.0),
                    )
                    .unwrap();
            }
        }
    }

    if run_fresh_cases(
        "game::native_scene_clock_tests::every_block_entity_in_a_clocked_scene_is_drawn_on_its_clock_and_knows_it_is_in_a_scene",
        "HORIZON_SCENE_BLOCK_ENTITY_CLOCK_CHILD",
        &["game"],
    ) {
        return;
    }
    let input = Arc::new(Mutex::new(None));
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "scene_clock",
        Box::new(Machine {
            scene: Arc::clone(&input),
        }),
    )]
    .into();
    mod_platform::__private::install(mods.clone());
    crate::game::api_values::install_parts(&mods).unwrap();
    let base = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let root = crate::game::mods::content_root(&base, &Default::default())
        .unwrap()
        .prepared()
        .to_path_buf();
    with_game_role(root, ExecutionRole::Client, |game| {
        use crate::game::api_values::TryIntoApi as _;

        let state = game.pack.assets.registry.default_state(BLOCK).unwrap();
        game.world.game_time = 500;
        let client = (
            game.world.game_time,
            game.clock.fraction() as f32,
            game.animation_time(),
            true,
        );
        let blocks = vec![mod_api::SceneBlock::new(
            [0, 0, 0],
            state.try_into_api().unwrap(),
            Some(SceneBlockEntity::new(
                Identifier::parse(KIND).unwrap(),
                Default::default(),
            )),
        )];
        let mut frame = |clock: Option<SceneClock>| {
            let mut scene = Scene::new(blocks.clone());
            if let Some(clock) = clock {
                scene = scene.with_clock(clock);
            }
            *input.lock().unwrap() = Some(scene);
            game.frames += 1;
            let mut batch = HudBatch::scratch([320, 240]);
            game.native_gui_layer_post("scene_clock", &mut batch);
            DRAWN.lock().unwrap().take().expect("the gauge was drawn")
        };
        let at = |tick, partial| Some(SceneClock::new(1, tick, partial).animate_blocks(false));
        assert_eq!(frame(None), client, "no clock: the client's");
        assert_eq!(frame(at(7, 0.25)), (7, 0.25, 7.25, true));
        assert_eq!(frame(at(7, 0.25)), (7, 0.25, 7.25, true), "paused");
        assert_eq!(frame(at(40, 0.5)), (40, 0.5, 40.5, true), "scrubbed on");
        assert_eq!(frame(at(3, 0.0)), (3, 0.0, 3.0, true), "scrubbed back");
        // An animating scene (the blocks' animate ticks) keeps the same time.
        let living = Some(SceneClock::new(2, 12, 0.75));
        assert_eq!(frame(living), (12, 0.75, 12.75, true));
        assert_eq!(game.world.game_time, 500);
    });
}
