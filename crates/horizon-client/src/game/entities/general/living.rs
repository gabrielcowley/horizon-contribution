//! PURPOSE: a general entity row with a living body (`GeneralBody::Living`): vanilla's
//! `LivingEntity` without AI, as the armor stand runs it, for a mod's own type.
//! WHY: the Original's package is a `LivingEntity` (gravity, collision, pushing, fire,
//! drowning, damage). The row's movement runs the stand's shared body
//! (`horizon_client::entities::living_body`); its damage is `LivingEntity.hurtServer` for
//! an entity with no armor and no effects, the dragon's path without its phases; the
//! Original's overrides reach the mod through the type-scoped `general_*` moments.
//! REFS: vanilla `LivingEntity.baseTick`, `aiStep`, `travel`, `hurtServer`, `actuallyHurt`,
//! `knockback`, `causeFallDamage`, `pushEntities`, `tickDeath`; `Entity.baseTick`,
//! `checkBelowWorld`, `isInWall`, `checkInsideBlocks`, `push(Entity)`.
//! LOGS: none. READS-BY: `tick.rs` (after `general_entity_tick`), the damage routes.
//! ENDS-WHEN: never; the body is the row's for good.

use super::super::Game;
use super::super::commands::Target;
use super::{GeneralRow, general_type_catalog};
use horizon_client::entities::living_body::{self, BodyShape, LivingBodyMut, LivingMedium};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_core::nbt::Tag;
use horizon_core::position::BlockPos;
use mod_api::{EntityRef, GeneralBody};

/// `LivingEntity`'s maximum air (`getMaxAirSupply`).
const MAX_AIR: i32 = 300;
/// Ticks a dead body lies before it is removed (`LivingEntity.tickDeath`).
const DEATH_TICKS: i32 = 20;

/// The living half of a general row with a living body: `LivingEntity`'s own state beside
/// the row's pose, velocity and fall distance. Saved with the row's record under vanilla's
/// names, never in the mod's attachments.
#[derive(Clone, Debug)]
pub struct GeneralLiving {
    /// `LivingEntity.getHealth`.
    pub health: f32,
    /// `Entity.remainingFireTicks`.
    pub fire_ticks: i32,
    /// `Entity.getAirSupply`.
    pub air: i32,
    pub on_ground: bool,
    /// `Entity.mainSupportingBlockPos`.
    pub support: horizon_world::ground_support::GroundState,
    /// `LivingEntity.hurtTime`: the red flash after a full hit.
    pub hurt_time: i32,
    /// `Entity.invulnerableTime`: the cooldown after a hit.
    pub invulnerable_time: i32,
    /// `LivingEntity.lastHurt`: what the cooldown compares a new hit with.
    pub last_hurt: f32,
    /// `LivingEntity.deathTime`: ticks since it died.
    pub death_time: i32,
    /// The row's own fire immunity (`entity_set_fire_immune`), over its type's.
    pub fire_immune: Option<bool>,
    /// On a client, `DATA_SHARED_FLAGS_ID`'s on-fire bit as the server last sent it: a
    /// client's `Entity.isOnFire` reads it beside its own fire ticks.
    pub shared_on_fire: bool,
}

impl GeneralLiving {
    /// A fresh body at full `health`.
    pub fn new(health: f32) -> Self {
        Self {
            health,
            fire_ticks: 0,
            air: MAX_AIR,
            on_ground: false,
            support: Default::default(),
            hurt_time: 0,
            invulnerable_time: 0,
            last_hurt: 0.0,
            death_time: 0,
            fire_immune: None,
            shared_on_fire: false,
        }
    }

    /// Whether it is dead (`LivingEntity.isDeadOrDying`).
    pub fn dead(&self) -> bool {
        self.health <= 0.0
    }

    /// The body as a saved record holds it (`LivingEntity.addAdditionalSaveData` and
    /// `Entity.saveWithoutId`'s names), at full `max_health` where the tag has none: a
    /// record written before the type had a living body loads with the body's defaults.
    pub fn from_tag(tag: &Tag, max_health: f32) -> Self {
        let mut living = Self::new(max_health);
        let int = |key: &str| tag.get(key).and_then(horizon_client::nbt_path::int_value);
        if let Some(Tag::Float(bits)) = tag.get("Health") {
            let health = f32::from_bits(*bits);
            if health.is_finite() {
                living.health = health.clamp(0.0, max_health);
            }
        }
        if let Some(fire) = int("Fire") {
            living.fire_ticks = fire;
        }
        if let Some(air) = int("Air") {
            living.air = air;
        }
        if let Some(hurt) = int("HurtTime") {
            living.hurt_time = hurt;
        }
        if let Some(death) = int("DeathTime") {
            living.death_time = death;
        }
        if let Some(Tag::Byte(on_ground)) = tag.get("OnGround") {
            living.on_ground = *on_ground != 0;
        }
        if let Some(Tag::Byte(immune)) = tag.get("HorizonFireImmune") {
            living.fire_immune = Some(*immune != 0);
        }
        living
    }

    /// Write the body into the row's saved compound, beside its pose.
    pub fn write(&self, tag: &mut Tag, fall_distance: f64) {
        let Tag::Compound(map) = tag else {
            return;
        };
        map.insert("Health".into(), Tag::Float(self.health.to_bits()));
        map.insert("Fire".into(), Tag::Short(self.fire_ticks as i16));
        map.insert("Air".into(), Tag::Short(self.air as i16));
        map.insert("HurtTime".into(), Tag::Short(self.hurt_time as i16));
        map.insert("DeathTime".into(), Tag::Short(self.death_time as i16));
        map.insert("OnGround".into(), Tag::Byte(i8::from(self.on_ground)));
        map.insert("fall_distance".into(), Tag::Double(fall_distance.to_bits()));
        if let Some(immune) = self.fire_immune {
            map.insert("HorizonFireImmune".into(), Tag::Byte(i8::from(immune)));
        }
    }
}

impl GeneralRow {
    /// Whether fire and lava leave this row alone: its own override, else its type's
    /// `fire_immune` (`Entity.fireImmune`).
    pub fn fire_immune(&self) -> bool {
        self.living
            .as_ref()
            .and_then(|living| living.fire_immune)
            .unwrap_or_else(|| general_type_catalog().fire_immune(self.type_slot))
    }

    /// `Entity.displayFireAnimation` of a living row: it is on fire (its fire ticks, or on a
    /// client the server's on-fire flag) and not fire immune.
    pub fn displays_fire(&self) -> bool {
        self.living
            .as_ref()
            .is_some_and(|living| living.fire_ticks > 0 || living.shared_on_fire)
            && !self.fire_immune()
    }

    /// The row's living body, lent to the shared movement code for one step.
    fn living_body(&mut self) -> Option<LivingBodyMut<'_>> {
        let catalog = general_type_catalog();
        let local = match self.local_box {
            Some(local) => local,
            None => {
                let (width, height) = catalog.dimensions(self.type_slot);
                let half = width / 2.0;
                [[-half, 0.0, -half], [half, height, half]]
            }
        };
        let shape = BodyShape {
            kind: catalog.key(self.type_slot),
            local,
            no_gravity: false,
            // `LivingEntity.getDefaultGravity`.
            gravity: 0.08,
            leather_boots: false,
        };
        let living = self.living.as_deref_mut()?;
        Some(LivingBodyMut {
            id: self.id,
            position: &mut self.position,
            velocity: &mut self.velocity,
            on_ground: &mut living.on_ground,
            fall_distance: &mut self.fall_distance,
            support: &mut living.support,
            shape,
        })
    }

    /// `Entity.getEyeY`: 0.85 of the box's height above its feet (`EntityDimensions.scalable`).
    fn eye_y(&self) -> f64 {
        use super::super::entity::Entity;
        let bounds = self.bounds();
        self.position.y + (bounds.max_position().y - bounds.min_position().y) * f64::from(0.85_f32)
    }
}

/// What one hit carries into a general row's `hurtServer`.
#[derive(Clone, Copy)]
pub(in crate::game) struct GeneralHit<'a> {
    pub damage_type: &'a str,
    pub amount: f32,
    pub direct: Option<Target>,
    pub causing: Option<Target>,
    /// The source's position, which the knockback pushes away from.
    pub from: Option<[f64; 3]>,
}

impl Game<'_> {
    /// The living body a fresh or loaded row of type `slot` starts with, from its saved
    /// `tag`: none for a type without one.
    pub(in crate::game) fn general_living_for(
        &self,
        slot: usize,
        tag: &Tag,
    ) -> Option<Box<GeneralLiving>> {
        let catalog = general_type_catalog();
        let GeneralBody::Living(_) = catalog.body(slot) else {
            return None;
        };
        let max = self.general_max_health(slot);
        Some(Box::new(GeneralLiving::from_tag(tag, max)))
    }

    /// `LivingEntity.getMaxHealth` of type `slot`: its declared attribute set
    /// (`declare_entity_attributes`, the `minecraft:armor_stand` template), else 20.
    pub(in crate::game) fn general_max_health(&self, slot: usize) -> f32 {
        let key = general_type_catalog().key(slot);
        self.res
            .motion()
            .attributes
            .entity_row(key)
            .and_then(|row| row.get("minecraft:max_health").copied())
            .map_or(20.0, |max| max as f32)
    }

    /// The living tick of the general row at `index`, after its `general_entity_tick`
    /// (`LivingEntity.tick` runs after the subclass's own lines, as `super.tick()`):
    /// `LivingEntity.baseTick` (the void, fire, air, the wall), then `aiStep` (`travel`,
    /// the block effects, `pushEntities`), and the death count. A row without a living
    /// body, or one its tick removed, is left alone.
    pub(in crate::game) fn tick_general_living(&mut self, index: usize) {
        self.tick_general_living_as(index, false);
    }

    /// The living tick of a rider (`Entity.rideTick`): its velocity is zeroed and it gets
    /// the base part of the body tick (the void, fire, air, the wall, damage and death),
    /// but no travel and no pushing: its vehicle places it.
    pub(in crate::game) fn tick_general_living_rider(&mut self, index: usize) {
        self.tick_general_living_as(index, true);
    }

    fn tick_general_living_as(&mut self, index: usize, riding: bool) {
        let Some(row) = self.resident_entities.general.rows.get(index) else {
            return;
        };
        if row.living.is_none() || self.persist.removals.contains(&Target::General(row.id)) {
            return;
        }
        let (id, dead) = (
            row.id,
            row.living.as_ref().is_some_and(|living| living.dead()),
        );
        if riding {
            // `Entity.rideTick`: `setDeltaMovement(Vec3.ZERO)` before the tick.
            self.resident_entities
                .general
                .rows
                .update(index, |row| row.velocity = Vec3::ZERO);
        }
        // `LivingEntity.tickDeath`: a dead body lies for 20 ticks, then goes.
        if dead {
            self.resident_entities.general.rows.update(index, |row| {
                if let Some(living) = row.living.as_deref_mut() {
                    living.death_time += 1;
                }
            });
            let done = self.resident_entities.general.rows[index]
                .living
                .as_ref()
                .is_some_and(|living| living.death_time >= DEATH_TICKS);
            if done {
                self.remove_general_row(id);
            }
            return;
        }
        self.resident_entities.general.rows.update(index, |row| {
            if let Some(living) = row.living.as_deref_mut() {
                living.hurt_time = (living.hurt_time - 1).max(0);
                living.invulnerable_time = (living.invulnerable_time - 1).max(0);
            }
        });
        let heights = self.general_living_base_tick(id);
        if riding || !self.general_living_alive(id) {
            return;
        }
        self.general_living_travel(id, heights);
        if !self.general_living_alive(id) {
            return;
        }
        self.general_living_push_entities(id);
    }

    /// The row's place, while it is a live living row this tick.
    fn general_living_index(&self, id: u32) -> Option<usize> {
        if self.persist.removals.contains(&Target::General(id)) {
            return None;
        }
        self.resident_entities
            .general
            .rows
            .iter()
            .position(|row| row.id == id && row.living.is_some())
    }

    fn general_living_alive(&self, id: u32) -> bool {
        self.general_living_index(id).is_some_and(|i| {
            self.resident_entities.general.rows[i]
                .living
                .as_ref()
                .is_some_and(|living| !living.dead())
        })
    }

    /// `Entity.baseTick` and `LivingEntity.baseTick` for a body with no AI: below the
    /// world it is hurt by the void; water puts its fire out, a burning body that is
    /// not fire immune takes `on_fire` each second; a body in a wall suffocates; under
    /// water its air runs out and it drowns, above water it refills.
    fn general_living_base_tick(&mut self, id: u32) -> (f64, f64) {
        let Some(i) = self.general_living_index(id) else {
            return (0.0, 0.0);
        };
        // `Entity.baseTick`'s `updateInWaterStateAndDoFluidPushing` and
        // `updateFluidOnEyes` come first.
        let (water_height, lava_height, eye_in_water) = self.general_living_fluids(i);
        if water_height > 0.0 {
            self.resident_entities
                .general
                .rows
                .update(i, |row| row.fall_distance = 0.0);
        }
        let heights = (water_height, lava_height);
        let min_y = f64::from(self.world.layout().min_y());
        let row = &self.resident_entities.general.rows[i];
        // `Entity.checkBelowWorld`: 64 blocks under the bottom.
        if row.position.y < min_y - 64.0 {
            self.hurt_general(id, self.general_hit("minecraft:out_of_world", 4.0));
            if !self.general_living_alive(id) {
                return heights;
            }
        }
        let Some(i) = self.general_living_index(id) else {
            return heights;
        };
        let row = &self.resident_entities.general.rows[i];
        let fire_immune = row.fire_immune();
        let bounds = {
            use super::super::entity::Entity;
            row.bounds()
        };
        let (water, _) = living_body::fluid_contacts(&self.world, self.res.facts(), bounds);
        let mut burn = false;
        self.resident_entities.general.rows.update(i, |row| {
            let Some(living) = row.living.as_deref_mut() else {
                return;
            };
            if water {
                living.fire_ticks = 0;
            } else if living.fire_ticks > 0 {
                // `Entity.baseTick`: a fire-immune body burns out four times as fast
                // and is never hurt by it.
                if fire_immune {
                    living.fire_ticks = (living.fire_ticks - 4).max(0);
                } else {
                    burn = living.fire_ticks % 20 == 0;
                    living.fire_ticks -= 1;
                }
            }
        });
        if burn {
            self.hurt_general(id, self.general_hit("minecraft:on_fire", 1.0));
        }
        // `LivingEntity.baseTick`: the wall, then the air.
        if self.general_living_in_wall(id) {
            self.hurt_general(id, self.general_hit("minecraft:in_wall", 1.0));
        }
        let Some(i) = self.general_living_index(id) else {
            return heights;
        };
        if self.resident_entities.general.rows[i]
            .living
            .as_ref()
            .is_none_or(|living| living.dead())
        {
            return heights;
        }
        let mut drown = false;
        self.resident_entities.general.rows.update(i, |row| {
            let Some(living) = row.living.as_deref_mut() else {
                return;
            };
            if eye_in_water {
                // `decreaseAirSupply`, then at -20 the air resets and it drowns.
                living.air -= 1;
                if living.air == -20 {
                    living.air = 0;
                    drown = true;
                }
            } else if living.air < MAX_AIR {
                living.air = (living.air + 4).min(MAX_AIR);
            }
        });
        if drown {
            self.hurt_general(id, self.general_hit("minecraft:drown", 2.0));
        }
        heights
    }

    /// The fluid push on the row at `index` and what it measured: the water and lava
    /// heights and whether the eyes are under water.
    fn general_living_fluids(&mut self, index: usize) -> (f64, f64, bool) {
        let row = &self.resident_entities.general.rows[index];
        let (start, eye) = (row.position, row.eye_y());
        let bounds = {
            use super::super::entity::Entity;
            row.bounds()
        };
        let mut velocity = row.velocity;
        let measured = horizon_client::entities::update_entity_fluids(
            bounds,
            start,
            eye,
            &mut velocity,
            &self.world,
            &self.res.motion().fluids,
            &self.loop_context,
        );
        if velocity != self.resident_entities.general.rows[index].velocity {
            self.resident_entities
                .general
                .rows
                .update(index, |row| row.velocity = velocity);
        }
        measured
    }

    /// `Entity.isInWall`: a suffocating block (a full collision box that is a full
    /// render cube, as the players' and the mobs' checks have it) in the box at the
    /// eyes as wide as 0.8 of the body.
    fn general_living_in_wall(&self, id: u32) -> bool {
        let Some(i) = self.general_living_index(id) else {
            return false;
        };
        let row = &self.resident_entities.general.rows[i];
        let bounds = {
            use super::super::entity::Entity;
            row.bounds()
        };
        let width =
            f64::from(((bounds.max_position().x - bounds.min_position().x) as f32) * 0.8_f32);
        let eye = row.eye_y();
        let (x, z) = (row.position.x, row.position.z);
        let lo = [x - width / 2.0, eye - 0.5e-6, z - width / 2.0];
        let hi = [x + width / 2.0, eye + 0.5e-6, z + width / 2.0];
        let facts = self.res.facts();
        (lo[0].floor() as i32..=hi[0].floor() as i32).any(|bx| {
            (lo[1].floor() as i32..=hi[1].floor() as i32).any(|by| {
                (lo[2].floor() as i32..=hi[2].floor() as i32).any(|bz| {
                    let f = facts.get(self.world.block(bx, by, bz));
                    f.full_collision && f.solid_render
                })
            })
        })
    }

    /// `LivingEntity.aiStep`'s movement for a body with no AI: the fluid push, then
    /// `travel` (gravity, collision with the blocks, the world border and the entities
    /// its `canCollideWith` accepts), the honey slide, the fall and its damage, the
    /// fire and lava of the blocks it is in, and `onInsideBlock` for each block of the
    /// move.
    fn general_living_travel(&mut self, id: u32, (water_height, lava_height): (f64, f64)) {
        let Some(i) = self.general_living_index(id) else {
            return;
        };
        let start = self.resident_entities.general.rows[i].position;
        let in_water = water_height > 0.0;
        let medium = LivingMedium::from_heights(water_height, lava_height);
        let obstacles = self.general_living_obstacles(id);
        let (world, facts, collision) = (&self.world, self.res.facts(), self.collision);
        let climbing = &self.res.motion().climbing;
        let border = self.border.geometry();
        let context = &self.loop_context;
        let mut landed = None;
        self.resident_entities.general.rows.update(i, |row| {
            let fall_before = row.fall_distance;
            let Some(mut body) = row.living_body() else {
                return;
            };
            let mut scene = horizon_client::collisions::SceneCollisions {
                world,
                binding: collision,
                subject: None,
            };
            if let Some(step) = body.travel(
                &mut scene,
                facts,
                climbing,
                in_water,
                Some(border),
                &obstacles,
                context,
            ) {
                body.settle(medium, step);
            }
            let (position, on_ground) = (*body.position, *body.on_ground);
            // Entity.checkFallDamage narrows each movement before adding to the double.
            if in_water {
                *body.fall_distance = 0.0;
            } else {
                *body.fall_distance += f64::from((start.y - position.y).max(0.0) as f32);
            }
            if on_ground {
                if *body.fall_distance > 0.0 {
                    landed = Some(*body.fall_distance);
                }
                *body.fall_distance = 0.0;
            }
            let _ = fall_before;
            body.honey_slide(world, facts, {
                let [lo, hi] = body.shape.local;
                0.4375 + (hi[0] - lo[0]) / 2.0
            });
        });
        if let Some(distance) = landed {
            self.general_living_fall(id, distance);
        }
        self.general_living_blocks(id, start);
    }

    /// `LivingEntity.causeFallDamage` after a landing from `distance`: past the safe
    /// three blocks, a block a heart, with the fall sound.
    fn general_living_fall(&mut self, id: u32, distance: f64) {
        // `calculateFallDamage`: the safe fall distance and the multiplier are the
        // living defaults, 3 and 1.
        let damage = ((distance - 3.0) as f32).ceil();
        if damage <= 0.0 {
            return;
        }
        let Some(i) = self.general_living_index(id) else {
            return;
        };
        let row = &self.resident_entities.general.rows[i];
        let at = [row.position.x, row.position.y, row.position.z];
        if let GeneralBody::Living(spec) = general_type_catalog().body(row.type_slot) {
            let sound = if damage > 4.0 {
                &spec.big_fall_sound
            } else {
                &spec.small_fall_sound
            };
            if let Some(sound) = sound {
                self.audio.play(sound, Some(at), 1.0, 1.0);
            }
        }
        self.hurt_general(id, self.general_hit("minecraft:fall", damage));
    }

    /// `Entity.applyEffectsFromBlocks` for the fire and lava of the blocks the row's
    /// box is in, then `onInsideBlock` (`general_inside_block`) for every block the
    /// move from `start` passed through, each once, `at_end` for those that meet the
    /// box where the move ended.
    fn general_living_blocks(&mut self, id: u32, start: Vec3) {
        let Some(i) = self.general_living_index(id) else {
            return;
        };
        let row = &self.resident_entities.general.rows[i];
        let end_box = {
            use super::super::entity::Entity;
            row.bounds()
        };
        let fire_immune = row.fire_immune();
        let (_, lava) = living_body::fluid_contacts(&self.world, self.res.facts(), end_box);
        let fires = living_body::fire_blocks(&self.world, self.pack.assets.registry, end_box);
        if fires > 0 {
            if !fire_immune {
                self.resident_entities.general.rows.update(i, |row| {
                    if let Some(living) = row.living.as_deref_mut() {
                        living_body::fire_ignite(&mut living.fire_ticks);
                    }
                });
            }
            // `BaseFireBlock.entityInside`: each fire block hurts, by 1.
            for _ in 0..fires {
                self.hurt_general(id, self.general_hit("minecraft:in_fire", 1.0));
                if !self.general_living_alive(id) {
                    return;
                }
            }
        }
        if lava {
            // `Entity.lavaIgnite`, then `lavaHurt`.
            if !fire_immune && let Some(i) = self.general_living_index(id) {
                self.resident_entities.general.rows.update(i, |row| {
                    if let Some(living) = row.living.as_deref_mut() {
                        living.fire_ticks = living.fire_ticks.max(300);
                    }
                });
            }
            self.hurt_general(id, self.general_hit("minecraft:lava", 4.0));
            if !self.general_living_alive(id) {
                return;
            }
        }
        self.general_inside_blocks(id, start, end_box);
    }

    /// `onInsideBlock` for each block the swept box from `start` to `end_box` meets, in
    /// `BlockPos.betweenClosed` order of the swept box, once each.
    fn general_inside_blocks(&mut self, id: u32, start: Vec3, end_box: Aabb) {
        let Some(i) = self.general_living_index(id) else {
            return;
        };
        let row = &self.resident_entities.general.rows[i];
        let kind = general_type_catalog().key(row.type_slot);
        if !general_moment_hooked(
            mod_api::HookId::GeneralInsideBlock,
            "general_inside_block",
            kind,
        ) {
            return;
        }
        let Some(epoch) = self.native_lifecycle.epoch() else {
            return;
        };
        let end = row.position;
        let delta = Vec3::new(start.x - end.x, start.y - end.y, start.z - end.z);
        let start_box = Aabb::new(
            end_box.min_position().add_vector(delta),
            end_box.max_position().add_vector(delta),
        );
        let deflate = Vec3::new(1e-5, 1e-5, 1e-5);
        let swept = start_box.union_bounds(end_box).deflate(deflate);
        let end_cells = end_box.deflate(deflate);
        let (lo, hi) = (swept.min_position(), swept.max_position());
        let (end_lo, end_hi) = (end_cells.min_position(), end_cells.max_position());
        let in_end = |p: f64, lo: f64, hi: f64| p >= lo.floor() && p <= hi.floor();
        let mut cells = Vec::new();
        for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
            for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
                for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
                    let state = self.world.block(x, y, z);
                    if self.res.facts().get(state).name.as_ref() == "minecraft:air" {
                        continue;
                    }
                    let at_end = in_end(f64::from(x), end_lo.x, end_hi.x)
                        && in_end(f64::from(y), end_lo.y, end_hi.y)
                        && in_end(f64::from(z), end_lo.z, end_hi.z);
                    cells.push((BlockPos::new(x, y, z), state, at_end));
                }
            }
        }
        let kind = kind.to_owned();
        for (pos, state, at_end) in cells {
            if !self.general_living_alive(id) {
                return;
            }
            self.native_general_inside_block(&kind, EntityRef::new(epoch, id), pos, state, at_end);
        }
    }

    /// `EntityGetter.getEntityCollisions` for a living row: the boxes of the entities it
    /// `canCollideWith` that meet its move. Vanilla's rule is the other entity's
    /// `canBeCollidedWith` (a boat, a living shulker, a happy ghast held still), and the
    /// type's `general_can_collide_with` may answer otherwise for any of them and for
    /// the other living rows.
    fn general_living_obstacles(&self, id: u32) -> Vec<Aabb> {
        let Some(i) = self.general_living_index(id) else {
            return Vec::new();
        };
        let row = &self.resident_entities.general.rows[i];
        let area = {
            use super::super::entity::Entity;
            row.bounds()
                .expand_towards(row.velocity)
                .inflate(Vec3::new(1.0e-7, 1.0e-7, 1.0e-7))
        };
        let kind = general_type_catalog().key(row.type_slot);
        let mut candidates: Vec<(Option<Target>, Aabb, bool)> = self
            .mover_collision_candidates()
            .into_iter()
            .filter(|(_, bounds)| bounds.intersects(area))
            .map(|(target, bounds)| (target, bounds, true))
            .collect();
        for other in self.resident_entities.general.rows.iter() {
            if other.id == id
                || other.living.is_none()
                || self.persist.removals.contains(&Target::General(other.id))
            {
                continue;
            }
            let bounds = {
                use super::super::entity::Entity;
                other.bounds()
            };
            if bounds.intersects(area) {
                candidates.push((Some(Target::General(other.id)), bounds, false));
            }
        }
        let asks = general_moment_hooked(
            mod_api::HookId::GeneralCanCollideWith,
            "general_can_collide_with",
            kind,
        );
        let epoch = self.native_lifecycle.epoch();
        candidates
            .into_iter()
            .filter(|&(target, _, vanilla)| {
                let (true, Some(epoch), Some(target)) = (asks, epoch, target) else {
                    return vanilla;
                };
                let Some(other) = self.network_entity_id(target) else {
                    return vanilla;
                };
                let mut collides = None;
                self.native_general_can_collide_with(
                    kind,
                    EntityRef::new(epoch, id),
                    EntityRef::new(epoch, other as u32),
                    &mut collides,
                );
                collides.unwrap_or(vanilla)
            })
            .map(|(_, bounds, _)| bounds)
            .collect()
    }

    /// `LivingEntity.pushEntities` of a living row, on its own turn: each pushable entity its
    /// box meets (a mob, the players, a server's players, another living row) gets
    /// `doPush(other)`, which is `other.push(this row)` (`Entity.push(Entity)`): the pair is
    /// pushed apart, both ways. The other entity's `push` decides, so this row's own
    /// `general_push` is not asked here but on the others' turns
    /// ([`Self::push_general_living_from_others`]); another living row's is. A server's
    /// players move only on their own clients, so their share is left out, as vanilla's
    /// server push never reaches them.
    fn general_living_push_entities(&mut self, id: u32) {
        let Some(i) = self.general_living_index(id) else {
            return;
        };
        let row = &self.resident_entities.general.rows[i];
        let bounds = {
            use super::super::entity::Entity;
            row.bounds()
        };
        let at = row.position;
        let mut targets: Vec<(Target, Vec3)> = Vec::new();
        for m in &self.mobs {
            if m.pushable() && m.bounds().intersects(bounds) {
                targets.push((Target::Mob(m.id), m.position));
            }
        }
        if let Some(p) = self.player.as_ref()
            && !self.spectator()
            && p.bounds().intersects(bounds)
        {
            targets.push((Target::Player, p.position));
        }
        if let Some(host) = self.lan.as_ref() {
            for g in host.hub.guests.iter().filter(|g| {
                host.hub.here(g)
                    && g.loaded
                    && !g.dead
                    && !g.spectator()
                    && g.bounds().intersects(bounds)
            }) {
                let [x, y, z] = g.position;
                targets.push((Target::Guest(g.conn), Vec3::new(x, y, z)));
            }
        }
        for other in self.resident_entities.general.rows.iter() {
            if other.id == id
                || other.living.as_ref().is_none_or(|living| living.dead())
                || self.persist.removals.contains(&Target::General(other.id))
            {
                continue;
            }
            let other_bounds = {
                use super::super::entity::Entity;
                other.bounds()
            };
            if other_bounds.intersects(bounds) {
                targets.push((Target::General(other.id), other.position));
            }
        }
        for (target, other) in targets {
            // `other.push(this)`: this row is pushed away from the other entity, at
            // most 0.05, and the other the opposite way.
            let Some((xa, za)) = push_apart(at, other) else {
                continue;
            };
            match target {
                Target::Mob(mob) => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                        m.velocity = Vec3::new(m.velocity.x + xa, m.velocity.y, m.velocity.z + za);
                    }
                }
                Target::Player => {
                    if let Some(p) = self.player.as_mut() {
                        p.velocity = Vec3::new(p.velocity.x + xa, p.velocity.y, p.velocity.z + za);
                    }
                }
                // Another living row's own `push(Entity)` may keep the pair apart.
                Target::General(other_id) => {
                    if !self.general_pushed_by(other_id, id) {
                        continue;
                    }
                    if let Some(j) = self.general_living_index(other_id) {
                        self.resident_entities.general.rows.update(j, |row| {
                            row.velocity =
                                Vec3::new(row.velocity.x + xa, row.velocity.y, row.velocity.z + za);
                        });
                    }
                }
                _ => {}
            }
            self.push_general_row(id, -xa, -za);
        }
    }

    /// The other half of `pushEntities` with a living row: each mob, the window's player and
    /// each of a server's players whose box meets a living row runs `doPush(row)` on its own
    /// turn, which is `row.push(it)`: the row's `general_push` (vanilla's yes) decides, then
    /// the row is pushed away from it and a mob back the other way. A player's share moves
    /// only the server's copy of it, which no player feels (as `push_carts`), so it is left
    /// out. It runs at the start of the rows' phase, after the mobs have moved.
    pub(in crate::game) fn push_general_living_from_others(&mut self) {
        let rows: Vec<(u32, Aabb, Vec3)> = self
            .resident_entities
            .general
            .rows
            .iter()
            .filter(|row| {
                row.mount.is_none()
                    && row.living.as_ref().is_some_and(|living| !living.dead())
                    && !self.persist.removals.contains(&Target::General(row.id))
            })
            .map(|row| {
                use super::super::entity::Entity;
                (row.id, row.bounds(), row.position)
            })
            .collect();
        if rows.is_empty() {
            return;
        }
        let teams = self.mob_teams();
        // (who, by network id, feet, box)
        let mut pushers: Vec<(Option<u32>, u32, Vec3, Aabb)> = self
            .mobs
            .iter()
            .filter(|m| {
                !m.removed
                    && m.alive()
                    && m.species() != horizon_client::mobs::Species::Bat
                    && horizon_client::scoreboard::pushable_by(
                        teams.of(horizon_client::mobs::Who::Mob(m.id)),
                        None,
                        true,
                    )
            })
            .map(|m| (Some(m.id), m.id, m.position, m.bounds()))
            .collect();
        drop(teams);
        if let Some(p) = self.player.as_ref()
            && !self.spectator()
        {
            let by = self.network_entity_id(Target::Player).unwrap_or(0) as u32;
            pushers.push((None, by, p.position, p.bounds()));
        }
        if let Some(host) = self.lan.as_ref() {
            for g in host.hub.guests.iter().filter(|g| {
                host.hub.here(g) && g.loaded && !g.dead && !g.spectator() && g.vehicle.is_none()
            }) {
                let [x, y, z] = g.position;
                pushers.push((None, g.entity_id as u32, Vec3::new(x, y, z), g.bounds()));
            }
        }
        for (mob, by, feet, bb) in pushers {
            for &(id, bounds, at) in &rows {
                if !bb.intersects(bounds) {
                    continue;
                }
                // A mob riding the row pushes it not (`isPassengerOfSameVehicle`).
                if mob.is_some_and(|m| {
                    self.mobs
                        .iter()
                        .find(|x| x.id == m)
                        .is_some_and(|x| x.extra.mount == Some(id))
                }) {
                    continue;
                }
                // `row.push(it)`: `xa` runs from the row to the pusher.
                let Some((xa, za)) = push_apart(at, feet) else {
                    continue;
                };
                if !self.general_pushed_by(id, by) {
                    continue;
                }
                self.push_general_row(id, -xa, -za);
                if let Some(m) = mob
                    && let Some(m) = self.mobs.iter_mut().find(|x| x.id == m)
                    && m.pushable()
                    && m.rider.is_none()
                {
                    m.velocity = Vec3::new(m.velocity.x + xa, m.velocity.y, m.velocity.z + za);
                }
            }
        }
    }

    /// `Entity.push(x, 0, z)` on living row `id`, when it carries no one (`isVehicle`).
    fn push_general_row(&mut self, id: u32, xa: f64, za: f64) {
        if self
            .resident_entities
            .general
            .rows
            .iter()
            .any(|row| row.mount == Some(id))
        {
            return;
        }
        if let Some(i) = self.general_living_index(id) {
            self.resident_entities.general.rows.update(i, |row| {
                row.velocity = Vec3::new(row.velocity.x + xa, row.velocity.y, row.velocity.z + za);
            });
        }
    }

    /// Whether living row `id` lets entity `by` push it (`Entity.push(Entity)`): its
    /// type's `general_push`, else vanilla's yes for a living body that is alive.
    pub(in crate::game) fn general_pushed_by(&mut self, id: u32, by: u32) -> bool {
        let Some(i) = self.general_living_index(id) else {
            return false;
        };
        let row = &self.resident_entities.general.rows[i];
        if row.living.as_ref().is_none_or(|living| living.dead()) {
            return false;
        }
        let kind = general_type_catalog().key(row.type_slot).to_owned();
        let Some(epoch) = self.native_lifecycle.epoch() else {
            return true;
        };
        if !general_moment_hooked(mod_api::HookId::GeneralPush, "general_push", &kind) {
            return true;
        }
        let mut pushed = None;
        self.native_general_push(
            &kind,
            EntityRef::new(epoch, id),
            EntityRef::new(epoch, by),
            &mut pushed,
        );
        pushed.unwrap_or(true)
    }

    fn general_hit(&self, damage_type: &'static str, amount: f32) -> GeneralHit<'static> {
        GeneralHit {
            damage_type,
            amount,
            direct: None,
            causing: None,
            from: None,
        }
    }

    /// `LivingEntity.hurtServer` of the living general row `id`: refused for a dead
    /// body and, for a fire-immune one, a fire hit; then the type's `general_hurt`
    /// may handle it; then vanilla's for an entity with no armor and no effects:
    /// `entity_invulnerability_check`, `living_incoming_damage`, the cooldown, the
    /// damage (`living_damage_pre`, `post`), the knockback from the source's position,
    /// the hurt or death sound and, at no health, its death. True when the hit was
    /// taken.
    pub(in crate::game) fn hurt_general(&mut self, id: u32, hit: GeneralHit<'_>) -> bool {
        let Some(i) = self.general_living_index(id) else {
            return false;
        };
        let row = &self.resident_entities.general.rows[i];
        if row.living.as_ref().is_none_or(|living| living.dead()) {
            return false;
        }
        let registry = &self.health_resources.damage;
        let Some(damage_kind) = registry.id(hit.damage_type) else {
            return false;
        };
        let is_fire = registry.in_tag(damage_kind, "minecraft:is_fire");
        let bypasses = registry.in_tag(damage_kind, "minecraft:bypasses_invulnerability");
        let bypasses_cooldown = registry.in_tag(damage_kind, "minecraft:bypasses_cooldown");
        let no_knockback = registry.in_tag(damage_kind, "minecraft:no_knockback");
        // `Entity.isInvulnerableToBase`: a fire-immune body and fire.
        if is_fire && row.fire_immune() && !bypasses {
            return false;
        }
        let kind = general_type_catalog().key(row.type_slot).to_owned();
        let position = row.position;
        let epoch = self.native_lifecycle.epoch();
        // The Original's `hurt` override.
        if let Some(epoch) = epoch
            && general_moment_hooked(mod_api::HookId::GeneralHurt, "general_hurt", &kind)
        {
            let causing = hit
                .causing
                .and_then(|t| self.network_entity_id(t))
                .map(|e| EntityRef::new(epoch, e as u32));
            let direct = hit
                .direct
                .and_then(|t| self.network_entity_id(t))
                .map(|e| EntityRef::new(epoch, e as u32));
            let mut taken = None;
            self.native_general_hurt(mod_api::__private::GeneralHurtCall {
                kind: &kind,
                entity: EntityRef::new(epoch, id),
                damage_type: hit.damage_type,
                amount: hit.amount,
                causing,
                direct,
                taken: &mut taken,
            });
            if let Some(taken) = taken {
                return taken;
            }
        }
        // The part may have removed or killed it.
        let Some(i) = self.general_living_index(id) else {
            return false;
        };
        if self.resident_entities.general.rows[i]
            .living
            .as_ref()
            .is_none_or(|living| living.dead())
        {
            return false;
        }
        let causing_id = hit
            .causing
            .and_then(|t| self.network_entity_id(t))
            .unwrap_or(0);
        let direct_id = hit
            .direct
            .and_then(|t| self.network_entity_id(t))
            .unwrap_or(0);
        let source = horizon_client::health::HurtSource {
            damage_type: hit.damage_type,
            causing: causing_id,
            direct: direct_id,
            position: hit.from,
        };
        let hurting = || horizon_client::health::Hurting::General(id as i32, kind.as_str(), source);
        let level = super::super::entity::damage_level_of!(self, scoped);
        if level.invulnerability_check(hurting(), false) {
            return false;
        }
        let mut container = horizon_client::health::DamageContainer::new(hit.amount);
        let victim = mod_api::entity::health::Victim {
            position: [position.x, position.y, position.z],
            view: [0.0, 0.0, 1.0],
        };
        if level.incoming_canceled(hurting(), victim, &mut container) {
            return true;
        }
        let mut damage = container.new_damage().max(0.0);
        if !damage.is_finite() {
            damage = f32::MAX;
        }
        let living = self.resident_entities.general.rows[i]
            .living
            .as_deref()
            .expect("checked living above")
            .clone();
        // `LivingEntity.hurtServer`: within the cooldown only what exceeds the last hit.
        let window = living.invulnerable_time > 10 && !bypasses_cooldown;
        container.record_should_cause_side_effects(!window);
        let mut dealt = if window {
            if damage <= living.last_hurt {
                return false;
            }
            container.reduce(
                horizon_client::health::Reduction::Invulnerability,
                damage,
                living.last_hurt,
                damage - living.last_hurt,
                true,
            )
        } else {
            container.set_post_attack_invulnerability_ticks(20);
            damage
        };
        if window {
            container.set_post_attack_invulnerability_ticks(living.invulnerable_time);
        }
        let level = super::super::entity::damage_level_of!(self, scoped);
        container.enter_pre(dealt);
        level.damage_pre(hurting(), &mut container);
        dealt = container.capture_inflicted();
        container.record_health_damage(dealt);
        let invulnerable = container.post_attack_invulnerability_ticks();
        let last_hurt = damage + container.pre_change();
        let mut died = false;
        self.resident_entities.general.rows.update(i, |row| {
            let Some(living) = row.living.as_deref_mut() else {
                return;
            };
            living.health = (living.health - dealt).max(0.0);
            living.last_hurt = last_hurt;
            living.invulnerable_time = invulnerable;
            if !window {
                living.hurt_time = 10;
            }
            died = living.dead();
        });
        // `dealDefaultKnockback` on a full hit with a source position.
        if !window
            && !no_knockback
            && let Some(from) = hit.from
        {
            self.general_knockback(id, 0.4, from[0] - position.x, from[2] - position.z);
        }
        let level = super::super::entity::damage_level_of!(self, scoped);
        level.damage_post(hurting(), &container);
        let at = [position.x, position.y, position.z];
        let slot = self.resident_entities.general.rows[i].type_slot;
        if let GeneralBody::Living(spec) = general_type_catalog().body(slot) {
            let sound = if died {
                &spec.death_sound
            } else {
                &spec.hurt_sound
            };
            if let Some(sound) = sound
                && !window
            {
                self.audio.play(sound, Some(at), 1.0, 1.0);
            }
        }
        if died {
            // `LivingEntity.die`: ENTITY_DIE by the body itself.
            self.entity_event_by("minecraft:entity_die", at, None);
        }
        true
    }

    /// `LivingEntity.knockback` of living row `id`, `strength` away from `(xd, zd)`
    /// (which points at the source). Its knockback resistance is the living default, 0.
    pub(in crate::game) fn general_knockback(&mut self, id: u32, strength: f64, xd: f64, zd: f64) {
        let Some(i) = self.general_living_index(id) else {
            return;
        };
        let len = (xd * xd + zd * zd).sqrt();
        if strength <= 0.0 || len < f64::from(1.0e-5_f32).sqrt() {
            return;
        }
        let (kx, kz) = (xd / len * strength, zd / len * strength);
        self.resident_entities.general.rows.update(i, |row| {
            let on_ground = row.living.as_ref().is_some_and(|living| living.on_ground);
            let v = row.velocity;
            let y = if on_ground {
                (v.y / 2.0 + strength).min(0.4)
            } else {
                v.y
            };
            row.velocity = Vec3::new(v.x / 2.0 - kx, y, v.z / 2.0 - kz);
        });
    }

    /// `LivingEntity.kill`: a living row is hurt by `generic_kill`, which passes its
    /// invulnerability and its cooldown, for all its health.
    pub(in crate::game) fn kill_general_living(&mut self, id: u32) -> bool {
        self.hurt_general(
            id,
            GeneralHit {
                damage_type: "minecraft:generic_kill",
                amount: f32::MAX,
                direct: None,
                causing: None,
                from: None,
            },
        )
    }

    /// Remove row `id` from its level, as `Entity.remove(KILLED)`.
    fn remove_general_row(&mut self, id: u32) {
        if let Some(epoch) = self.native_lifecycle.epoch() {
            let _ = self.entity_remove(EntityRef::new(epoch, id), mod_api::RemovalReason::Killed);
        }
    }

    /// `entity_set_fire_immune`: the living row's own fire immunity over its type's,
    /// saved with it. False for a stale handle or a row without a living body.
    pub(in crate::game) fn entity_set_fire_immune(
        &mut self,
        entity: EntityRef,
        fire_immune: bool,
    ) -> bool {
        let Some(Target::General(id)) = self.native_entity_target(entity) else {
            return false;
        };
        let Some(i) = self.general_living_index(id) else {
            return false;
        };
        self.resident_entities.general.rows.update(i, |row| {
            if let Some(living) = row.living.as_deref_mut() {
                living.fire_immune = Some(fire_immune);
                // `Entity.fireImmune` keeps a burning body from burning on.
                if fire_immune {
                    living.fire_ticks = living.fire_ticks.min(0);
                }
            }
        });
        true
    }
}

/// Whether any part attached to general type `kind` hears the moment `id` (named
/// `name`): one bit check when no part anywhere hears it.
pub(in crate::game) fn general_moment_hooked(
    id: mod_api::HookId,
    name: &'static str,
    kind: &str,
) -> bool {
    mod_api::__private::has_part_hook(id) && mod_api::__private::entity_part_hooked(id, name, kind)
}

/// `Entity.push(Entity)`'s impulse with `this` at `at` and the other entity at `other`: the
/// other's share `(xa, za)` (this one gets the opposite), at most 0.05 on an axis; none when
/// the two stand within 0.01 of each other.
fn push_apart(at: Vec3, other: Vec3) -> Option<(f64, f64)> {
    let (mut xa, mut za) = (other.x - at.x, other.z - at.z);
    let mut dd = xa.abs().max(za.abs());
    if dd < f64::from(0.01_f32) {
        return None;
    }
    dd = dd.sqrt();
    xa /= dd;
    za /= dd;
    let pow = (1.0 / dd).min(1.0);
    xa *= pow;
    za *= pow;
    Some((xa * f64::from(0.05_f32), za * f64::from(0.05_f32)))
}
