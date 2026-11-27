//! A wasm placement modifier's `modify` crosses the wire: its draws and emitted positions reach
//! the game's context in order, and a call that answers nothing (as a trap leaves it) is an
//! `Err`, never a silent `Ok`, so the game uses none of the positions it pushed.
use super::*;
use mod_api::{
    BlockMetadata, BlockState, FeatureError, HeightmapType, PlacementModifier,
    PlacementModifierContext, StructureRandom,
};

/// Draws `nextInt(4)` and emits two positions that far along +z, at the lowest Y.
#[derive(Debug)]
struct Pair;
impl PlacementModifier for Pair {
    fn modify(
        &self,
        context: &mut dyn PlacementModifierContext,
        origin: [i32; 3],
    ) -> Result<(), FeatureError> {
        let dz = context.random().next_int_bound(4);
        let y = context.min_y();
        context.emit([origin[0], y, origin[2] + dz]);
        context.emit([origin[0] + 1, y, origin[2] + dz]);
        Ok(())
    }
}

/// The game's side: a random that answers `bound - 1`, and the positions emitted.
#[derive(Default)]
struct Context {
    bounds: Vec<i32>,
    emitted: Vec<[i32; 3]>,
}
impl StructureRandom for Context {
    fn next_int_bound(&mut self, bound: i32) -> i32 {
        self.bounds.push(bound);
        bound - 1
    }
    fn next_long(&mut self) -> i64 {
        unreachable!("the modifier draws only nextInt")
    }
    fn next_float(&mut self) -> f32 {
        unreachable!("the modifier draws only nextInt")
    }
    fn next_double(&mut self) -> f64 {
        unreachable!("the modifier draws only nextInt")
    }
    fn next_boolean(&mut self) -> bool {
        unreachable!("the modifier draws only nextInt")
    }
}
impl PlacementModifierContext for Context {
    fn emit(&mut self, pos: [i32; 3]) {
        self.emitted.push(pos);
    }
    fn metadata(&self) -> &dyn BlockMetadata {
        unreachable!("the modifier reads no block metadata")
    }
    fn random(&mut self) -> &mut dyn StructureRandom {
        self
    }
    fn min_y(&mut self) -> i32 {
        -64
    }
    fn height(&mut self, _: HeightmapType, _: i32, _: i32) -> Option<i32> {
        None
    }
    fn block(&mut self, _: [i32; 3]) -> Option<BlockState> {
        None
    }
}

#[test]
fn wire_loopback_placement_modifier_emits_in_order_and_a_refused_call_fails() {
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    let index = guest::add_handler(vec![guest::view::<dyn PlacementModifier>(Arc::new(Pair))]);
    for corrupt in [false, true] {
        let proxy = WasmHandler {
            index,
            module: WasmMod {
                group: 0,
                subscribed: &[],
                listeners: &[],
                corrupt,
            },
        };
        let mut context = Context::default();
        let answer = proxy.modify(&mut context, [5, 70, -3]);
        assert_eq!(context.bounds, [4], "corrupt {corrupt}");
        assert_eq!(
            context.emitted,
            [[5, -64, 0], [6, -64, 0]],
            "corrupt {corrupt}"
        );
        assert_eq!(answer.is_err(), corrupt, "corrupt {corrupt}: {answer:?}");
    }
}
