//! The `general_entity_client_tick` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// A client tick of this client's copy of one general entity row of type
    /// `kind` (`mymod:crate`), with ID `entity`, in this client's level
    /// `level` (the one `LevelRef::collide` reads): the client half of
    /// `Entity.tick` for a row the server tracks to this client, after the
    /// copy took its step toward the server's pose (vanilla's interpolation,
    /// `LivingEntity.aiStep`'s `lerpPositionAndRotationStep`). Once per client
    /// tick while the level runs, for every copy of the type, drawn or not.
    /// It runs no authority: the host may move only this copy, with
    /// `EntityRef::client_set_position`, `client_set_velocity` and
    /// `client_lerp_to`, and the server's next move or teleport of the row
    /// still overwrites what it wrote. `LevelRef::collide` gives vanilla's
    /// `collideBoundingBox` for a predicted step. Never called on a server, and
    /// never again for a copy the server removed. Client-side.
    #[notes = r##"## Client ticks of general entities: smoothing a row the server moves

`general_entity_client_tick` is a general entity type's client `Entity.tick`: what a client does with its
copy of a row between the server's updates, such as an item-like body that predicts its first second of a
throw, or a part eased toward where the server will put it. A part attached to the type hears it; vanilla
families never reach it.

It runs on each client (the window's own in single player included) once per client tick while its level
runs (not paused or frozen), for every copy of the type the server tracks to it, after the copy's step
toward the pose the server last sent (vanilla's three-step `lerpTo`). The copy's age (`Entity.tickCount`)
already counts that tick.

The host moves only the copy being ticked, and only on this client:

- `EntityRef::client_set_position` places it (`Entity.setPos`); its previous position stays, so a frame
  eases from where it was.
- `EntityRef::client_set_velocity` sets its motion (`Entity.setDeltaMovement`).
- `EntityRef::client_lerp_to` starts a new ease toward a pose over a number of ticks (`Entity.lerpTo`).
  `EntityRef::client_lerp_target` reads the ease in progress (`lerpX`, `lerpY`, `lerpZ`, `lerpSteps`).

Each one refuses any other entity, and refuses outside this hook. `LevelRef::collide` is vanilla's
`Entity.collideBoundingBox`: how far a box may move before blocks, the world border and the entities that
stop movers (boats, shulkers) stop it, with no step-up. Nothing the hook writes reaches the server: its
next move, teleport or velocity for the row overwrites the copy, as vanilla's client updates do.

`general_motion_received` is its counterpart for a velocity the server sends (`Entity.lerpMotion`): a part
may answer the motion the copy keeps, such as an average of its own and the received one."##]
    #[hook(
        side = "client",
        contract = "listen",
        scope = "entity",
        key = "kind",
        neoforge = "none (vanilla Entity.tick on the client level; NeoForge's EntityTickEvent is the global pre/post pair)"
    )]
    fn general_entity_client_tick();
}
