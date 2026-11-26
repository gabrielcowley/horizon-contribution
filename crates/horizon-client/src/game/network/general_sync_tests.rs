//! `gap/general-entity-sync`: a mod's general entity rows tracked to clients on vanilla's entity
//! packets, with their synced attachments as the spawn snapshot, and a client's copies made,
//! moved, boxed and dropped by those packets.
use super::Game;
use crate::game::{Audio, Options, Progress, Settings, load_resources};
use horizon_client::lan::Out;
use horizon_core::nbt::{Compound, Tag};
use horizon_protocol::packets::play::{self, Clientbound as C};
use mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, GeneralEntityCategory, GeneralSavePolicy,
    Holder, Hooks, Registrar,
};

/// The test type: tracked 8 chunks away, its pose sent only at spawn and on a teleport, its
/// velocity changes never.
const KIND: &str = "sync:crate";

/// A synced counter, as a mod's renderer would read it.
static COUNT: AttachmentType<i32> = AttachmentType {
    namespace: "sync",
    name: "count",
    default: || 0,
    codec: Some(AttachmentCodec {
        encode: |n| Ok([("n".into(), Tag::Int(*n))].into()),
        decode: |tag: &Compound| match tag.get(&"n".into()) {
            Some(Tag::Int(n)) => Ok(*n),
            _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: false,
    sync: true,
};

struct Declare;

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
            i32::MAX,
            1,
            &[],
        )
        .unwrap();
        r.general_entity_sends_velocity("crate", false).unwrap();
    }
}

/// Runs `test` with the test type declared. The registration phase lasts until the test has
/// built its game, which registers the mods again ([`world_phase`] ends it).
fn with_declared(test: impl FnOnce()) {
    mod_platform::__private::initialize().expect("test registration requires the mod API runtime");
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new("sync", Box::new(Declare))].into();
    mod_platform::testing::with_mods(mods, || {
        // As the game fixture does: the parts registration installs the declarations, then the
        // registry order is published.
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
        test()
    });
}

/// The world phase a server's tick runs in, once the game is built.
fn world_phase() -> impl Sized {
    mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World)
}

/// The bodies a guest was sent, decoded, in order.
fn sent(rx: &std::sync::mpsc::Receiver<Out>) -> Vec<C> {
    rx.try_iter()
        .flat_map(|out| match out {
            Out::Packet(body) => vec![body],
            Out::Packets(bodies) => bodies,
            Out::Disconnect(_) => panic!("unexpected disconnect"),
        })
        .filter_map(|body| {
            let mut r = horizon_protocol::buf::Reader::new(&body);
            let id = r.varint().ok()?;
            C::decode(id, r.rest()).ok()
        })
        .collect()
}

/// Whether `packets` move or place the entity `id`.
fn moves(packets: &[C], id: u32) -> bool {
    packets.iter().any(|p| match p {
        C::MoveEntityPos(p) => p.0.id == id as i32,
        C::MoveEntityPosRot(p) => p.0.id == id as i32,
        C::EntityPositionSync(p) => p.id == id as i32,
        C::TeleportEntity(p) => p.id == id as i32,
        _ => false,
    })
}

/// A late tracker gets the row on vanilla's add packet, under the type's network id after
/// vanilla's, and starts tracking it, so its synced attachments (its box among them) go to it
/// whole in that flush. The type's `i32::MAX` interval sends no move after the spawn and its
/// `general_entity_sends_velocity(false)` no velocity, but a teleport is sent; the box saves with
/// the row and comes back with it.
#[test]
#[ignore = "daily"]
fn a_tracker_gets_a_general_row_its_snapshot_and_only_its_teleports() {
    with_declared(|| {
        let mut resources = None;
        let mut game = crate::game::game_tests::tests::server_game(&mut resources);
        game.build_registries();
        let _world = world_phase();
        game.options.dedicated = Some(Vec::new());
        // Clear of the guest's own entity id (`guest_at` gives it 1).
        game.next_entity = game.next_entity.max(100);
        let dimension = game.dim.id().to_owned();
        let row = game
            .spawn_general(
                &dimension,
                KIND,
                [8.5, 80.0, 8.5],
                [90.0, 0.0],
                Some([[-0.5, 0.0, -0.5], [0.5, 2.0, 0.5]]),
            )
            .expect("a declared type spawns");
        COUNT.set(&mut game, Holder::Entity(row.id()), 7).unwrap();
        assert_eq!(
            game.entity_bounding_box(row),
            Some([[8.0, 80.0, 8.0], [9.0, 82.0, 9.0]]),
            "the spawn's box is the row's"
        );
        assert!(
            !game.entity_set_bounds(row, [0.0; 3], [65.0, 1.0, 1.0]),
            "a side past 64 blocks is refused"
        );

        let rx = crate::game::lan::mob_swing_sync_tests::guest_at(&mut game, [8.5, 80.0, 8.5]);
        // The guest has its chunk, as its client would once it is sent.
        game.lan.as_mut().unwrap().hub.guests.update(0, |guest| {
            guest.sent_chunks.insert((0, 0));
            guest.watched.insert((0, 0));
            guest.loaded = true;
        });
        // The guest's chunks go first; the row is paired once its chunk is the guest's.
        let mut first = Vec::new();
        for _ in 0..5 {
            game.tick_lan(false);
            first.extend(sent(&rx));
        }
        let builtin = horizon_client::lan::data::with_custom_stats(
            horizon_protocol::registry::Builtin::load(
                &horizon_protocol::registry::Builtin::find(&game.options.resources).unwrap(),
            )
            .unwrap(),
        );
        let types = builtin.get("minecraft:entity_type").unwrap();
        let kind = types.id(KIND).expect("the type has a network id");
        let pig = types.id("minecraft:pig").unwrap();
        assert!(kind > pig, "after vanilla's types");
        let added = first
            .iter()
            .find_map(|p| match p {
                C::AddEntity(p) if p.id == row.id() as i32 => Some(p),
                _ => None,
            })
            .expect("the row is added");
        assert_eq!(added.kind, kind);
        assert_eq!(added.position, [8.5, 80.0, 8.5]);
        let guest = &game.lan.as_ref().unwrap().hub.guests[0];
        assert!(guest.tracker.is_tracking(row.id() as i32));
        let snapshot = game.attachment_snapshot(Holder::Entity(row.id()));
        let names: Vec<String> = snapshot
            .iter()
            .flat_map(|m| &m.changes)
            .map(|c| {
                format!(
                    "{}:{}",
                    c.namespace.to_string_lossy(),
                    c.name.to_string_lossy()
                )
            })
            .collect();
        assert!(names.contains(&"sync:count".to_owned()), "{names:?}");
        assert!(
            names.contains(&"horizon:general_box".to_owned()),
            "{names:?}"
        );

        // No update interval: a move the row makes by itself is not sent.
        let rows = &mut game.resident_entities.general.rows;
        rows.update(0, |r| r.position.x += 1.0);
        for _ in 0..4 {
            game.tick_lan(false);
        }
        assert!(!moves(&sent(&rx), row.id()), "no move after the spawn");
        // No velocity updates: a change of velocity is not sent either.
        assert!(game.entity_set_velocity(row, [0.5, 0.0, 0.0]));
        game.tick_lan(false);
        assert!(
            !sent(&rx)
                .iter()
                .any(|p| matches!(p, C::SetEntityMotion(m) if m.id == row.id() as i32)),
            "a type that sends no velocity updates sends none"
        );
        // A teleport is.
        assert!(game.entity_teleport_to(row, [12.5, 80.0, 8.5], None));
        game.tick_lan(false);
        assert!(moves(&sent(&rx), row.id()), "the teleport is sent");

        // The box saves with the row and comes back with it.
        let keys = std::iter::once((0, 0)).collect();
        let (_, tag) = <crate::game::general::GeneralRow as crate::game::entity::Entity>::save_nbt(
            &mut game, 0, &keys,
        )
        .expect("the row saves");
        // `save_lists!` writes a root's own attachments beside its record.
        let mut tag = tag;
        crate::game::persist::write_attachments(&mut tag, row.id(), &game.entity_attachments);
        game.resident_entities.general.rows.clear();
        if let Tag::Compound(map) = &mut tag {
            map.insert("UUID".into(), Tag::IntArray(vec![9, 9, 9, 9]));
        }
        game.load_general_tree(tag, None).unwrap();
        assert_eq!(
            game.resident_entities.general.rows[0].local_box,
            Some([[-0.5, 0.0, -0.5], [0.5, 2.0, 0.5]])
        );
    });
}

/// A window joined to a server through memory.
fn with_client(test: impl FnOnce(&mut Game<'_>)) {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
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
    // What the login packet does: the level the mods' handles name.
    game.native_lifecycle
        .begin_open(mod_api::Authority::RemoteClient);
    game.remote.as_mut().unwrap().logged_in = true;
    let _world = world_phase();
    test(&mut game)
}

/// A client makes its copy of a general row from the add packet, takes its box from the synced
/// attachment (so `level_entities_in` finds it by that box), eases it to a sent position over
/// vanilla's three steps, and drops it with the remove packet.
#[test]
#[ignore = "daily"]
fn a_client_row_follows_the_packets_and_its_synced_box() {
    with_declared(|| {
        with_client(|game| {
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
                id: 40,
                uuid: 40,
                kind,
                position: [0.5, 64.0, 0.5],
                velocity: [0.0; 3],
                pitch: 0,
                yaw: 0,
                head_yaw: 0,
                data: 0,
            }))
            .unwrap();
            let rows = &game.resident_entities.general.rows;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].id, 40);
            let row = game.entity(40).expect("both-side reads see the row");
            assert_eq!(
                game.entity_bounding_box(row),
                Some([[0.25, 64.0, 0.25], [0.75, 64.5, 0.75]]),
                "the declared box until the server sends one"
            );

            // The box arrives as a synced attachment.
            let mut value = Compound::new();
            for (key, corner) in [("Min", [-2.0, 0.0, -2.0]), ("Max", [2.0, 1.0, 2.0])] {
                value.insert(
                    key.into(),
                    Tag::List(corner.map(|v: f64| Tag::Double(v.to_bits())).to_vec()),
                );
            }
            let message = horizon_protocol::attachment_sync::Message {
                holder: Holder::Entity(40),
                replace: true,
                changes: vec![horizon_mod_runtime::attachments::AttachmentSync {
                    namespace: "horizon".into(),
                    name: "general_box".into(),
                    value: Some(value),
                }],
            };
            game.remote_attachment_sync(
                &horizon_protocol::attachment_sync::encode(&message).unwrap(),
            );
            let level = game.native_client_level().unwrap();
            let found = game
                .level_entities_in(level, [2.2, 64.0, 2.2], [2.4, 64.2, 2.4])
                .unwrap();
            assert!(
                found.iter().any(|e| e.id() == 40),
                "the synced box reaches past the declared one: {found:?}"
            );

            // Vanilla's lerpTo: a third of the way each tick, there on the third.
            game.handle_packet(C::EntityPositionSync(play::EntityPositionSync {
                id: 40,
                path: play::PositionPath::Linear([3.5, 64.0, 0.5]),
                yaw: 0.0,
                pitch: 0.0,
                on_ground: false,
            }))
            .unwrap();
            for _ in 0..3 {
                game.tick_remote_entities(true);
            }
            let x = game.resident_entities.general.rows[0].position.x;
            assert!((x - 3.5).abs() < 1e-9, "at {x}");

            game.handle_packet(C::RemoveEntities(play::RemoveEntities { ids: vec![40] }))
                .unwrap();
            assert!(game.resident_entities.general.rows.is_empty());
        });
    });
}
