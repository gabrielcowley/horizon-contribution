//! `gap/general-entity-bodies` (b): a general type with a `Hanging` body survives and breaks
//! as a painting does. Two fixture types hang on a wall at z = 11, facing north: `hang:board`
//! keeps vanilla's survival rule, and `hang:fussy` has a part that answers
//! `general_hanging_survives` with no support needed. A mod listener keeps any hanging entity
//! while its lever is set.
use super::super::super::Game;
use super::super::super::commands::Target;
use crate::game::native_entity_refs::tests::with_game;
use mod_api::{
    EntityPart, Flow, GeneralBody, GeneralEntityCategory, GeneralHangingSurvivesEvent,
    GeneralSavePolicy, HangingBreakEvent, HangingSpec, Hooks, Registrar, ServerHost,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const BOARD: &str = "hang:board";
const FUSSY: &str = "hang:fussy";

/// The part of `hang:fussy`: survives wherever nothing blocks it and nothing overlaps it.
struct Fussy;

#[mod_api::horizon_part]
impl EntityPart for Fussy {
    fn general_hanging_survives(
        &self,
        _: &mut dyn ServerHost,
        event: &mut GeneralHangingSurvivesEvent<'_>,
    ) -> Option<bool> {
        Some(event.check.unobstructed && event.check.alone)
    }
}

struct Declare {
    keep: Arc<AtomicBool>,
}

#[mod_api::__subscriptions]
impl Hooks for Declare {
    fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
        for path in ["board", "fussy"] {
            r.declare_general_entity(
                path,
                GeneralEntityCategory::Misc,
                1.0,
                1.0,
                GeneralSavePolicy::Save,
                true,
                10,
                i32::MAX,
                1,
                &[],
            )
            .unwrap();
            r.general_entity_body(
                path,
                GeneralBody::Hanging(HangingSpec {
                    item: "minecraft:stick".to_owned(),
                    break_sound: "minecraft:entity.painting.break".to_owned(),
                }),
            )
            .unwrap();
        }
        r.entity_part(FUSSY, Fussy);
    }

    fn hanging_break(&self, _: &mut dyn ServerHost, _: &mut HangingBreakEvent<'_>) -> Flow {
        if self.keep.load(Ordering::Relaxed) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
}

/// Runs `test` in a server game with the fixture types declared; the lever keeps every
/// hanging entity a break would take.
fn with_hanging(test: impl FnOnce(&mut Game<'_>, &AtomicBool)) {
    let keep = Arc::new(AtomicBool::new(false));
    mod_platform::__private::initialize().expect("test registration requires the mod API runtime");
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
    let declare = Declare { keep: keep.clone() };
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new("hang", Box::new(declare))].into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            game.build_registries();
            game.set_allow_commands(true);
            game.world
                .chunks
                .insert((0, 0), Arc::new(horizon_client::chunk::Chunk::empty(0, 0)));
            game.block_ticks
                .add_empty_chunk(horizon_core::position::ChunkPos::new(0, 0))
                .unwrap();
            game.load_ticking_neighbourhood(0, 0);
            assert!(game.run_command("forceload add 0 0"));
            assert!(game.run_command("fill 8 199 11 14 202 11 minecraft:stone"));
            game.options.dedicated = Some(Vec::new());
            game.native_level_opened(crate::game::Dim::Overworld);
            game.reset_entity_residency();
            let _world = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
            test(game, &keep)
        })
    });
}

/// A one-block row of `kind` against the wall, its back at z = 11, facing north.
fn hang(game: &mut Game<'_>, kind: &str, x: f64) -> mod_api::EntityRef {
    let dimension = game.dim.id().to_owned();
    let row = game
        .spawn_general(
            &dimension,
            kind,
            [x, 200.5, 10.96875],
            [0.0, 0.0],
            Some([[-0.5, -0.5, -0.03125], [0.5, 0.5, 0.03125]]),
        )
        .expect("a declared type spawns");
    assert!(game.entity_set_hanging_direction(row, horizon_core::geometry::Direction::North));
    row
}

/// One entity phase over the current level.
fn tick(game: &mut Game<'_>) {
    game.tickets.update(None);
    game.begin_ordered_ticks();
    game.tick_general().unwrap();
}

/// Whether the row is gone, or going at the level's next sweep.
fn gone(game: &Game<'_>, row: mod_api::EntityRef) -> bool {
    game.persist.removals.contains(&Target::General(row.id()))
        || !game
            .resident_entities
            .general
            .rows
            .iter()
            .any(|r| r.id == row.id())
}

fn sticks(game: &Game<'_>) -> usize {
    game.dropped
        .iter()
        .filter(|d| &*d.stack.item == "minecraft:stick")
        .count()
}

/// `BlockAttachedEntity.tick`: a row whose wall goes is discarded within 100 ticks and drops
/// its item; with its wall it stays. A part's answer to `general_hanging_survives` replaces
/// vanilla's: `hang:fussy` stays with no wall behind it.
#[test]
#[ignore = "daily"]
fn a_hanging_row_breaks_within_100_ticks_of_its_wall_going() {
    with_hanging(|game, _| {
        let board = hang(game, BOARD, 9.5);
        let fussy = hang(game, FUSSY, 12.5);
        assert_eq!(game.entity_hanging_survives(board), Some(true));
        for _ in 0..150 {
            tick(game);
        }
        assert!(!gone(game, board), "a row on its wall stays");
        assert!(game.run_command("fill 8 199 11 14 202 11 minecraft:air"));
        assert_eq!(game.entity_hanging_survives(board), Some(false));
        assert_eq!(
            game.entity_hanging_survives(fussy),
            Some(true),
            "the part's rule needs no wall"
        );
        for _ in 0..100 {
            tick(game);
        }
        assert!(gone(game, board), "the board went with its wall");
        assert_eq!(sticks(game), 1, "and dropped its item");
        assert!(!gone(game, fussy), "the part kept its own row up");
    });
}

/// `canCoexist`: a painting and a hanging row facing the same way may not share a box, either
/// way round; facing another way they may.
#[test]
#[ignore = "daily"]
fn a_painting_and_a_hanging_row_keep_out_of_each_others_way() {
    with_hanging(|game, _| {
        assert!(game.run_command(
            "summon minecraft:painting 10.5 200.5 10.5 {block_pos:[I;10,200,10],facing:2b,variant:\"minecraft:kebab\"}"
        ));
        let painting = game.paintings[0].clone();
        assert!(game.painting_survives(&painting), "alone on its wall");
        let row = hang(game, BOARD, 10.5);
        assert_eq!(game.entity_hanging_survives(row), Some(false));
        assert!(!game.painting_survives(&painting), "the row is in its way");
        assert!(game.entity_set_hanging_direction(row, horizon_core::geometry::Direction::Up));
        let index = game.general_row_index(row.id()).unwrap();
        assert!(
            game.general_hanging_check(index).alone,
            "facing up, not in its way"
        );
        assert!(game.painting_survives(&painting));
    });
}

/// `Painting.dropItem`: a creative player's break drops nothing, and with `entityDrops` off
/// nothing drops at all; otherwise the item drops. A hit breaks the row.
#[test]
#[ignore = "daily"]
fn a_hanging_rows_drop_honours_creative_and_entity_drops() {
    with_hanging(|game, _| {
        let hit = |game: &mut Game<'_>, row: mod_api::EntityRef| {
            game.hurt_target(
                Target::General(row.id()),
                1.0,
                "minecraft:player_attack",
                Some(Target::Player),
                Some(Target::Player),
                None,
            )
        };
        game.creative = true;
        let first = hang(game, BOARD, 9.5);
        assert!(hit(game, first));
        assert!(gone(game, first));
        assert_eq!(sticks(game), 0, "a creative break drops nothing");
        game.creative = false;
        assert!(game.run_command("gamerule entity_drops false"));
        let second = hang(game, BOARD, 11.5);
        assert!(hit(game, second));
        assert!(gone(game, second));
        assert_eq!(sticks(game), 0, "no drops without entityDrops");
        assert!(game.run_command("gamerule entity_drops true"));
        let third = hang(game, BOARD, 13.5);
        assert!(hit(game, third));
        assert!(gone(game, third));
        assert_eq!(sticks(game), 1);
    });
}

/// `HangingEntity.push` and `move`: `entity_push` and a shulker's lid break a hanging row
/// rather than move it; `hanging_break` can keep it, unmoved.
#[test]
#[ignore = "daily"]
fn a_push_or_a_lid_breaks_a_hanging_row_unless_a_mod_keeps_it() {
    with_hanging(|game, keep| {
        let pushed = hang(game, BOARD, 9.5);
        keep.store(true, Ordering::Relaxed);
        assert!(game.entity_push(pushed, [0.0, 0.0, -0.5]));
        assert!(!gone(game, pushed), "the mod kept it");
        let index = game.general_row_index(pushed.id()).unwrap();
        assert_eq!(
            game.resident_entities.general.rows[index].position.z, 10.96875,
            "a kept row does not move"
        );
        keep.store(false, Ordering::Relaxed);
        assert!(game.entity_push(pushed, [0.0, 0.0, -0.5]));
        assert!(gone(game, pushed));
        assert_eq!(sticks(game), 1);

        let lidded = hang(game, BOARD, 12.5);
        assert!(game.run_command("summon minecraft:shulker 24 200 24 {NoAI:1b}"));
        let shulker = game
            .mobs
            .iter()
            .find(|m| m.species() == horizon_client::mobs::Species::Shulker)
            .unwrap()
            .id;
        game.handle_mob_events(vec![horizon_client::mobs::MobEvent::LidPush {
            shulker,
            area: horizon_core::geometry::Aabb::new(
                horizon_core::geometry::Vec3::new(12.0, 200.0, 10.0),
                horizon_core::geometry::Vec3::new(13.0, 201.0, 11.0),
            ),
            delta: [0.0, 0.0, -0.25],
        }])
        .unwrap();
        assert!(gone(game, lidded), "the lid broke it");
        assert_eq!(sticks(game), 2);
    });
}
