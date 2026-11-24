//! Fluid interactions in `LiquidBlock.shouldSpreadLiquid`: vanilla's lava rules as the table's
//! first entries, the mods' after them, and the read a mod makes of one neighbour.
use super::{super::tests::World, fixture as tag_fixture};
use crate::{
    fluid::FluidKind,
    fluid_interactions::DeclaredFluidInteraction,
    level::{Behaviours, LevelEvent, tests::Fixture},
    registry::BlockStateId,
};
use horizon_core::{
    position::BlockPos,
    random::{LegacyRandom, RandomSource},
};

const POS: BlockPos = BlockPos { x: 0, y: 1, z: 0 };
/// The neighbours `shouldSpreadLiquid` reads, in its order: above, north, south, west, east.
const NEIGHBOURS: [BlockPos; 5] = [
    BlockPos { x: 0, y: 2, z: 0 },
    BlockPos { x: 0, y: 1, z: -1 },
    BlockPos { x: 0, y: 1, z: 1 },
    BlockPos { x: -1, y: 1, z: 0 },
    BlockPos { x: 1, y: 1, z: 0 },
];
const BELOW: BlockPos = BlockPos { x: 0, y: 0, z: 0 };

fn fixture(declared: &[DeclaredFluidInteraction]) -> Fixture {
    let (registry, mut behaviours) = tag_fixture::load().unwrap();
    let refused = behaviours.resolve_fluid_interactions(&registry, declared);
    assert_eq!(refused, Vec::<String>::new());
    Fixture {
        registry,
        behaviours,
    }
}

fn declared(
    fluid: &str,
    touching_fluid: Option<&str>,
    touching_block: Option<&str>,
    source: &str,
    flowing: &str,
) -> DeclaredFluidInteraction {
    DeclaredFluidInteraction {
        owner: "test".into(),
        fluid: fluid.into(),
        touching_fluid: touching_fluid.map(Into::into),
        touching_block: touching_block.map(Into::into),
        block_below: None,
        source: source.into(),
        flowing: flowing.into(),
    }
}

/// Puts `state` at the converting position, runs `shouldSpreadLiquid` there and gives its answer,
/// the block left there and the 1501 events fired.
fn spread(w: &mut World, state: BlockStateId) -> (bool, BlockStateId, usize) {
    w.put(POS, state);
    w.level.events.clear();
    let b: &Behaviours = w.b();
    let spreads = b
        .sim(&mut w.level, &mut w.ticks)
        .should_spread_liquid(POS, state);
    let fizzes = w
        .level
        .events
        .iter()
        .filter(|e| matches!(e, LevelEvent::LevelEvent { id: 1501, pos, .. } if *pos == POS))
        .count();
    (spreads, w.get(POS), fizzes)
}

#[test]
fn vanilla_lava_meets_water_from_above_and_the_sides_and_never_from_below() {
    let f = fixture(&[]);
    let mut w = World::new(&f);
    let (lava, flowing_lava) = (w.id("lava", &[]), w.id("lava", &[("level", "2")]));
    let (water, flowing_water) = (w.id("water", &[]), w.id("water", &[("level", "3")]));
    let (obsidian, cobblestone) = (w.id("obsidian", &[]), w.id("cobblestone", &[]));
    let air = f.behaviours.air();
    for neighbour in NEIGHBOURS {
        for touching in [water, flowing_water] {
            w.put(neighbour, touching);
            assert_eq!(spread(&mut w, lava), (false, obsidian, 1), "{neighbour:?}");
            assert_eq!(
                spread(&mut w, flowing_lava),
                (false, cobblestone, 1),
                "{neighbour:?}"
            );
            w.put(neighbour, air);
        }
    }
    w.put(BELOW, water);
    assert_eq!(spread(&mut w, lava), (true, lava, 0));
}

#[test]
fn vanilla_basalt_needs_soul_soil_below_and_the_first_direction_wins() {
    let f = fixture(&[]);
    let mut w = World::new(&f);
    let lava = w.id("lava", &[]);
    let (water, blue_ice) = (w.id("water", &[]), w.id("blue_ice", &[]));
    let (basalt, obsidian) = (w.id("basalt", &[]), w.id("obsidian", &[]));
    w.put(NEIGHBOURS[1], blue_ice);
    assert_eq!(spread(&mut w, lava), (true, lava, 0), "no soul soil");
    w.put(BELOW, w.id("soul_soil", &[]));
    // Blue ice to the north is read before water to the east.
    w.put(NEIGHBOURS[4], water);
    assert_eq!(spread(&mut w, lava), (false, basalt, 1));
    // Water above is read before either.
    w.put(NEIGHBOURS[0], water);
    assert_eq!(spread(&mut w, lava), (false, obsidian, 1));
}

#[test]
fn mod_interactions_follow_vanilla_in_declaration_order_for_any_fluid() {
    let f = fixture(&[
        declared(
            "minecraft:flowing_lava",
            None,
            Some("minecraft:dirt"),
            "minecraft:clay",
            "minecraft:mud",
        ),
        declared(
            "minecraft:lava",
            None,
            Some("minecraft:dirt"),
            "minecraft:sponge",
            "minecraft:sponge",
        ),
        declared(
            "minecraft:water",
            Some("minecraft:flowing_lava"),
            None,
            "minecraft:stone",
            "minecraft:netherrack",
        ),
    ]);
    let mut w = World::new(&f);
    let (lava, flowing_lava) = (w.id("lava", &[]), w.id("lava", &[("level", "2")]));
    let (water, flowing_water) = (w.id("water", &[]), w.id("water", &[("level", "3")]));
    let (clay, mud) = (w.id("clay", &[]), w.id("mud", &[]));
    // Dirt to the north: the first lava entry, whatever form the declaration named.
    w.put(NEIGHBOURS[1], w.id("dirt", &[]));
    assert_eq!(spread(&mut w, lava), (false, clay, 1));
    assert_eq!(spread(&mut w, flowing_lava), (false, mud, 1));
    // Water to the south is read later; water above is read first, and vanilla's entry wins.
    w.put(NEIGHBOURS[2], water);
    assert_eq!(spread(&mut w, lava), (false, clay, 1));
    w.put(NEIGHBOURS[0], water);
    assert_eq!(spread(&mut w, lava), (false, w.id("obsidian", &[]), 1));
    // Water converts beside lava in either form.
    let mut w = World::new(&f);
    w.put(NEIGHBOURS[3], lava);
    assert_eq!(spread(&mut w, water), (false, w.id("stone", &[]), 1));
    assert_eq!(
        spread(&mut w, flowing_water),
        (false, w.id("netherrack", &[]), 1)
    );
}

#[test]
fn a_cancelled_placement_leaves_the_lava_and_still_fizzes() {
    let f = fixture(&[]);
    let mut w = World::new(&f);
    let lava = w.id("lava", &[]);
    w.put(NEIGHBOURS[2], w.id("water", &[]));
    w.level.fluid_place_answer = Some(None);
    assert_eq!(spread(&mut w, lava), (false, lava, 1));
    let sponge = w.id("sponge", &[]);
    w.level.fluid_place_answer = Some(Some(sponge));
    assert_eq!(spread(&mut w, lava), (false, sponge, 1));
}

#[test]
fn a_fluid_with_no_interactions_spreads_at_once() {
    let f = fixture(&[]);
    let mut w = World::new(&f);
    let water = w.id("water", &[]);
    assert!(!f.behaviours.has_fluid_interactions(FluidKind::Water));
    w.put(NEIGHBOURS[0], w.id("lava", &[]));
    w.put(NEIGHBOURS[1], w.id("blue_ice", &[]));
    assert_eq!(spread(&mut w, water), (true, water, 0));
}

#[test]
fn declarations_naming_what_does_not_exist_are_refused_by_owner_and_field() {
    let (registry, mut behaviours) = tag_fixture::load().unwrap();
    let ok = declared(
        "minecraft:lava",
        None,
        Some("minecraft:dirt"),
        "minecraft:stone",
        "minecraft:stone",
    );
    let refused = behaviours.resolve_fluid_interactions(
        &registry,
        &[
            DeclaredFluidInteraction {
                fluid: "test:nothing".into(),
                ..ok.clone()
            },
            DeclaredFluidInteraction {
                fluid: "minecraft:empty".into(),
                ..ok.clone()
            },
            DeclaredFluidInteraction {
                touching_block: Some("test:nothing".into()),
                ..ok.clone()
            },
            DeclaredFluidInteraction {
                touching_fluid: Some("minecraft:water".into()),
                ..ok.clone()
            },
            DeclaredFluidInteraction {
                touching_block: None,
                ..ok.clone()
            },
            DeclaredFluidInteraction {
                block_below: Some("test:nothing".into()),
                ..ok.clone()
            },
            DeclaredFluidInteraction {
                source: "minecraft:stone[level=3]".into(),
                ..ok.clone()
            },
            DeclaredFluidInteraction {
                flowing: String::new(),
                ..ok.clone()
            },
            ok.clone(),
        ],
    );
    assert_eq!(
        refused,
        [
            "mod test: fluid interaction of test:nothing: fluid: test:nothing is not a fluid in the world",
            "mod test: fluid interaction of minecraft:empty: fluid: minecraft:empty is not a fluid in the world",
            "mod test: fluid interaction of minecraft:lava: when.touching_block: test:nothing is not a block",
            "mod test: fluid interaction of minecraft:lava: when: names both touching_fluid and touching_block",
            "mod test: fluid interaction of minecraft:lava: when: names no condition",
            "mod test: fluid interaction of minecraft:lava: when.block_below: test:nothing is not a block",
            "mod test: fluid interaction of minecraft:lava: becomes.source: minecraft:stone has no property level",
            "mod test: fluid interaction of minecraft:lava: becomes.flowing: is missing",
        ]
    );
    // The accepted one reads back after vanilla's two.
    let lava: Vec<_> = behaviours
        .fluid_interactions_of(FluidKind::FlowingLava)
        .map(|i| i.declared().clone())
        .collect();
    assert_eq!(lava.len(), 3);
    assert_eq!(lava[0].touching_fluid.as_deref(), Some("#minecraft:water"));
    assert_eq!(
        lava[1].touching_block.as_deref(),
        Some("minecraft:blue_ice")
    );
    assert_eq!(lava[1].block_below.as_deref(), Some("minecraft:soul_soil"));
    assert_eq!(lava[2], ok);
}

/// Over random neighbourhoods, the read a mod makes of each neighbour in turn finds the block
/// `shouldSpreadLiquid` places.
#[test]
fn the_one_neighbour_read_agrees_with_the_spread() {
    let f = fixture(&[declared(
        "minecraft:lava",
        None,
        Some("minecraft:dirt"),
        "minecraft:clay",
        "minecraft:mud",
    )]);
    let b = &f.behaviours;
    let mut w = World::new(&f);
    let palette = [
        b.air(),
        w.id("water", &[]),
        w.id("water", &[("level", "5")]),
        w.id("blue_ice", &[]),
        w.id("dirt", &[]),
        w.id("stone", &[]),
        w.id("lava", &[]),
    ];
    let below = [b.air(), w.id("soul_soil", &[]), w.id("water", &[])];
    let converting = [w.id("lava", &[]), w.id("lava", &[("level", "4")])];
    let mut random = LegacyRandom::new(1501);
    for _ in 0..2_000 {
        let pick =
            |random: &mut LegacyRandom, n: usize| random.next_int_bound(n as i32).unwrap() as usize;
        for neighbour in NEIGHBOURS {
            w.put(neighbour, palette[pick(&mut random, palette.len())]);
        }
        let below_state = below[pick(&mut random, below.len())];
        w.put(BELOW, below_state);
        let state = converting[pick(&mut random, converting.len())];
        let fluid = b.fluid(state);
        let read = NEIGHBOURS.iter().find_map(|&neighbour| {
            b.fluid_interaction(fluid.kind(), fluid.is_source(), w.get(neighbour), || {
                below_state
            })
            .map(|(_, result)| result)
        });
        let (spreads, placed, _) = spread(&mut w, state);
        assert_eq!(spreads, read.is_none());
        assert_eq!(placed, read.unwrap_or(state));
    }
}
