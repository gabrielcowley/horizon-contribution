//! A client's copies of the server's general entity rows (`gap/general-entity-sync`): made from
//! vanilla's add packet, moved by its move, teleport and velocity packets with vanilla's
//! three-step `lerpTo`, and dropped with its remove packet. They live in the same general store
//! as the server's rows, under the server's network id, so every both-side entity read finds
//! them; no mod behaviour runs on them (a client never ticks a world).
use super::Game;
use crate::game::general::{GeneralRow, general_type_catalog};
use horizon_client::join_seq::JoinSeq;
use horizon_core::geometry::Vec3;
use horizon_core::nbt::Tag;
use horizon_protocol::entity_data::Value;

impl Game<'_> {
    /// `ClientboundAddEntityPacket` for a general type: the client's row, in place of any row
    /// that had the id before. False for a kind no mod declared as a general type.
    pub(in crate::game) fn spawn_remote_general(
        &mut self,
        id: i32,
        kind: &str,
        position: [f64; 3],
        rotation: [f32; 2],
        velocity: [f64; 3],
    ) -> bool {
        let Some(type_slot) = general_type_catalog().slot(kind) else {
            return false;
        };
        let id = id as u32;
        let at = Vec3::new(position[0], position[1], position[2]);
        // A living body starts at full health; the entity data that follows the add
        // packet brings the server's health and on-fire flag.
        let living = self.general_living_for(type_slot, &Tag::Compound(Default::default()));
        let rows = &mut self.resident_entities.general.rows;
        rows.retain(|row| row.id != id);
        rows.push(GeneralRow {
            id,
            uuid: self.persist.uuids.get(&id).copied().unwrap_or_default(),
            join: JoinSeq::UNJOINED,
            type_slot,
            position: at,
            previous: at,
            velocity: Vec3::new(velocity[0], velocity[1], velocity[2]),
            rotation,
            previous_rotation: rotation,
            fall_distance: 0.0,
            payload_version: 0,
            payload_bytes: Vec::new(),
            mount: None,
            local_box: None,
            needs_sync: false,
            age: 0,
            attachment_lifetime: Default::default(),
            living,
        });
        // The box and the rest of its attachments follow the add packet; a
        // row that had them under this id before keeps none of them.
        self.refresh_general_box(id);
        true
    }

    /// `Entity.lerpMotion` for a client's general row: true when `id` is one.
    pub(super) fn remote_general_motion(&mut self, id: i32, velocity: [f64; 3]) -> bool {
        let rows = &mut self.resident_entities.general.rows;
        let Some(i) = rows.iter().position(|row| row.id == id as u32) else {
            return false;
        };
        rows.update(i, |row| {
            row.velocity = Vec3::new(velocity[0], velocity[1], velocity[2]);
        });
        true
    }

    /// `ClientboundSetEntityDataPacket` for a client's living row: `DATA_SHARED_FLAGS_ID`'s
    /// on-fire bit, which its flames are drawn from, and `LivingEntity.DATA_HEALTH_ID`.
    pub(in crate::game) fn remote_general_data(&mut self, id: i32, values: &[(u8, i32, Value)]) {
        let rows = &mut self.resident_entities.general.rows;
        let Some(i) = rows
            .iter()
            .position(|row| row.id == id as u32 && row.living.is_some())
        else {
            return;
        };
        rows.update(i, |row| {
            let Some(living) = row.living.as_deref_mut() else {
                return;
            };
            for (index, _, value) in values {
                match (index, value) {
                    (0, Value::Byte(flags)) => living.shared_on_fire = flags & 1 != 0,
                    (9, Value::Float(health)) => living.health = *health,
                    _ => {}
                }
            }
        });
    }

    /// `ClientboundRemoveEntitiesPacket`: the client's rows among `removed` go.
    pub(super) fn remove_remote_general(
        &mut self,
        removed: &horizon_client::fast_hash::FxHashSet<u32>,
    ) {
        let general = &mut self.resident_entities.general;
        if !general.rows.is_empty() {
            general.rows.retain(|row| !removed.contains(&row.id));
            general.rider_places.retain(|id, _| !removed.contains(id));
        }
    }

    /// One client tick of the general rows: each takes a step of its `lerpTo` toward the
    /// server's pose (`step`, `turn` and `tilt`, the shared interpolation of `tick_remote_entities`), and
    /// counts a tick of age for its renderer. Nothing else runs: a client's row has no
    /// behaviour.
    pub(super) fn tick_remote_general(
        &mut self,
        step: &mut impl FnMut(u32, Vec3) -> Vec3,
        turn: impl Fn(u32, f32) -> f32,
        tilt: impl Fn(u32, f32) -> f32,
    ) {
        let rows = &mut self.resident_entities.general.rows;
        for i in 0..rows.len() {
            let (id, position, [yaw, pitch]) = (rows[i].id, rows[i].position, rows[i].rotation);
            let to = step(id, position);
            let rotation_to = [turn(id, yaw), tilt(id, pitch)];
            rows.update(i, |row| {
                row.previous = position;
                row.previous_rotation = row.rotation;
                row.position = to;
                row.rotation = rotation_to;
                row.age = row.age.wrapping_add(1);
            });
        }
    }
}
