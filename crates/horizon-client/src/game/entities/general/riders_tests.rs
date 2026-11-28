//! `gap/general-entity-sync`, riders: a general row as a vehicle through the game fixture. The
//! fixture's `seats:seat` part places riders as a ground seat does (the passenger point 0.35 up,
//! plus 1/16, less the rider's own attachment) and lets them off half a block above vanilla's
//! default; `seats:plain` has no part, so its riders take vanilla's defaults.
use super::super::Game;
use crate::game::native_entity_refs::tests::with_game;
use horizon_client::mobs::Mob;
use horizon_core::geometry::Vec3;
use horizon_core::nbt::Tag;
use mod_api::{
    EntityPart, EntityRef, GeneralDismountLocationEvent, GeneralEntityCategory,
    GeneralPassengerRemovedEvent, GeneralRiderPlacementEvent, GeneralSavePolicy, Hooks, Registrar,
    ServerHost,
};
use std::sync::{Arc, Mutex};

/// What the seat part heard, in order.
type Log = Arc<Mutex<Vec<String>>>;

/// The seat's centred box, 0.25 wide and 0.35 tall, as the Port's ground seat sets it.
const SEAT_BOX: [[f64; 3]; 2] = [[-0.125, -0.175, -0.125], [0.125, 0.175, 0.125]];

struct Seat {
    log: Log,
}

#[mod_api::horizon_part]
impl EntityPart for Seat {
    fn general_rider_placement(
        &self,
        _: &mut dyn ServerHost,
        event: &mut GeneralRiderPlacementEvent<'_>,
    ) {
        let attachment = event.rider.vehicle_attachment;
        event.placement.local = [0.0, 0.35 + 1.0 / 16.0, 0.0];
        event.placement.world = [-attachment[0], -attachment[1], -attachment[2]];
        self.log
            .lock()
            .unwrap()
            .push(format!("placement {}", event.rider.kind));
    }
    fn general_dismount_location(
        &self,
        _: &mut dyn ServerHost,
        event: &mut GeneralDismountLocationEvent<'_>,
    ) {
        event.location[1] += 0.5;
        self.log.lock().unwrap().push("dismount".to_owned());
    }
    fn general_passenger_removed(
        &self,
        _: &mut dyn ServerHost,
        event: &mut GeneralPassengerRemovedEvent<'_>,
    ) {
        self.log
            .lock()
            .unwrap()
            .push(format!("removed {} {:?}", event.rider.id(), event.reason));
    }
}

struct Declare {
    log: Log,
}

#[mod_api::__subscriptions]
impl Hooks for Declare {
    fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
        for (path, width, height) in [
            ("seat", 0.25, 0.35),
            ("plain", 0.5, 0.5),
            ("crate", 0.5, 0.5),
        ] {
            r.declare_general_entity(
                path,
                GeneralEntityCategory::Misc,
                width,
                height,
                GeneralSavePolicy::Save,
                true,
                5,
                i32::MAX,
                1,
                &[],
            )
            .unwrap();
        }
        r.entity_part(
            "seats:seat",
            Seat {
                log: self.log.clone(),
            },
        );
    }
}

/// A server game with the seat types declared, as `tick_tests`' fixture builds its own.
fn with_seats(test: impl FnOnce(&mut Game<'_>, Log)) {
    let log = Log::default();
    let declare = Declare { log: log.clone() };
    mod_platform::__private::initialize().expect("test registration requires the mod API runtime");
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new("seats", Box::new(declare))].into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            game.native_level_opened(crate::game::Dim::Overworld);
            game.build_registries();
            game.reset_entity_residency();
            // The chunk the tests spawn in, empty: `summon` takes loaded chunks only.
            game.world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
            );
            let _world = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
            test(game, log)
        })
    });
}

fn spawn(
    game: &mut Game<'_>,
    kind: &str,
    at: [f64; 3],
    bounds: Option<[[f64; 3]; 2]>,
) -> EntityRef {
    let dimension = game.dim.id().to_owned();
    game.spawn_general(&dimension, kind, at, [0.0, 0.0], bounds)
        .expect("a declared type spawns")
}

fn add_mob(game: &mut Game<'_>, id: u32, name: &str, at: Vec3) -> EntityRef {
    let kind = game.res.mob_types().get(name).unwrap().clone();
    game.mobs.push(Mob::new(id, kind, at, 0.0));
    game.entity(id).unwrap()
}

fn mob_at(game: &Game<'_>, id: u32) -> Vec3 {
    game.mobs.iter().find(|m| m.id == id).unwrap().position
}

fn row_at(game: &Game<'_>, id: u32) -> Vec3 {
    game.resident_entities
        .general
        .rows
        .iter()
        .find(|row| row.id == id)
        .unwrap()
        .position
}

fn assert_near(at: Vec3, expected: [f64; 3]) {
    let d = [at.x - expected[0], at.y - expected[1], at.z - expected[2]];
    assert!(
        d.iter().all(|d| d.abs() < 1e-9),
        "{at:?} is not {expected:?}"
    );
}

fn taken(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.lock().unwrap())
}

/// A zombie sits where the seat's part says, asked once, and gets off where its part says. A
/// second rider is refused without force, as vanilla's `canAddPassenger` refuses a ridden
/// entity, and the part hears the first one leave.
#[test]
#[ignore = "daily"]
fn a_mob_sits_and_gets_off_where_the_seat_says() {
    with_seats(|game, log| {
        let seat = spawn(game, "seats:seat", [0.5, 200.0, 0.5], Some(SEAT_BOX));
        let zombie = add_mob(game, 900, "minecraft:zombie", Vec3::new(2.5, 200.0, 0.5));
        assert!(game.entity_start_riding(zombie, seat, false));
        assert_eq!(taken(&log), ["placement minecraft:zombie"]);
        game.position_riders();
        // `EntityType.ZOMBIE`'s `ridingOffset(-0.7F)`: its vehicle attachment is the float 0.7 up.
        assert_near(
            mob_at(game, 900),
            [0.5, 200.0 + 0.35 + 0.0625 - f64::from(0.7_f32), 0.5],
        );
        // Seated again on the next tick: nothing changed, so nothing is asked.
        game.reconcile_general_seating();
        assert!(taken(&log).is_empty());

        let other = add_mob(game, 901, "minecraft:zombie", Vec3::new(3.5, 200.0, 0.5));
        assert!(
            !game.entity_start_riding(other, seat, false),
            "one passenger"
        );

        assert!(game.entity_stop_riding(zombie));
        // Vanilla's default, the top of the centred box, then the part's half block.
        assert_near(mob_at(game, 900), [0.5, 200.175 + 0.5, 0.5]);
        assert_eq!(taken(&log), ["dismount", "removed 900 Dismounted"]);
    });
}

/// With no part, a rider sits on the passenger point `(0, box height, 0)` less its attachment
/// and gets off at the vehicle's x and z on top of its box: a generic seat's numbers
/// (`Seat::player_seat`, `Seat::dismount`).
#[test]
#[ignore = "daily"]
fn a_vehicle_without_a_part_uses_vanillas_defaults() {
    with_seats(|game, log| {
        let plain = spawn(game, "seats:plain", [4.5, 200.0, 4.5], None);
        let rider = spawn(game, "seats:crate", [8.5, 210.0, 8.5], None);
        assert!(game.entity_start_riding(rider, plain, false));
        assert_near(row_at(game, rider.id()), [4.5, 200.5, 4.5]);
        let seat = game.seat(plain.id()).unwrap();
        assert_near(
            game.general_rider_seat(plain.id(), 1234, 0.6).unwrap(),
            [
                seat.player_seat().x,
                seat.player_seat().y,
                seat.player_seat().z,
            ],
        );
        assert!(game.entity_stop_riding(rider));
        let off = seat.dismount();
        assert_near(row_at(game, rider.id()), [off.x, off.y, off.z]);
        assert!(taken(&log).is_empty(), "no part, nothing heard");
    });
}

/// A removed vehicle lets its rider off at the rider's own x and z, at the higher of the two
/// heights (`LivingEntity.dismountVehicle`), without asking where; the part hears why. A
/// general rider gets off at the level's removal sweep.
#[test]
#[ignore = "daily"]
fn a_removed_vehicle_lets_its_rider_off_where_it_is() {
    with_seats(|game, log| {
        let seat = spawn(game, "seats:seat", [0.5, 200.0, 0.5], Some(SEAT_BOX));
        let rider = spawn(game, "seats:crate", [6.5, 220.0, 0.5], None);
        assert!(game.entity_start_riding(rider, seat, false));
        assert_eq!(taken(&log), ["placement seats:crate"]);
        assert_near(row_at(game, rider.id()), [0.5, 200.4125, 0.5]);
        // The rider has drifted beside the seat when the seat goes.
        let rows = &mut game.resident_entities.general.rows;
        let index = rows.iter().position(|row| row.id == rider.id()).unwrap();
        rows.update(index, |row| row.position = Vec3::new(1.5, 200.4125, 0.5));
        assert!(game.entity_discard(seat));
        // A general rider gets off at the removal sweep.
        game.native_removals_flush();
        assert_near(row_at(game, rider.id()), [1.5, 200.4125, 0.5]);
        assert_eq!(
            taken(&log),
            [format!("removed {} VehicleRemoved", rider.id())]
        );
        assert_eq!(game.entity_vehicle(rider), None);
    });
}

/// A rider whose box changes is asked about again, and one that goes without getting off is
/// let go with `RiderGone`.
#[test]
#[ignore = "daily"]
fn a_changed_or_gone_rider_is_asked_again_or_let_go() {
    with_seats(|game, log| {
        let seat = spawn(game, "seats:seat", [0.5, 200.0, 0.5], Some(SEAT_BOX));
        let rider = spawn(game, "seats:crate", [6.5, 220.0, 0.5], None);
        assert!(game.entity_start_riding(rider, seat, false));
        taken(&log);
        assert!(game.entity_set_bounds(rider, [-0.25, 0.0, -0.25], [0.25, 1.0, 0.25]));
        game.reconcile_general_seating();
        assert_eq!(taken(&log), ["placement seats:crate"]);
        // Removed with no dismount of its own: the next pass lets it go.
        game.persist
            .removals
            .push(crate::game::commands::Target::General(rider.id()));
        game.reconcile_general_seating();
        assert_eq!(taken(&log), [format!("removed {} RiderGone", rider.id())]);
    });
}

/// A mount onto a row spawned in the same transaction rides once the transaction commits; an
/// aborted one leaves the rider on foot.
#[test]
#[ignore = "daily"]
fn a_mount_onto_a_row_spawned_in_the_same_transaction_follows_its_join() {
    with_seats(|game, _| {
        let zombie = add_mob(game, 900, "minecraft:zombie", Vec3::new(2.5, 200.0, 0.5));
        let seat = mod_api::run_transaction(game, |g, _| {
            let seat = spawn(g, "seats:seat", [0.5, 200.0, 0.5], Some(SEAT_BOX));
            g.entity_start_riding(zombie, seat, false)
                .then_some(seat)
                .ok_or(())
        })
        .expect("the mount is taken inside the transaction");
        assert_eq!(game.entity_vehicle(zombie), Some(seat));

        let other = add_mob(game, 901, "minecraft:zombie", Vec3::new(6.5, 200.0, 0.5));
        let aborted: Result<(), ()> = mod_api::run_transaction(game, |g, _| {
            let seat = spawn(g, "seats:seat", [4.5, 200.0, 0.5], Some(SEAT_BOX));
            assert!(g.entity_start_riding(other, seat, false));
            Err(())
        });
        assert!(aborted.is_err());
        // The aborted row is gone, and the rider's link with it.
        game.sync_riders();
        assert_eq!(game.entity_vehicle(other), None);
    });
}

/// A player's `RootVehicle` takes a general vehicle with its riders, the tree leaves with the
/// player, and it comes back under its UUID.
#[test]
#[ignore = "daily"]
fn a_general_vehicle_round_trips_through_root_vehicle() {
    with_seats(|game, _| {
        let seat = spawn(game, "seats:seat", [0.5, 200.0, 0.5], Some(SEAT_BOX));
        let (tag, root, attach) = game
            .parent_vehicle_tag(Some(seat.id()), &[])
            .expect("a general vehicle saves in RootVehicle");
        assert_eq!(root, seat.id());
        let entity = tag.get("Entity").unwrap();
        assert!(
            matches!(entity.get("id"), Some(Tag::String(id)) if id.to_string_lossy() == "seats:seat")
        );
        game.remove_vehicle_tree(root);
        assert!(!game.is_general_row(seat.id()));
        let mut wrapper = horizon_core::nbt::Compound::new();
        wrapper.insert("RootVehicle".into(), tag);
        let back = game
            .restore_saved_vehicle(attach, Some(&Tag::Compound(wrapper)))
            .expect("the vehicle comes back with the player");
        assert!(game.is_general_row(back));
        assert_eq!(
            game.resident_entities
                .general
                .rows
                .iter()
                .find(|row| row.id == back)
                .unwrap()
                .local_box,
            Some(SEAT_BOX)
        );
    });
}
