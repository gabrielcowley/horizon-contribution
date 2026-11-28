//! The `general_motion_received` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// `Entity.lerpMotion` for this client's copy of a general entity row of
    /// type `kind`, with ID `entity`: the server sent the row's velocity
    /// (`ClientboundSetEntityMotionPacket`, and the add packet's through
    /// `recreateFromPacket`, when the fresh copy's own is zero). `current` is
    /// the copy's velocity before it and `received` the one sent. Answer
    /// the velocity the copy keeps; with no answer, it keeps `received`, as
    /// vanilla's. Asked only for a type a part is attached
    /// to. Client-side.
    #[hook(
        side = "client",
        contract = "replace",
        out = "motion",
        scope = "entity",
        key = "kind",
        neoforge = "none (vanilla Entity.lerpMotion; NeoForge adds no event)"
    )]
    fn general_motion_received();
}
