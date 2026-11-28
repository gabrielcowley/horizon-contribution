use super::*;

impl Game<'_> {
    /// `Entity.onInsideBlock` of general entity row `entity` of type `kind`
    /// with a living body: one block `state` at `pos` its move meets, `at_end`
    /// when it meets the box where the move ended.
    pub(in crate::game) fn native_general_inside_block(
        &mut self,
        kind: &str,
        entity: EntityRef,
        pos: BlockPos,
        state: BlockStateId,
        at_end: bool,
    ) {
        self.fire_general_inside_block(kind, entity, pos, state, at_end);
    }
}
