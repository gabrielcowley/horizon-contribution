//! The local player in a mod's world fluid that declared entity rules: one it `can_swim` in moves
//! it as water does, tick for tick, and one without rules lets it fall as through air.
use super::native_entity_refs::tests::{run_fresh_cases, with_game_at};
use horizon_client::chunk::Chunk;
use horizon_client::motion::{MotionWorld, PlayerMotion};
use horizon_core::geometry::Vec3;
use mod_api::{FluidImmersion, Hooks, ModEntry, Registrar, WorldFluid};
use std::sync::Arc;

struct Pools;
#[mod_api::__subscriptions]
impl Hooks for Pools {
    fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
        let mut immersion = FluidImmersion::default();
        immersion.can_swim = true;
        immersion.can_push_entity = true;
        immersion.motion_scale = 0.007;
        let mut syrup = WorldFluid::default();
        syrup.immersion = Some(immersion);
        r.declare_world_fluid("syrup", syrup);
        r.declare_world_fluid("paint", WorldFluid::default());
    }
}

/// Each fluid fills a 3 by 3 pool 4 deep on a stone floor; the player starts with its feet half
/// a block under the surface, still, and takes ten ticks with no input. In syrup it sinks as
/// slowly as in water, to the same height; in paint it falls as it would through air.
#[test]
#[ignore = "daily"]
fn a_swimmable_mod_fluid_moves_the_player_as_water_does() {
    if run_fresh_cases(
        "game::fluid_immersion_player_tests::a_swimmable_mod_fluid_moves_the_player_as_water_does",
        "HORIZON_FLUID_IMMERSION_PLAYER_CHILD",
        &["valid"],
    ) {
        return;
    }
    let mods: mod_api::Mods = vec![ModEntry::new("pools", Box::new(Pools))].into();
    mod_platform::__private::install(mods.clone());
    crate::game::api_values::install_parts(&mods).unwrap();
    let base = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let root = super::mods::content_root(&base, &Default::default())
        .unwrap()
        .prepared()
        .to_path_buf();
    with_game_at(root, |game| {
        for cx in -1..=2 {
            for cz in -1..=1 {
                game.world
                    .chunks
                    .insert((cx, cz), Arc::new(Chunk::empty(cx, cz)));
            }
        }
        let registry = game.pack.assets.registry;
        let stone = registry.default_state("minecraft:stone").unwrap();
        let floor_y = 63;
        let pools = [
            ("pools:syrup", 2),
            ("pools:paint", 10),
            ("minecraft:water", 18),
        ];
        for (fluid, x0) in pools {
            let source = registry.default_state(fluid).unwrap();
            for x in x0 - 1..=x0 + 3 {
                for z in 1..=5 {
                    game.world.set(x, floor_y, z, stone).unwrap();
                    for y in floor_y + 1..=floor_y + 4 {
                        let inside = (x0..=x0 + 2).contains(&x) && (2..=4).contains(&z);
                        game.world
                            .set(x, y, z, if inside { source } else { stone })
                            .unwrap();
                    }
                }
            }
        }
        let mut sink = |x0: i32| {
            let start = Vec3::new(f64::from(x0) + 1.5, f64::from(floor_y) + 4.5, 3.5);
            let mut player = PlayerMotion::new(game.motion, start).unwrap();
            let mut world =
                std::mem::replace(&mut game.world, horizon_client::world::World::new(stone, 0));
            {
                let mut w = MotionWorld {
                    entities: &[],
                    player: &mut player,
                    scene: &mut world,
                    assets: &game.pack.assets,
                    collision_binding: game.collision,
                    resources: game.motion,
                    health: &game.health_resources,
                    loop_context: &game.loop_context,
                    damage_hooks: &game.damage_hooks,
                    tag_items: None,
                    block_registries: None,
                };
                for _ in 0..10 {
                    w.simulation_tick([0.0; 2], false).unwrap();
                }
            }
            game.world = world;
            player.position.y
        };
        let (syrup, paint, water) = (sink(2), sink(10), sink(18));
        assert!((syrup - water).abs() < 1e-9, "syrup {syrup}, water {water}");
        assert!(water > f64::from(floor_y) + 3.0, "water {water}");
        // Ten ticks of free fall from rest drop 3.35 blocks; ten in water, about 0.2.
        assert!(paint < water - 3.0, "paint {paint}, water {water}");
    });
}
