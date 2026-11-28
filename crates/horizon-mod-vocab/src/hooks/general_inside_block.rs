//! The `general_inside_block` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// One block a moving general entity row of type `kind` with a living body,
    /// with ID `entity`, is inside: vanilla's `Entity.onInsideBlock` override,
    /// told on the same walk as the blocks' own effects (`checkInsideBlocks`),
    /// once per block, after the block's own `entityInside`. `state` is the
    /// block's state at `pos`; `at_end` says the block meets the box where the
    /// move ended, which a rule that the Original ran on its box at the end of
    /// the move (before 26.3's swept walk) reads. Server-side; told only for
    /// general entity types.
    #[hook(
        side = "server",
        contract = "listen",
        scope = "entity",
        key = "kind",
        neoforge = "none (vanilla Entity.onInsideBlock override)"
    )]
    fn general_inside_block();
}
