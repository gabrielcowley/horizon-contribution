//! Fluid interactions for native mods: the ordered list a fluid converts by, and what it would
//! place beside one neighbour (`FluidInteractionRegistry`, read without writing).
use super::Game;
use horizon_core::position::BlockPos;
use horizon_world::registry::BlockStateId;
use mod_api::{FluidInteraction, FluidKey, FluidState, LevelRef};

impl Game<'_> {
    /// The resolved interactions of `fluid`'s family, vanilla's first, or None for a fluid with
    /// no world form.
    pub(super) fn fluid_interactions(&self, fluid: FluidKey) -> Option<Vec<FluidInteraction>> {
        let kind = super::native_world_read::engine_fluid_kind(fluid)
            .filter(|kind| *kind != horizon_world::fluid::FluidKind::Empty)?;
        Some(
            self.res
                .behaviours()
                .fluid_interactions_of(kind)
                .map(|interaction| {
                    let d = interaction.declared();
                    let mut api = match (&d.touching_fluid, &d.touching_block) {
                        (Some(touching), _) => FluidInteraction::touching_fluid(
                            &d.fluid, touching, &d.source, &d.flowing,
                        ),
                        (None, touching) => FluidInteraction::touching_block(
                            &d.fluid,
                            touching.as_deref().unwrap_or_default(),
                            &d.source,
                            &d.flowing,
                        ),
                    };
                    api.when.block_below = d.block_below.clone();
                    api
                })
                .collect(),
        )
    }

    /// What the first matching interaction of a fluid in `fluid` would place at `pos`, judged
    /// against the block at `neighbour` and the one under `pos`, in a server level.
    pub(super) fn level_fluid_interaction_at(
        &self,
        level: LevelRef,
        fluid: FluidState,
        [x, y, z]: [i32; 3],
        neighbour: [i32; 3],
    ) -> Option<BlockStateId> {
        let kind = super::native_world_read::engine_fluid_kind(fluid.fluid)?;
        let world = self.native_level_world(level)?;
        let behaviours = self.res.behaviours();
        if !behaviours.has_fluid_interactions(kind) {
            return None;
        }
        let read = |[x, y, z]: [i32; 3]| world.is_loaded_at(x, y, z).then(|| world.block(x, y, z));
        let neighbour = read(neighbour)?;
        read([x, y, z])?;
        let below = BlockPos::new(x, y - 1, z);
        let below = read([below.x, below.y, below.z]).unwrap_or_else(|| behaviours.air());
        behaviours
            .fluid_interaction(kind, fluid.source, neighbour, || below)
            .map(|(_, placed)| placed)
    }
}
