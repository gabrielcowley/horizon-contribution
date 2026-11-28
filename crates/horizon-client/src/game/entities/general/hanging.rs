//! The `Hanging` body of a general entity type (`gap/general-entity-bodies`, part b):
//! vanilla's `HangingEntity` and `BlockAttachedEntity` on a general row, sharing the
//! painting's survival check and the hanging entities' coexistence rule.
//!
//! A hanging row has no gravity and no velocity. It breaks when anything hurts it
//! (`BlockAttachedEntity.hurtServer`: a blast, `entity_hurt`), when anything moves or pushes
//! it (`HangingEntity.move`, `push`: a piston, a shulker's lid, `entity_push`), and when the
//! survival check its tick runs every 100 ticks fails (`BlockAttachedEntity.tick`). Every
//! break asks `hanging_break` first. A break drops the type's item as `Painting.dropItem`
//! does, and a hit or a move kills it with `ENTITY_DIE` while a failed check discards it
//! without one. Its facing is the engine's synced attachment `horizon:hanging_direction`
//! (beside the box, `bounds.rs`), so it saves and reaches clients on the attachment paths.

use super::super::Game;
use super::super::commands::Target;
use super::{GeneralRow, general_type_catalog};
use horizon_client::items::ItemStack;
use horizon_core::geometry::{Aabb, Direction, Vec3};
use horizon_core::nbt::{Compound, Tag};
use mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, EntityRef, HangingCheck, HangingSpec, Holder,
    RemovalReason,
};
use std::sync::Arc;

/// `Direction.SOUTH`'s 3D data value: the facing a hanging row has until it is turned
/// (`HangingEntity`'s `direction` field starts south).
pub(in crate::game) const DEFAULT_FACING: u8 = 3;

/// The ticks between two survival checks (`BlockAttachedEntity.tick`'s `checkInterval`).
const CHECK_INTERVAL: u32 = 100;

/// The engine's facing attachment of a hanging row: `{Facing: <3D data value>}`.
pub(in crate::game) static GENERAL_FACING: AttachmentType<u8> = AttachmentType {
    namespace: "horizon",
    name: "hanging_direction",
    default: || DEFAULT_FACING,
    codec: Some(AttachmentCodec {
        encode: |facing| Ok([("Facing".into(), Tag::Byte(*facing as i8))].into()),
        decode: decode_facing,
    }),
    copy_on_death: false,
    sync: true,
};

fn decode_facing(saved: &Compound) -> Result<u8, AttachmentError> {
    match saved.get(&"Facing".into()) {
        Some(Tag::Byte(value)) if (0..6).contains(value) => Ok(*value as u8),
        _ => Err(AttachmentError::Decode),
    }
}

/// A hanging row's own state: its facing, which saves in [`GENERAL_FACING`], and the
/// survival check's counter, which vanilla does not save either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HangingState {
    /// `HangingEntity.direction` as its 3D data value (down 0, up 1, north 2, south 3,
    /// west 4, east 5).
    pub facing: u8,
    /// `BlockAttachedEntity.checkInterval`: ticks since the last survival check.
    pub check_interval: u32,
}

impl Default for HangingState {
    fn default() -> Self {
        Self {
            facing: DEFAULT_FACING,
            check_interval: 0,
        }
    }
}

/// `Direction.from3DDataValue(facing).getUnitVec3i()`.
pub(in crate::game) fn facing_step(facing: u8) -> [i32; 3] {
    match facing {
        0 => [0, -1, 0],
        1 => [0, 1, 0],
        2 => [0, 0, -1],
        4 => [-1, 0, 0],
        5 => [1, 0, 0],
        _ => [0, 0, 1],
    }
}

/// `Direction.get3DDataValue`.
pub(in crate::game) fn facing_of(direction: Direction) -> u8 {
    match direction {
        Direction::Down => 0,
        Direction::Up => 1,
        Direction::North => 2,
        Direction::South => 3,
        Direction::West => 4,
        Direction::East => 5,
    }
}

/// The inverse of [`facing_of`].
pub(in crate::game) fn direction_of(facing: u8) -> Direction {
    match facing {
        0 => Direction::Down,
        1 => Direction::Up,
        2 => Direction::North,
        4 => Direction::West,
        5 => Direction::East,
        _ => Direction::South,
    }
}

/// `HangingEntity.setDirection`'s rotation: a horizontal facing turns the yaw to
/// `get2DDataValue() * 90` with a level pitch, a vertical one tilts the pitch to
/// `-90 * axisDirection.getStep()` with no yaw.
fn facing_rotation(direction: Direction) -> [f32; 2] {
    match direction {
        Direction::South => [0.0, 0.0],
        Direction::West => [90.0, 0.0],
        Direction::North => [180.0, 0.0],
        Direction::East => [270.0, 0.0],
        Direction::Up => [0.0, -90.0],
        Direction::Down => [0.0, 90.0],
    }
}

/// `AABB.intersects` of two `(min, max)` boxes: they share volume, not only a face.
pub(in crate::game) fn boxes_overlap(a: ([f64; 3], [f64; 3]), b: ([f64; 3], [f64; 3])) -> bool {
    (0..3).all(|i| a.0[i] < b.1[i] && b.0[i] < a.1[i])
}

fn corners(bounds: Aabb) -> ([f64; 3], [f64; 3]) {
    let (lo, hi) = (bounds.min_position(), bounds.max_position());
    ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z])
}

impl GeneralRow {
    /// Whether the row's type runs the `Hanging` body.
    pub(in crate::game) fn hangs(&self) -> bool {
        general_type_catalog().hanging(self.type_slot).is_some()
    }
}

impl Game<'_> {
    /// `HangingEntity.survives`' first two conditions for a box `lo..hi` hanging with its
    /// back against `step`'s opposite, the part paintings and hanging rows share: no block
    /// collision in the box, and every block of the support box (the box moved half a block
    /// into the wall and deflated by 1e-7, `calculateSupportBox`) solid or a diode.
    pub(in crate::game) fn hanging_room(
        &self,
        lo: [f64; 3],
        hi: [f64; 3],
        step: [i32; 3],
    ) -> (bool, bool) {
        let bounds = Aabb::new(
            Vec3::new(lo[0], lo[1], lo[2]),
            Vec3::new(hi[0], hi[1], hi[2]),
        );
        let mut scene = horizon_client::collisions::SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        let unobstructed = scene.unobstructed(bounds).unwrap_or(false);
        let s = step.map(f64::from);
        let (slo, shi) = (
            std::array::from_fn::<f64, 3, _>(|i| lo[i] - s[i] * 0.5 + 1.0e-7),
            std::array::from_fn::<f64, 3, _>(|i| hi[i] - s[i] * 0.5 - 1.0e-7),
        );
        let mut supported = true;
        'cells: for x in slo[0].floor() as i32..=shi[0].floor() as i32 {
            for y in slo[1].floor() as i32..=shi[1].floor() as i32 {
                for z in slo[2].floor() as i32..=shi[2].floor() as i32 {
                    if !self.hanging_support_at(x, y, z) {
                        supported = false;
                        break 'cells;
                    }
                }
            }
        }
        (unobstructed, supported)
    }

    /// The per-block rule of the support box: `BlockState.isSolid()` or `DiodeBlock.isDiode`.
    pub(in crate::game) fn hanging_support_at(&self, x: i32, y: i32, z: i32) -> bool {
        let f = self.res.facts().get(self.world.block(x, y, z));
        f.legacy_solid || f.has_class("DiodeBlock")
    }

    /// Whether a hanging row facing `step` overlaps `bounds`: what a painting or an item
    /// frame finds in its `canCoexist`, where an entity of another type is in the way only
    /// when it faces the same way.
    pub(in crate::game) fn general_hanging_in_the_way(
        &self,
        bounds: ([f64; 3], [f64; 3]),
        step: [i32; 3],
    ) -> bool {
        self.resident_entities.general.rows.iter().any(|row| {
            row.hangs()
                && facing_step(row.hanging.facing) == step
                && boxes_overlap(corners(crate::game::entity::Entity::bounds(row)), bounds)
                && !self.persist.removals.contains(&Target::General(row.id))
        })
    }

    /// Vanilla's three survival conditions of the hanging row at `index`
    /// (`HangingEntity.survives`, `canCoexist(false)`): another hanging entity is in its way
    /// when it overlaps the row's box and faces the same way, or is a row of the same type.
    pub(in crate::game) fn general_hanging_check(&self, index: usize) -> HangingCheck {
        let row = &self.resident_entities.general.rows[index];
        let bounds = corners(crate::game::entity::Entity::bounds(row));
        let step = facing_step(row.hanging.facing);
        let (unobstructed, supported) = self.hanging_room(bounds.0, bounds.1, step);
        let painting = self
            .paintings
            .iter()
            .any(|p| p.step() == step && boxes_overlap(p.bounds(), bounds));
        let frame = self
            .item_frames
            .iter()
            .any(|f| f.step() == step && boxes_overlap(f.bounds(), bounds));
        let general = self.resident_entities.general.rows.iter().any(|other| {
            other.id != row.id
                && other.hangs()
                && (other.type_slot == row.type_slot || facing_step(other.hanging.facing) == step)
                && boxes_overlap(corners(crate::game::entity::Entity::bounds(other)), bounds)
                && !self.persist.removals.contains(&Target::General(other.id))
        });
        HangingCheck {
            unobstructed,
            supported,
            alone: !(painting || frame || general),
        }
    }

    /// The hanging row `id`'s `survives()`: vanilla's check, then its type part's answer to
    /// `general_hanging_survives` when one gives it. None when `id` is no hanging row here.
    pub(in crate::game) fn general_hanging_survives(&mut self, id: u32) -> Option<bool> {
        let index = self.general_row_index(id)?;
        let row = &self.resident_entities.general.rows[index];
        if !row.hangs() {
            return None;
        }
        let kind = general_type_catalog().key(row.type_slot);
        let check = self.general_hanging_check(index);
        let mut survives = None;
        if mod_api::__private::has_part_hook(mod_api::HookId::GeneralHangingSurvives)
            && mod_api::__private::entity_part_hooked(
                mod_api::HookId::GeneralHangingSurvives,
                "general_hanging_survives",
                kind,
            )
            && let Some(epoch) = self.native_lifecycle.epoch()
        {
            survives = self.ask_general_hanging_survives(kind, EntityRef::new(epoch, id), check);
        }
        Some(survives.unwrap_or_else(|| check.survives()))
    }

    /// The type part's answer, contained as the type's tick is (`dispatch_general_tick`):
    /// inside a transaction, so a part that panics leaves no write behind and its row keeps
    /// vanilla's rule for this check.
    fn ask_general_hanging_survives(
        &mut self,
        kind: &str,
        entity: EntityRef,
        check: HangingCheck,
    ) -> Option<bool> {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            mod_api::run_transaction(self, |game, _| {
                let mut survives = None;
                game.native_general_hanging_survives(kind, entity, check, &mut survives);
                Ok::<_, ()>(survives)
            })
        }));
        match outcome {
            Ok(Ok(survives)) => survives,
            Ok(Err(())) => None,
            Err(_) => {
                log::error!(
                    "general entity {kind} #{}: its survival check's part panicked; vanilla's rule answered",
                    entity.id()
                );
                None
            }
        }
    }

    /// `entity_hanging_survives`: the survival check of a hanging general row, asked now.
    pub(in crate::game) fn entity_hanging_survives(&mut self, entity: EntityRef) -> Option<bool> {
        if self.remote.is_some() {
            return None;
        }
        let Target::General(id) = self.native_entity_target(entity)? else {
            return None;
        };
        self.general_hanging_survives(id)
    }

    /// `level_hanging_support`: the support box's per-block rule at `pos` of `level`. None
    /// for a stale level, an unloaded block or a client.
    pub(in crate::game) fn level_hanging_support(
        &self,
        level: mod_api::LevelRef,
        pos: [i32; 3],
    ) -> Option<bool> {
        if self.remote.is_some() {
            return None;
        }
        let state = self.level_block_state(level, pos)?;
        let f = self.res.facts().get(state);
        Some(f.legacy_solid || f.has_class("DiodeBlock"))
    }

    /// The facing of the hanging row `id`, if it is one.
    pub(in crate::game) fn general_hanging_direction(&self, id: u32) -> Option<Direction> {
        let row = self
            .resident_entities
            .general
            .rows
            .iter()
            .find(|row| row.id == id)?;
        row.hangs().then(|| direction_of(row.hanging.facing))
    }

    /// `HangingEntity.setDirection` for the hanging row `id`: its facing (saved and synced)
    /// and the rotation that follows from it. False when `id` is no hanging row here.
    pub(in crate::game) fn set_general_hanging_direction(
        &mut self,
        id: u32,
        direction: Direction,
    ) -> bool {
        let Some(index) = self.general_row_index(id) else {
            return false;
        };
        if !self.resident_entities.general.rows[index].hangs() {
            return false;
        }
        let facing = facing_of(direction);
        if GENERAL_FACING
            .set(self, Holder::Entity(id), facing)
            .is_err()
        {
            return false;
        }
        let rotation = facing_rotation(direction);
        self.resident_entities.general.rows.update(index, |row| {
            row.hanging.facing = facing;
            row.rotation = rotation;
            row.previous_rotation = rotation;
            // The pose reaches trackers this tick whatever the type's interval.
            row.needs_sync = true;
        });
        true
    }

    /// A row's facing follows its saved or synced attachment, read again whenever it loads
    /// or the server sends that holder's attachments.
    pub(in crate::game) fn refresh_general_facing(&mut self, id: u32) {
        let Some(index) = self.general_row_index(id) else {
            return;
        };
        if !self.resident_entities.general.rows[index].hangs() {
            return;
        }
        let facing = GENERAL_FACING
            .get_existing(self, Holder::Entity(id))
            .ok()
            .flatten()
            .unwrap_or(DEFAULT_FACING);
        self.resident_entities.general.rows[index].hanging.facing = facing;
    }

    /// `BlockAttachedEntity.tick` for the row at `index`, after its type's own tick: every
    /// 100th tick it checks that it can still hang there, and one that cannot is discarded
    /// and drops its item, with no `ENTITY_DIE` event, unless `hanging_break` keeps it.
    pub(in crate::game) fn tick_general_hanging(&mut self, index: usize) {
        let row = &self.resident_entities.general.rows[index];
        if !row.hangs() {
            return;
        }
        let id = row.id;
        let interval = row.hanging.check_interval + 1;
        let due = interval >= CHECK_INTERVAL;
        self.resident_entities.general.rows[index]
            .hanging
            .check_interval = if due { 0 } else { interval };
        if due && self.general_hanging_survives(id) == Some(false) {
            self.break_general_hanging(id, None, false);
        }
    }

    /// `BlockAttachedEntity.hurtServer`, `HangingEntity.move` and `push` for the hanging row
    /// `id`, or its tick's failed survival check when `killed` is none: `hanging_break` is
    /// asked (a player's break only for the acting guest), then the row is removed (killed,
    /// with `ENTITY_DIE` credited to the killer, or discarded) and its item drops by
    /// `Painting.dropItem`'s rules. False, and the row kept, when it is no hanging row here,
    /// already gone, or a mod kept it.
    pub(in crate::game) fn break_general_hanging(
        &mut self,
        id: u32,
        killed: Option<Option<Target>>,
        by_player: bool,
    ) -> bool {
        let Some(index) = self.general_row_index(id) else {
            return false;
        };
        let row = &self.resident_entities.general.rows[index];
        let Some(spec) = general_type_catalog().hanging(row.type_slot).cloned() else {
            return false;
        };
        if self.persist.removals.contains(&Target::General(id)) {
            return false;
        }
        let kind = general_type_catalog().key(row.type_slot);
        let p = row.position;
        let pos = [p.x, p.y, p.z].map(|v| v.floor() as i32);
        if self.hanging_break_cancelled(kind, pos, by_player) {
            return false;
        }
        // The hook had `&mut self`: find the row again by id.
        let Some(index) = self.general_row_index(id) else {
            return false;
        };
        let row = &self.resident_entities.general.rows[index];
        let (lo, hi) = corners(crate::game::entity::Entity::bounds(row));
        let at = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
        let step = facing_step(row.hanging.facing);
        let Some(epoch) = self.native_lifecycle.epoch() else {
            return false;
        };
        let reason = if killed.is_some() {
            RemovalReason::Killed
        } else {
            RemovalReason::Discarded
        };
        if !self.entity_remove(EntityRef::new(epoch, id), reason) {
            return false;
        }
        let source = killed.flatten();
        if killed.is_some() {
            self.entity_killed_event_by(at, source);
        }
        // `causedBy instanceof Player && hasInfiniteMaterials()`.
        let creative_breaker = self.creative_source_of(source);
        self.drop_general_hanging(&spec, at, step, creative_breaker);
        true
    }

    /// `Painting.dropItem` with the type's item and sound: nothing while `entityDrops` is off,
    /// otherwise the break sound, then the item unless a creative player broke it.
    fn drop_general_hanging(
        &mut self,
        spec: &HangingSpec,
        at: [f64; 3],
        step: [i32; 3],
        creative_breaker: bool,
    ) {
        if !self
            .ui
            .rules
            .bool_at(horizon_client::gamerules::vanilla::ENTITY_DROPS)
        {
            return;
        }
        self.audio.play(&spec.break_sound, Some(at), 1.0, 1.0);
        if creative_breaker || self.res.items().get(&spec.item).is_none() {
            return;
        }
        let stack = ItemStack {
            item: Arc::from(spec.item.as_str()),
            count: 1,
            damage: 0,
            extra: None,
        };
        self.spawn_item(
            stack,
            super::super::frames::hanging_drop_position(at, step),
            [0.0; 3],
            10,
        );
    }

    /// `BlockAttachedEntity.hurtServer` of the row `id` by a source of `damage_type` caused
    /// by `cause`: refused for an invulnerable row (`isInvulnerableToBase`) and for a mob's
    /// hit without `mobGriefing`, otherwise a kill and a drop. None when `id` is no hanging
    /// row, which takes no damage.
    pub(in crate::game) fn hurt_general_hanging(
        &mut self,
        id: u32,
        damage_type: &str,
        cause: Option<Target>,
    ) -> Option<bool> {
        self.general_hanging_direction(id)?;
        let creative = self.creative_source_of(cause);
        if self.saved_refuses_damage(id, damage_type, creative) {
            return Some(false);
        }
        if matches!(cause, Some(Target::Mob(_)))
            && !self
                .ui
                .rules
                .bool_at(horizon_client::gamerules::vanilla::MOB_GRIEFING)
        {
            return Some(false);
        }
        let by_player = matches!(cause, Some(Target::Player | Target::Guest(_)));
        Some(self.break_general_hanging(id, Some(cause), by_player))
    }

    /// `ServerExplosion.hurtEntities` for the hanging rows: a row whose position the blast
    /// reaches is killed and dropped (`BlockAttachedEntity.hurtServer`), credited to the
    /// blast's cause. The caller has already spared the decorations a blast leaves.
    pub(in crate::game) fn explosion_breaks_general_hanging(
        &mut self,
        center: [f64; 3],
        diameter: f64,
    ) {
        let hit: Vec<u32> = self
            .resident_entities
            .general
            .rows
            .iter()
            .filter(|row| {
                row.hangs()
                    && self.combat.visits(row.id)
                    && super::super::explosion::blast_reaches(
                        center,
                        diameter,
                        [row.position.x, row.position.y, row.position.z],
                    )
            })
            .map(|row| row.id)
            .collect();
        let damage_type = self.blast_damage_type();
        for id in hit {
            if self.saved_refuses_damage(id, damage_type, false) {
                continue;
            }
            let credit = self.explosion_kill_credit();
            self.break_general_hanging(id, Some(credit), false);
        }
    }

    /// `HangingEntity.move` and `push`: anything that moves the hanging row `id` (a piston,
    /// a shulker's lid, `entity_push`) breaks it on the server, `kill` then
    /// `dropItem(level, null)`. True when `id` is a hanging row, which then never moves.
    pub(in crate::game) fn general_hanging_moved(&mut self, id: u32) -> bool {
        if self.remote.is_some() || self.general_hanging_direction(id).is_none() {
            return false;
        }
        self.break_general_hanging(id, Some(None), false);
        true
    }
}

#[cfg(test)]
#[path = "hanging_tests.rs"]
mod tests;
