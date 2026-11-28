//! The client tick of general entity rows (`gap/general-entity-bodies`, the client tick): each
//! client copy hears `general_entity_client_tick` after its step toward the server's pose, and may
//! then move only itself, on this client. `general_motion_received` answers a velocity the server
//! sends, and `level_collide` is vanilla's `Entity.collideBoundingBox` for a predicted step.
//! Nothing here reaches the server: its next move, teleport or velocity overwrites the copy.
use super::Game;
use crate::game::general::general_type_catalog;
use horizon_core::geometry::{Aabb, Vec3};
use mod_api::{EntityRef, LevelRef};

impl Game<'_> {
    /// `general_entity_client_tick` for each of the client's general rows, in their order, once
    /// the rows took this tick's step (`tick_remote_general`) and the eases in progress are back
    /// in place, so a tick reads and replaces its row's own. Nothing runs while no part has a
    /// client tick, and a row removed during the pass is not called.
    pub(super) fn client_tick_general_rows(&mut self) {
        if !mod_api::__private::has_part_hook(mod_api::HookId::GeneralEntityClientTick) {
            return;
        }
        let (Some(epoch), Some(level)) =
            (self.native_lifecycle.epoch(), self.native_client_level())
        else {
            return;
        };
        let catalog = general_type_catalog();
        let rows: Vec<(u32, usize)> = self
            .resident_entities
            .general
            .rows
            .iter()
            .map(|row| (row.id, row.type_slot))
            .collect();
        for (id, type_slot) in rows {
            if self.client_general_row(id).is_none() {
                continue;
            }
            self.resident_entities.general.client_ticking = Some(id);
            self.native_general_entity_client_tick(
                catalog.key(type_slot),
                EntityRef::new(epoch, id),
                level,
            );
            self.resident_entities.general.client_ticking = None;
        }
    }

    /// The velocity this client's copy of general row `id` keeps from `received`
    /// (`Entity.lerpMotion`): a part's `general_motion_received` answer, or `received`.
    pub(super) fn general_motion_kept(&self, id: u32, received: [f64; 3]) -> [f64; 3] {
        let (Some(epoch), Some(i)) = (self.native_lifecycle.epoch(), self.client_general_row(id))
        else {
            return received;
        };
        let row = &self.resident_entities.general.rows[i];
        let current = [row.velocity.x, row.velocity.y, row.velocity.z];
        let kind = general_type_catalog().key(row.type_slot);
        let mut motion = None;
        self.native_general_motion_received(
            kind,
            EntityRef::new(epoch, id),
            current,
            received,
            &mut motion,
        );
        motion
            .filter(|m| m.iter().all(|v| v.is_finite()))
            .unwrap_or(received)
    }

    /// The place of this client's copy of general row `id`, or None on a server or for no such
    /// row.
    fn client_general_row(&self, id: u32) -> Option<usize> {
        if self.is_logical_server() {
            return None;
        }
        self.resident_entities
            .general
            .rows
            .iter()
            .position(|row| row.id == id)
    }

    /// The place of the row `general_entity_client_tick` is ticking, when `entity` names it: the
    /// only row a client tick may write, and only during that tick.
    fn client_ticked_row(&self, entity: EntityRef) -> Option<usize> {
        let ticking = self.resident_entities.general.client_ticking?;
        (self.native_lifecycle.epoch() == Some(entity.epoch()) && entity.id() == ticking)
            .then_some(())?;
        self.client_general_row(ticking)
    }

    /// `Entity.setPos` on this client's copy of the row being client-ticked.
    pub(in crate::game) fn entity_client_set_position(
        &mut self,
        entity: EntityRef,
        pos: [f64; 3],
    ) -> bool {
        let Some(i) = self.client_ticked_row(entity) else {
            return false;
        };
        if !pos.iter().all(|v| v.is_finite()) {
            return false;
        }
        self.resident_entities
            .general
            .rows
            .update(i, |row| row.position = Vec3::new(pos[0], pos[1], pos[2]));
        true
    }

    /// `Entity.setDeltaMovement` on this client's copy of the row being client-ticked.
    pub(in crate::game) fn entity_client_set_velocity(
        &mut self,
        entity: EntityRef,
        velocity: [f64; 3],
    ) -> bool {
        let Some(i) = self.client_ticked_row(entity) else {
            return false;
        };
        if !velocity.iter().all(|v| v.is_finite()) {
            return false;
        }
        self.resident_entities.general.rows.update(i, |row| {
            row.velocity = Vec3::new(velocity[0], velocity[1], velocity[2]);
        });
        true
    }

    /// `Entity.lerpTo` on this client's copy of the row being client-ticked: a new ease toward
    /// `pos` and `rotation` over `steps` ticks, in place of the one in progress.
    pub(in crate::game) fn entity_client_lerp_to(
        &mut self,
        entity: EntityRef,
        pos: [f64; 3],
        rotation: [f32; 2],
        steps: u32,
    ) -> bool {
        if self.client_ticked_row(entity).is_none()
            || steps == 0
            || !pos.iter().all(|v| v.is_finite())
            || !rotation.iter().all(|v| v.is_finite())
        {
            return false;
        }
        let Some(remote) = self.remote.as_mut() else {
            return false;
        };
        remote.lerp_to(entity.id() as i32, pos, rotation, steps);
        true
    }

    /// The ease in progress of this client's copy of a general row: its target and ticks left.
    pub(in crate::game) fn entity_client_lerp_target(
        &self,
        entity: EntityRef,
    ) -> Option<([f64; 3], u32)> {
        (self.native_lifecycle.epoch() == Some(entity.epoch())).then_some(())?;
        self.client_general_row(entity.id())?;
        self.remote.as_ref()?.lerp_target(entity.id() as i32)
    }

    /// `Entity.collideBoundingBox` on this client: `motion` cut by the blocks, the world border
    /// and the entities that stop movers, for `entity`'s box in this client's level.
    pub(in crate::game) fn level_collide(
        &self,
        level: LevelRef,
        entity: EntityRef,
        motion: [f64; 3],
    ) -> Option<[f64; 3]> {
        if self.is_logical_server() || self.native_client_level() != Some(level) {
            return None;
        }
        if !motion.iter().all(|v| v.is_finite()) {
            return None;
        }
        let [min, max] = self.entity_bounding_box(entity)?;
        let bounds = Aabb::new(
            Vec3::new(min[0], min[1], min[2]),
            Vec3::new(max[0], max[1], max[2]),
        );
        let [x, _, z] = self.entity_position(entity)?;
        let entities = self.item_entity_collisions();
        let scene = horizon_client::collisions::SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        // A query, not a move: vanilla's `collideBoundingBox` is static and moves nothing, so
        // it does not go through the move gate (arismoko/horizon#2414), as a particle's step.
        #[allow(clippy::disallowed_methods)]
        let moved = scene.resolve_in_border_with(
            bounds,
            Vec3::new(motion[0], motion[1], motion[2]),
            0.0,
            false,
            Some((self.border.geometry(), [x, z])),
            &entities,
        );
        moved.ok().map(|v| [v.x, v.y, v.z])
    }
}
