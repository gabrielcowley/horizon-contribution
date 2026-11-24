//! A map over a mod's world fluid: the pixel takes the colour the fluid declared (`map_color`),
//! as vanilla's `getCorrectStateForFluidBlock` keeps the fluid's own block, and one that declared
//! none takes water's, as its block is water's.
use super::MapData;
use crate::game::native_entity_refs::tests::{run_fresh_cases, with_game_at};
use horizon_client::chunk::Chunk;
use horizon_client::motion::PlayerMotion;
use horizon_core::geometry::Vec3;
use mod_api::{Hooks, ModEntry, Registrar, WorldFluid};
use std::sync::Arc;

/// `MapColor.TERRACOTTA_YELLOW`'s id.
const TERRACOTTA_YELLOW: u8 = 40;
/// `MapColor.WATER`'s id.
const WATER: u8 = 12;

struct Tinted;
#[mod_api::__subscriptions]
impl Hooks for Tinted {
    fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
        let mut syrup = WorldFluid::default();
        syrup.map_color = Some(TERRACOTTA_YELLOW);
        r.declare_world_fluid("syrup", syrup);
        r.declare_world_fluid("paint", WorldFluid::default());
    }
}

#[test]
#[ignore = "daily"]
fn a_mod_fluid_draws_its_declared_map_colour() {
    if run_fresh_cases(
        "game::maps::fluid_colour_tests::a_mod_fluid_draws_its_declared_map_colour",
        "HORIZON_MAP_FLUID_COLOUR_CHILD",
        &["valid"],
    ) {
        return;
    }
    let mods: mod_api::Mods = vec![ModEntry::new("tinted", Box::new(Tinted))].into();
    mod_platform::__private::install(mods.clone());
    crate::game::api_values::install_parts(&mods).unwrap();
    let base = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let root = crate::game::mods::content_root(&base, &Default::default())
        .unwrap()
        .prepared()
        .to_path_buf();
    with_game_at(root, |game| {
        for cx in -1..=1 {
            for cz in -1..=1 {
                game.world
                    .chunks
                    .insert((cx, cz), Arc::new(Chunk::empty(cx, cz)));
            }
        }
        let registry = game.pack.assets.registry;
        let stone = registry.default_state("minecraft:stone").unwrap();
        // A stone floor; a one-block-deep strip of each fluid across it.
        for x in 0..16 {
            for z in 0..16 {
                game.world.set(x, 63, z, stone).unwrap();
            }
        }
        let strips = [
            ("tinted:syrup", 2),
            ("tinted:paint", 6),
            ("minecraft:water", 10),
        ];
        for (fluid, x) in strips {
            let source = registry.default_state(fluid).unwrap();
            for z in 0..16 {
                game.world.set(x, 64, z, source).unwrap();
            }
        }
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(8.5, 65.0, 8.5)).unwrap());
        let map = MapData::fresh(8.5, 8.5, 0, game.dimension_id());
        let [cx, cz] = map.center;
        game.maps.data.insert(0, map);
        // Each call draws one column in sixteen.
        for _ in 0..16 {
            game.update_map(0);
        }
        let pixel = |x: i32, z: i32| {
            game.maps.data[&0].colors[((z - cz + 64) * 128 + (x - cx + 64)) as usize] >> 2
        };
        assert_eq!(pixel(2, 8), TERRACOTTA_YELLOW, "syrup");
        assert_eq!(pixel(6, 8), WATER, "paint");
        assert_eq!(pixel(10, 8), WATER, "water");
    });
}
