//! `gap/general-entity-bodies`, the client tick: a general type's part ticks this client's copies
//! of its rows after their step toward the server's pose, moves only the copy it is ticking, and
//! answers the motion the server sends.
use super::Game;
use crate::game::{Audio, Options, Progress, Settings, load_resources};
use horizon_client::chunk::Chunk;
use horizon_protocol::packets::play::{self, Clientbound as C};
use mod_api::{
    ClientHost, EntityPart, EntityRef, GeneralEntityCategory, GeneralEntityClientTickEvent,
    GeneralMotionReceivedEvent, GeneralSavePolicy, Hooks, Registrar,
};
use std::sync::{Arc, Mutex};

const KIND: &str = "smooth:crate";

/// What the part saw and was answered on the one tick it acts on.
#[derive(Debug, Default, PartialEq)]
struct Seen {
    /// The rows ticked, in order, over every tick.
    ticked: Vec<u32>,
    /// `level_collide` of one block down, from half a block above stone.
    collided: Option<[f64; 3]>,
    /// The writes to its own row: position, velocity, ease.
    own: [bool; 3],
    /// The same writes to the other row.
    other: [bool; 3],
    /// The ease it read back after starting it.
    lerp: Option<([f64; 3], u32)>,
}

/// The type's part: on the first tick of the row named by `acts_on`, it predicts a step down
/// with `level_collide`, places, speeds and eases its own row, and tries the same on `other`.
struct Smooth {
    seen: Arc<Mutex<Seen>>,
    acts_on: u32,
    other: u32,
    acted: std::sync::atomic::AtomicBool,
}

#[mod_api::horizon_part]
impl EntityPart for Smooth {
    fn general_entity_client_tick(
        &self,
        host: &mut dyn ClientHost,
        event: &mut GeneralEntityClientTickEvent<'_>,
    ) {
        let entity = event.entity;
        let mut seen = self.seen.lock().unwrap();
        seen.ticked.push(entity.id());
        if entity.id() != self.acts_on
            || self.acted.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        let collided = event.level.collide(host, entity, [0.0, -1.0, 0.0]);
        seen.collided = collided;
        let [x, y, z] = entity.position(host).unwrap();
        let [_, down, _] = collided.unwrap();
        let other = EntityRef::new(entity.epoch(), self.other);
        seen.own = [
            entity.client_set_position(host, [x, y + down, z]),
            entity.client_set_velocity(host, [0.1, 0.0, 0.0]),
            entity.client_lerp_to(host, [4.5, 64.0, 0.5], [90.0, 0.0], 4),
        ];
        seen.other = [
            other.client_set_position(host, [9.0, 9.0, 9.0]),
            other.client_set_velocity(host, [9.0, 9.0, 9.0]),
            other.client_lerp_to(host, [9.0, 9.0, 9.0], [0.0, 0.0], 3),
        ];
        seen.lerp = entity.client_lerp_target(host);
    }
    fn general_motion_received(
        &self,
        _: &dyn ClientHost,
        event: &mut GeneralMotionReceivedEvent<'_>,
    ) -> Option<[f64; 3]> {
        // The Original package's `lerpMotion`: the average of its motion and the one sent.
        let (current, received) = (event.current, event.received);
        Some([0, 1, 2].map(|i| (current[i] + received[i]) * 0.5))
    }
}

struct Declare {
    seen: Arc<Mutex<Seen>>,
}

#[mod_api::__subscriptions]
impl Hooks for Declare {
    fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
        r.declare_general_entity(
            "crate",
            GeneralEntityCategory::Misc,
            0.5,
            0.5,
            GeneralSavePolicy::Save,
            true,
            8,
            3,
            1,
            &[],
        )
        .unwrap();
        r.entity_part(
            KIND,
            Smooth {
                seen: self.seen.clone(),
                acts_on: 40,
                other: 41,
                acted: std::sync::atomic::AtomicBool::new(false),
            },
        );
    }
}

/// Runs `test` with the type declared and its part attached, in a window joined to a server
/// through memory, with stone under 0 63 0.
fn with_client(seen: Arc<Mutex<Seen>>, test: impl FnOnce(&mut Game<'_>)) {
    mod_platform::__private::initialize().expect("test registration requires the mod API runtime");
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
    let mods: mod_api::Mods =
        vec![mod_api::ModEntry::new("smooth", Box::new(Declare { seen }))].into();
    mod_platform::testing::with_mods(mods, || {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let (registry, _) = horizon_world::level::data::load_registry_with_flags(&root).unwrap();
        let ids: std::collections::HashSet<_> = (0..registry.len())
            .map(|i| {
                registry
                    .state(horizon_world::registry::BlockStateId(i as u32))
                    .unwrap()
                    .name
                    .clone()
            })
            .collect();
        crate::game::api_values::register_game_keys(ids);
        crate::game::mods::register_iterated_registries(&root).unwrap();
        let settings = Settings::default();
        let loaded = load_resources(
            horizon_assets::resource_source::ResourceRoot::directory(root.clone()),
            Vec::new(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            crate::game::execution_role::ExecutionRole::IntegratedServer,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&horizon_assets::resource_source::ResourceSource::directory(
                root.to_path_buf(),
            )),
            None,
        )
        .unwrap();
        let (pipe, _server) = horizon_protocol::connection::Pipe::pair();
        game.connect_local(pipe, "Steve").unwrap();
        game.native_lifecycle
            .begin_open(mod_api::Authority::RemoteClient);
        game.remote.as_mut().unwrap().logged_in = true;
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        game.world.set(0, 63, 0, stone).unwrap();
        let _world = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
        test(&mut game)
    });
}

/// The server's add packet for row `id` of the test type at `position`, moving at `velocity`.
fn add(game: &mut Game<'_>, id: i32, position: [f64; 3], velocity: [f64; 3]) {
    let kind = game
        .remote
        .as_ref()
        .unwrap()
        .builtin
        .get("minecraft:entity_type")
        .unwrap()
        .id(KIND)
        .expect("the type has a network id");
    game.handle_packet(C::AddEntity(play::AddEntity {
        id,
        uuid: id as u128,
        kind,
        position,
        velocity,
        pitch: 0,
        yaw: 0,
        head_yaw: 0,
        data: 0,
    }))
    .unwrap();
}

/// The part ticks every copy each client tick. On its row's first tick, `level_collide` stops a
/// one-block drop at the stone half a block down, and the part places, speeds and eases that row
/// and reads the ease back; the same writes to the other row, and any write outside the tick,
/// are refused and change nothing. The ease it started then moves the row a quarter of the way
/// on the next tick.
#[test]
#[ignore = "daily"]
fn a_client_tick_moves_only_the_row_it_ticks() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    with_client(seen.clone(), |game| {
        add(game, 40, [0.5, 64.5, 0.5], [0.0; 3]);
        add(game, 41, [8.5, 64.5, 0.5], [0.0; 3]);
        game.tick_remote_entities(true);
        {
            let seen = seen.lock().unwrap();
            assert_eq!(seen.ticked, vec![40, 41]);
            assert_eq!(seen.collided, Some([0.0, -0.5, 0.0]));
            assert_eq!(seen.own, [true; 3]);
            assert_eq!(seen.other, [false; 3]);
            assert_eq!(seen.lerp, Some(([4.5, 64.0, 0.5], 4)));
        }
        let row = |game: &Game<'_>, id: u32| {
            let rows = &game.resident_entities.general.rows;
            rows.iter().find(|row| row.id == id).cloned().unwrap()
        };
        let own = row(game, 40);
        assert_eq!(
            [own.position.y, own.velocity.x],
            [64.0, 0.1],
            "placed on the stone and sped up"
        );
        let other = row(game, 41);
        assert_eq!(
            [other.position.x, other.position.y, other.velocity.x],
            [8.5, 64.5, 0.0]
        );

        let entity = game.entity(40).unwrap();
        let host: &mut dyn ClientHost = game;
        assert!(
            !entity.client_set_position(host, [9.0, 9.0, 9.0]),
            "outside the tick"
        );
        assert!(!entity.client_lerp_to(host, [9.0, 9.0, 9.0], [0.0, 0.0], 3));

        game.tick_remote_entities(true);
        assert_eq!(seen.lock().unwrap().ticked, vec![40, 41, 40, 41]);
        let x = row(game, 40).position.x;
        assert!((x - 1.5).abs() < 1e-9, "a quarter of the way to 4.5: {x}");
        let host: &dyn ClientHost = game;
        assert_eq!(entity.client_lerp_target(host), Some(([4.5, 64.0, 0.5], 3)));
    });
}

/// A velocity the server sends a copy, with the add packet or after it, keeps the part's answer:
/// here the average of the copy's own and the sent one, the Original package's `lerpMotion`.
#[test]
#[ignore = "daily"]
fn a_sent_motion_keeps_the_parts_answer() {
    with_client(Arc::default(), |game| {
        add(game, 40, [0.5, 70.0, 0.5], [0.2, 0.0, 0.0]);
        let velocity = |game: &Game<'_>| {
            let rows = &game.resident_entities.general.rows;
            rows.iter().find(|row| row.id == 40).unwrap().velocity
        };
        // The add packet's motion meets the fresh copy's zero (`recreateFromPacket`).
        assert!(
            (velocity(game).x - 0.1).abs() < 1e-9,
            "{:?}",
            velocity(game)
        );
        game.handle_packet(C::SetEntityMotion(play::SetEntityMotion {
            id: 40,
            velocity: [0.4, 0.0, -0.2],
        }))
        .unwrap();
        let velocity = velocity(game);
        assert!((velocity.x - 0.25).abs() < 1e-9, "{velocity:?}");
        assert!((velocity.z + 0.1).abs() < 1e-9, "{velocity:?}");
    });
}
