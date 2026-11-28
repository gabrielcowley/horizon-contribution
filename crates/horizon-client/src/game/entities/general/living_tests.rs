//! A general entity type with a living body (`GeneralBody::Living`): the armor stand's
//! movement, vanilla's living damage, and the type-scoped moments that stand for the
//! Original's overrides. The fixture type has the stand's box, so its fall can be read
//! against a stand's tick by tick.
use super::*;
use crate::game::native_entity_refs::tests::with_game;
use horizon_client::chunk::Chunk;
use horizon_client::entities::ArmorStand;
use horizon_core::nbt::{Compound, Tag};
use mod_api::{
    EntityPart, GeneralBody, GeneralCanCollideWithEvent, GeneralEntityCategory, GeneralHurtEvent,
    GeneralInsideBlockEvent, GeneralPushEvent, GeneralSavePolicy, Hooks, Host, LivingSpec,
    Registrar, ServerHost,
};
use std::sync::{Arc, Mutex};

/// What the fixture part heard and how it answers, shared with the test.
#[derive(Default)]
struct Script {
    /// Each hit's damage type, in order.
    hurts: Vec<String>,
    /// Handle every hit (`Some(true)`) and discard the row, as the Original's package
    /// pops; otherwise leave it to vanilla.
    pop: bool,
    /// Each `onInsideBlock`: the block's position and `at_end`.
    inside: Vec<([i32; 3], bool)>,
    /// `canCollideWith`'s answer for another row; `None` leaves vanilla's.
    collides: Option<bool>,
    /// `push`'s answer; `None` leaves vanilla's.
    pushed: Option<bool>,
}

type Shared = Arc<Mutex<Script>>;

struct Crate(Shared);

#[mod_api::horizon_part]
impl EntityPart for Crate {
    fn general_hurt(
        &self,
        host: &mut dyn ServerHost,
        event: &mut GeneralHurtEvent<'_>,
    ) -> Option<bool> {
        let mut script = self.0.lock().unwrap();
        script.hurts.push(event.damage_type.to_owned());
        if script.pop {
            assert!(event.entity.discard(host));
            return Some(true);
        }
        None
    }
    fn general_inside_block(
        &self,
        _: &mut dyn ServerHost,
        event: &mut GeneralInsideBlockEvent<'_>,
    ) {
        let pos = [event.pos.x, event.pos.y, event.pos.z];
        self.0.lock().unwrap().inside.push((pos, event.at_end));
    }
    fn general_can_collide_with(
        &self,
        _: &dyn Host,
        _: &mut GeneralCanCollideWithEvent<'_>,
    ) -> Option<bool> {
        self.0.lock().unwrap().collides
    }
    fn general_push(&self, _: &mut dyn ServerHost, _: &mut GeneralPushEvent<'_>) -> Option<bool> {
        self.0.lock().unwrap().pushed
    }
}

/// `mymod:crate`: the stand's box, a living body, 5 health from the stand's
/// attribute set.
struct Declare(Shared);

#[mod_api::__subscriptions]
impl Hooks for Declare {
    fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
        r.declare_general_entity(
            "crate",
            GeneralEntityCategory::Misc,
            0.5,
            f64::from(1.975_f32),
            GeneralSavePolicy::Save,
            true,
            10,
            3,
            1,
            &[],
        )
        .unwrap();
        r.general_entity_body("crate", GeneralBody::Living(LivingSpec::default()))
            .unwrap();
        r.declare_entity_attributes(
            "crate",
            "minecraft:armor_stand",
            vec![("minecraft:max_health".into(), 5.0)],
        )
        .unwrap();
        r.entity_part("mymod:crate", Crate(self.0.clone()));
    }
}

fn with_crates(test: impl FnOnce(&mut Game<'_>, Shared)) {
    let shared = Shared::default();
    mod_platform::__private::initialize().expect("test registration requires the mod API runtime");
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "mymod",
        Box::new(Declare(shared.clone())),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            game.native_level_opened(crate::game::Dim::Overworld);
            game.build_registries();
            game.reset_entity_residency();
            game.world
                .chunks
                .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
            let stone = game.pack.assets.registry.default_state("minecraft:stone");
            for x in 0..16 {
                for z in 0..16 {
                    game.world.set(x, 63, z, stone.unwrap()).unwrap();
                }
            }
            let _world = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
            test(game, shared)
        })
    });
}

fn place(game: &mut Game<'_>, at: [i32; 3], name: &str) {
    let state = game.pack.assets.registry.default_state(name).unwrap();
    game.world.set(at[0], at[1], at[2], state).unwrap();
}

/// A crate at `at`, joined and past the tick it joined in.
fn spawn(game: &mut Game<'_>, at: [f64; 3]) -> u32 {
    let entity = game
        .spawn_general("minecraft:overworld", "mymod:crate", at, [0.0, 0.0], None)
        .expect("the crate spawns");
    game.tickets.update(None);
    game.begin_ordered_ticks();
    entity.id()
}

fn tick(game: &mut Game<'_>) {
    game.tickets.update(None);
    game.begin_ordered_ticks();
    game.tick_general().unwrap();
}

fn row<'g>(game: &'g Game<'_>, id: u32) -> Option<&'g GeneralRow> {
    game.resident_entities
        .general
        .rows
        .iter()
        .find(|row| row.id == id && !game.persist.removals.contains(&Target::General(id)))
}

fn living<'g>(game: &'g Game<'_>, id: u32) -> &'g living::GeneralLiving {
    row(game, id).unwrap().living.as_deref().unwrap()
}

/// The crate falls as an armor stand with the same box does, tick by tick, and comes to
/// rest on the floor with vanilla's resting speed, `(0 - 0.08) * 0.98F`.
#[test]
#[ignore = "daily"]
fn a_living_crate_falls_as_a_stand_and_rests_at_its_speed() {
    with_crates(|game, _| {
        let id = spawn(game, [0.5, 70.0, 0.5]);
        game.armor_stands
            .push(ArmorStand::new(9_000, Vec3::new(4.5, 70.0, 4.5), 0.0));
        assert_eq!(living(game, id).health, 5.0, "the attribute set's health");
        for tick_no in 0..40 {
            tick(game);
            game.tick_armor_stands().unwrap();
            let stand = &game.armor_stands[0];
            let crate_row = row(game, id).unwrap();
            assert_eq!(
                (crate_row.position.y, crate_row.velocity.y),
                (stand.position.y, stand.velocity.y),
                "tick {tick_no}"
            );
        }
        let crate_row = row(game, id).unwrap();
        assert_eq!(crate_row.position.y, 64.0);
        assert!(living(game, id).on_ground);
        assert_eq!(crate_row.velocity.y, -0.08 * f64::from(0.98_f32));
    });
}

/// A fire block lights a crate for eight seconds and hurts it through its type's
/// `general_hurt`; a crate made fire immune is neither lit nor hurt.
#[test]
#[ignore = "daily"]
fn fire_lights_and_hurts_a_crate_unless_it_is_fire_immune() {
    with_crates(|game, script| {
        place(game, [2, 64, 2], "minecraft:fire");
        let id = spawn(game, [2.5, 64.0, 2.5]);
        tick(game);
        assert_eq!(living(game, id).fire_ticks, 160);
        assert_eq!(script.lock().unwrap().hurts, ["minecraft:in_fire"]);
        // The vanilla hurt took 1 and started the cooldown.
        assert_eq!(living(game, id).health, 4.0);

        script.lock().unwrap().hurts.clear();
        place(game, [6, 64, 6], "minecraft:fire");
        let immune = spawn(game, [6.5, 64.0, 6.5]);
        let epoch = game.native_lifecycle.epoch().unwrap();
        assert!(game.entity_set_fire_immune(mod_api::EntityRef::new(epoch, immune), true));
        tick(game);
        assert_eq!(living(game, immune).fire_ticks, 0);
        assert_eq!(living(game, immune).health, 5.0);
        assert!(
            !script.lock().unwrap().hurts.is_empty(),
            "the burning crate is still hurt"
        );
        // The immune crate's own fire immunity saves with it.
        let mut tag = Tag::Compound(Compound::new());
        living(game, immune).write(&mut tag, 0.0);
        let loaded = living::GeneralLiving::from_tag(&tag, 5.0);
        assert_eq!(loaded.fire_immune, Some(true));
    });
}

/// Under water a crate's air runs down by one a tick from 300; at -20, the 320th tick, it
/// drowns, and its air starts again from 0.
#[test]
#[ignore = "daily"]
fn a_crate_under_water_drowns_on_its_320th_tick() {
    with_crates(|game, script| {
        for y in 64..70 {
            for x in 0..4 {
                for z in 0..4 {
                    place(game, [x, y, z], "minecraft:water");
                }
            }
        }
        let id = spawn(game, [1.5, 64.0, 1.5]);
        for _ in 0..319 {
            tick(game);
        }
        assert_eq!(living(game, id).air, -19);
        assert!(script.lock().unwrap().hurts.is_empty());
        tick(game);
        assert_eq!(script.lock().unwrap().hurts, ["minecraft:drown"]);
        assert_eq!(living(game, id).air, 0);
    });
}

/// `/kill` and the void reach the type's `general_hurt`, which pops the crate.
#[test]
#[ignore = "daily"]
fn kill_and_the_void_reach_the_crates_own_hurt() {
    with_crates(|game, script| {
        script.lock().unwrap().pop = true;
        let killed = spawn(game, [3.5, 64.0, 3.5]);
        assert!(game.kill_general_living(killed));
        assert!(row(game, killed).is_none(), "the part popped it");

        let min_y = f64::from(game.world.layout().min_y());
        let fallen = spawn(game, [5.5, min_y - 65.0, 5.5]);
        tick(game);
        assert!(row(game, fallen).is_none());
        assert_eq!(
            script.lock().unwrap().hurts,
            ["minecraft:generic_kill", "minecraft:out_of_world"]
        );
    });
}

/// The push `Entity.push(Entity)` gives a pair 0.3 apart on x: `sqrt(0.3)`, scaled by
/// 0.05.
fn push_at_0_3() -> f64 {
    0.3_f64.sqrt() * f64::from(0.05_f32)
}

fn summon_still_zombie(game: &mut Game<'_>, at: [f64; 3]) -> u32 {
    let mut no_ai = Compound::new();
    no_ai.insert("NoAI".into(), Tag::Byte(1));
    game.summon("minecraft:overworld", "minecraft:zombie", at, no_ai)
        .unwrap()
}

fn mob_velocity_x(game: &Game<'_>, id: u32) -> f64 {
    game.mobs.iter().find(|m| m.id == id).unwrap().velocity.x
}

/// A crate and a zombie push each other apart on both their turns, as vanilla's
/// `pushEntities` runs for each: the zombie's `doPush(crate)` asks the crate's
/// `general_push`, the crate's own `doPush(zombie)` is the zombie's vanilla `push`. So a
/// crate whose `general_push` says no is still pushed on its own turn, once.
#[test]
#[ignore = "daily"]
fn a_crate_and_a_zombie_push_each_other_on_both_turns() {
    with_crates(|game, script| {
        let zombie = summon_still_zombie(game, [8.8, 64.0, 8.5]);
        let id = spawn(game, [8.5, 64.0, 8.5]);
        tick(game);
        let pushed = mob_velocity_x(game, zombie);
        assert!(
            pushed > 1.5 * push_at_0_3(),
            "the zombie is pushed east twice: {pushed}"
        );
        assert!(
            row(game, id).unwrap().velocity.x < -push_at_0_3(),
            "the crate west, twice"
        );

        script.lock().unwrap().pushed = Some(false);
        let kept = spawn(game, [12.5, 64.0, 12.5]);
        let other = summon_still_zombie(game, [12.8, 64.0, 12.5]);
        tick(game);
        let once = push_at_0_3();
        let zombie_x = mob_velocity_x(game, other);
        assert!(
            (zombie_x - once).abs() < 1e-9,
            "the zombie once: {zombie_x}"
        );
        let crate_x = row(game, kept).unwrap().velocity.x;
        assert!((crate_x + once).abs() < 1e-9, "the crate once: {crate_x}");
    });
}

/// One player of a server, standing at `at` in the overworld.
fn join_guest(game: &mut Game<'_>, at: [f64; 3]) -> i32 {
    use horizon_client::lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig};
    use horizon_protocol::packets::configuration::ClientInformation;
    use std::sync::atomic::{AtomicI32, AtomicUsize};
    game.lan_host(
        crate::game::lan::LanSettings {
            port: 0,
            guest_commands: false,
            force_game_mode: false,
        },
        NetConfig {
            listen: false,
            announce: false,
            lod_distance: 0,
            ..NetConfig::lan(0, String::new(), 2)
        },
        HubConfig {
            default_game_mode: 0,
            guest_commands: false,
            force_game_mode: false,
            hardcore: false,
            view_distance: 2,
            simulation_distance: 2,
            dimension: "minecraft:overworld".into(),
            allow_flight: true,
            seed: 0,
            max_players: 2,
            integrated: true,
            levels: game.level_shapes,
            level_seeds: game.levels.runtime.seeds(),
        },
    )
    .unwrap();
    let (tx, _rx) = std::sync::mpsc::channel();
    let me = game.take_entity_id() as i32;
    let mut guest = Guest::new(
        Joined {
            conn: 12,
            uuid: 12,
            name: "guest12".into(),
            properties: Vec::new(),
            info: ClientInformation::new(8),
            brand: String::new(),
            sender: PlayerSender::new(tx, Arc::new(AtomicUsize::new(0))),
            latency: Arc::new(AtomicI32::new(0)),
            address: "127.0.0.1:1".parse().unwrap(),
            expected: false,
        },
        me,
        at,
        2,
    );
    guest.placed = true;
    guest.loaded = true;
    guest.dimension = "minecraft:overworld".into();
    game.lan.as_mut().unwrap().hub.guests.push(guest);
    me
}

/// A server's player pushes a crate on its own turn, and the crate's own turn pushes the
/// pair as well: the crate moves away on both, and on its own turn alone when its
/// `general_push` refuses the player. The player's share moves only the server's copy,
/// which no player feels, so the player stays where it is.
#[test]
#[ignore = "daily"]
fn a_servers_player_and_a_crate_push_each_other() {
    for refuses in [false, true] {
        with_crates(|game, script| {
            join_guest(game, [8.8, 64.0, 8.5]);
            let id = spawn(game, [8.5, 64.0, 8.5]);
            script.lock().unwrap().pushed = refuses.then_some(false);
            tick(game);
            let x = row(game, id).unwrap().velocity.x;
            let once = push_at_0_3();
            if refuses {
                assert!((x + once).abs() < 1e-9, "its own turn only: {x}");
            } else {
                assert!(x < -once, "both turns: {x}");
            }
            let guest = &game.lan.as_ref().unwrap().hub.guests[0];
            assert_eq!(guest.position, [8.8, 64.0, 8.5], "no player feels it");
        });
    }
}

/// A crate riding another gets the base part of its body tick (its fire burns down) but
/// no travel (`Entity.rideTick`): its velocity is zeroed and gravity does not pull it.
#[test]
#[ignore = "daily"]
fn a_riding_crate_burns_but_does_not_travel() {
    with_crates(|game, _| {
        let vehicle = spawn(game, [8.5, 64.0, 8.5]);
        let rider = spawn(game, [8.5, 66.0, 8.5]);
        let rows = &mut game.resident_entities.general.rows;
        let i = rows.iter().position(|row| row.id == rider).unwrap();
        rows.update(i, |row| {
            row.mount = Some(vehicle);
            row.velocity = Vec3::new(0.3, 0.0, 0.0);
            row.living.as_deref_mut().unwrap().fire_ticks = 100;
        });
        game.resident_entities.general.mount_writes += 1;
        tick(game);
        assert_eq!(living(game, rider).fire_ticks, 99, "its fire burns down");
        let row = row(game, rider).unwrap();
        assert_eq!(row.velocity, Vec3::ZERO, "rideTick zeroes its velocity");
        assert_eq!(row.position.y, 66.0, "and it does not fall");
    });
}

/// A client's copy of a crate burns while the server's on-fire flag is set
/// (`Entity.isOnFire` on a client), and takes the server's health.
#[test]
#[ignore = "daily"]
fn a_clients_crate_burns_while_the_servers_flag_is_set() {
    use horizon_protocol::entity_data::{Value, serializer};
    with_crates(|game, _| {
        assert!(game.spawn_remote_general(
            900,
            "mymod:crate",
            [8.5, 64.0, 8.5],
            [0.0, 0.0],
            [0.0; 3]
        ));
        let row = |game: &Game<'_>| {
            game.resident_entities
                .general
                .rows
                .iter()
                .find(|row| row.id == 900)
                .unwrap()
                .clone()
        };
        assert!(!row(game).displays_fire(), "not before the flag");
        game.remote_general_data(
            900,
            &[
                (0, serializer::BYTE, Value::Byte(1)),
                (9, serializer::FLOAT, Value::Float(3.0)),
            ],
        );
        let burning = row(game);
        assert!(burning.displays_fire(), "the flag lights it");
        assert_eq!(burning.living.as_ref().unwrap().health, 3.0);
        game.remote_general_data(900, &[(0, serializer::BYTE, Value::Byte(0))]);
        assert!(!row(game).displays_fire(), "and its clearing puts it out");
    });
}

/// A crate falling onto another stops on top of it only while its type's
/// `general_can_collide_with` says so; vanilla's living rows pass through each other.
#[test]
#[ignore = "daily"]
fn crates_stack_only_when_their_type_collides_with_them() {
    for collides in [Some(true), None] {
        with_crates(|game, script| {
            script.lock().unwrap().collides = collides;
            let below = spawn(game, [1.5, 64.0, 1.5]);
            let above = spawn(game, [1.5, 67.0, 1.5]);
            for _ in 0..40 {
                tick(game);
            }
            let top = row(game, below).unwrap().position.y + f64::from(1.975_f32);
            let y = row(game, above).unwrap().position.y;
            if collides == Some(true) {
                assert_eq!(y, top, "stacked");
            } else {
                assert_eq!(y, 64.0, "fell through to the floor");
            }
        });
    }
}

/// A move through three cells of grass tells `onInsideBlock` each once, and `at_end` only
/// for the cell the box ends in.
#[test]
#[ignore = "daily"]
fn inside_block_tells_at_end_only_for_the_final_box() {
    with_crates(|game, script| {
        for x in 0..3 {
            place(game, [x, 64, 7], "minecraft:short_grass");
        }
        let id = spawn(game, [0.5, 64.0, 7.5]);
        game.resident_entities.general.rows.update(
            game.resident_entities
                .general
                .rows
                .iter()
                .position(|row| row.id == id)
                .unwrap(),
            |row| row.velocity = Vec3::new(2.0, 0.0, 0.0),
        );
        tick(game);
        let heard: Vec<_> = script
            .lock()
            .unwrap()
            .inside
            .iter()
            .copied()
            .filter(|(pos, _)| pos[1] == 64)
            .collect();
        assert_eq!(
            heard,
            [([0, 64, 7], false), ([1, 64, 7], false), ([2, 64, 7], true)]
        );
    });
}
