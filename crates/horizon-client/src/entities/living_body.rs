//! PURPOSE: vanilla `LivingEntity`'s movement without AI, shared by every entity that runs it:
//! the armor stand and a general entity type with a living body (`GeneralBody::Living`).
//! WHY: the stand's travel, shove, honey slide, settle and fire-contact scan were the only
//! port of `LivingEntity.travel` for an entity with no AI. A second copy for general rows
//! would drift from it, so both owners lend their own fields to one [`LivingBodyMut`] and
//! run these functions on it.
//! REFS: vanilla `LivingEntity.travel`, `travelInAir`, `travelInFluid`, `Entity.move`,
//! `Entity.checkSupportingBlock`, `HoneyBlock.entityInside`, `Entity.checkInsideBlocks`.
//! LOGS: none. READS-BY: `ArmorStand`, `game/entities/general/living.rs`.
//! ENDS-WHEN: every living family keeps its state in one struct.

use crate::blocks::{BlockFacts, Fluid};
use crate::collisions::SceneCollisions;
use crate::motion::block_friction;
use crate::world::World;
use horizon_core::geometry::{Aabb, Vec3, border::BorderGeometry};
use horizon_core::position::BlockPos;

/// The shape and flags a living body moves with, which its owner computes from its own state.
#[derive(Clone, Copy, Debug)]
pub struct BodyShape {
    /// The entity type's id, which the move gate and block friction are told
    /// (`minecraft:armor_stand`, a mod's `mymod:crate`).
    pub kind: &'static str,
    /// The box's corners relative to the position, in blocks: `[-w/2, 0, -w/2]` and
    /// `[w/2, h, w/2]` for an entity type's own dimensions (`EntityDimensions`), the
    /// position twice (zeros) for a marker stand's empty ones.
    pub local: [[f64; 3]; 2],
    /// `Entity.isNoGravity`: the body travels nowhere, its velocity stays.
    pub no_gravity: bool,
    /// `Entity.getGravity`.
    pub gravity: f64,
    /// Leather boots on its feet: it walks on powder snow
    /// (`PowderSnowBlock.canEntityWalkOnPowderSnow`).
    pub leather_boots: bool,
}

impl BodyShape {
    /// The box at `position` (`EntityDimensions.makeBoundingBox`).
    pub fn bounds_at(&self, position: Vec3) -> Aabb {
        let ([lo, hi], p) = (self.local, position);
        Aabb::new(
            Vec3::new(p.x + lo[0], p.y + lo[1], p.z + lo[2]),
            Vec3::new(p.x + hi[0], p.y + hi[1], p.z + hi[2]),
        )
    }
}

/// A living body's movement state, lent by its owner for one step: the owner keeps the
/// fields, these functions read and write them through here.
pub struct LivingBodyMut<'a> {
    /// The owner's network id: the move gate and block friction name it.
    pub id: u32,
    pub position: &'a mut Vec3,
    pub velocity: &'a mut Vec3,
    pub on_ground: &'a mut bool,
    /// `Entity.fallDistance`.
    pub fall_distance: &'a mut f64,
    /// `Entity.mainSupportingBlockPos` after the last move.
    pub support: &'a mut horizon_world::ground_support::GroundState,
    pub shape: BodyShape,
}

/// What a body's travel carries from [`travel`] to [`settle`].
#[derive(Clone, Copy)]
pub struct LivingStep {
    pub(crate) falling: bool,
    pub(crate) friction: f32,
}

/// The fluid a body travels in (`LivingEntity.travelInFluid`: water first,
/// then lava), from the heights `Entity::push_by_fluid` measured.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LivingMedium {
    Air,
    Water,
    /// `shallow`: `isInShallowFluid(LAVA)`, the lava is no deeper than
    /// `getFluidJumpThreshold` (0.4 for a body whose eyes are above 0.4).
    Lava {
        shallow: bool,
    },
}

impl LivingMedium {
    pub fn from_heights(water_height: f64, lava_height: f64) -> Self {
        if water_height > 0.0 {
            Self::Water
        } else if lava_height > 0.0 {
            Self::Lava {
                shallow: lava_height <= 0.4,
            }
        } else {
            Self::Air
        }
    }
}

/// Living entities share LivingEntity's block/tag and trapdoor climb predicate.
struct BodyClimbing<'a> {
    world: &'a World,
    position: BlockPos,
}

impl horizon_world::climbing::ClimbingWorld for BodyClimbing<'_> {
    type Error = std::convert::Infallible;
    fn is_spectator(&mut self) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn is_fall_flying(&mut self) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn block_position(&mut self) -> Result<BlockPos, Self::Error> {
        Ok(self.position)
    }
    fn in_block_state(&mut self) -> Result<horizon_world::registry::BlockStateId, Self::Error> {
        self.block_state(self.position)
    }
    fn block_state(
        &mut self,
        p: BlockPos,
    ) -> Result<horizon_world::registry::BlockStateId, Self::Error> {
        Ok(self.world.block(p.x, p.y, p.z))
    }
    fn set_last_climbable_position(&mut self, _: BlockPos) {}
}

impl LivingBodyMut<'_> {
    /// The box where the body stands now.
    pub fn bounds(&self) -> Aabb {
        self.shape.bounds_at(*self.position)
    }

    fn powder_snow_subject<'s>(&self) -> Option<crate::collisions::PowderSnowSubject<'s>> {
        self.shape
            .leather_boots
            .then_some(crate::collisions::PowderSnowSubject {
                leather_boots: true,
                ..Default::default()
            })
    }

    /// LivingEntity.travel for a body with no AI: clamp tiny velocity
    /// components (`LivingEntity.aiStep`), move by the velocity (a landing
    /// stops the fall, `Block.updateEntityMovementAfterFallOn`, and a wall
    /// collision zeros that horizontal axis), then hand [`Self::settle`] what
    /// it needs. `entities` are the boxes `EntityGetter.getEntityCollisions`
    /// gives the body, which stop its move as blocks do. None: a body with
    /// `NoGravity` keeps its velocity and does not travel.
    pub fn travel(
        &mut self,
        scene: &mut SceneCollisions,
        facts: &BlockFacts,
        climbing: &horizon_world::climbing::ClimbingRegistry<'_>,
        in_water: bool,
        border: Option<&BorderGeometry>,
        entities: &[Aabb],
        context: &horizon_world::native_load_context::LoopContext,
    ) -> Option<LivingStep> {
        scene.subject = self.powder_snow_subject();
        let v = *self.velocity;
        let mut wanted = Vec3::new(
            if v.x.abs() < 0.003 { 0.0 } else { v.x },
            if v.y.abs() < 0.003 { 0.0 } else { v.y },
            if v.z.abs() < 0.003 { 0.0 } else { v.z },
        );
        *self.velocity = wanted;
        if self.shape.no_gravity {
            return None;
        }
        // `travelInFluid` reads `isFalling` before the move and any push;
        // `travelInAir` reads ground friction before the move.
        let falling = wanted.y <= 0.0;
        let friction = if !in_water && *self.on_ground {
            let below =
                BlockPos::containing(self.position.subtract(Vec3::new(0.0, 0.500_001, 0.0)));
            block_friction(context, self.id, self.shape.kind, scene.world, facts, below)
        } else {
            1.0
        };
        let on_climbable = |position| {
            horizon_world::climbing::on_climbable(
                &mut BodyClimbing {
                    world: scene.world,
                    position: BlockPos::containing(position),
                },
                climbing,
            )
            .unwrap_or(false)
        };
        if !in_water && on_climbable(*self.position) {
            *self.fall_distance = 0.0;
            // LivingEntity.handleOnClimbable; suppressing slide is Player-only.
            let limit = f64::from(0.15f32);
            wanted.x = wanted.x.clamp(-limit, limit);
            wanted.z = wanted.z.clamp(-limit, limit);
            if !wanted.y.is_nan() {
                wanted.y = wanted.y.max(-limit);
            }
        }
        let mut velocity = wanted;
        let mut horizontal_collision = false;
        // Entity.move goes through the move gate (arismoko/horizon#2414); PR 1a
        // records the movement unchanged.
        let mut gate = crate::move_gate::armed(
            context,
            self.id,
            self.shape.kind,
            horizon_world::entity_move::MoverType::SelfMovement,
            wanted,
        );
        let wanted = gate.delta();
        // Entity.collide: the world border's walls and the entities that
        // `canBeCollidedWith` the body stop it too.
        let moved = crate::move_gate::scene_resolve_in_border_with(
            &mut gate,
            SceneCollisions {
                world: scene.world,
                binding: scene.binding,
                subject: scene.subject,
            },
            self.bounds(),
            wanted,
            0.0,
            *self.on_ground,
            border.map(|b| (b, [self.position.x, self.position.z])),
            entities,
        );
        if let Ok(moved) = moved {
            *self.position = Vec3::new(
                self.position.x + moved.x,
                self.position.y + moved.y,
                self.position.z + moved.z,
            );
            *self.on_ground = wanted.y < 0.0 && moved.y > wanted.y;
            // Entity.setOnGroundWithMovement: the block under the feet.
            self.ground_support(scene, wanted);
            if (wanted.x - moved.x).abs() >= f64::from(1.0e-5_f32) {
                horizontal_collision = true;
                velocity.x = 0.0;
            }
            if moved.y != wanted.y {
                velocity.y = 0.0;
            }
            if (wanted.z - moved.z).abs() >= f64::from(1.0e-5_f32) {
                horizontal_collision = true;
                velocity.z = 0.0;
            }
        }
        if !in_water && horizontal_collision && on_climbable(*self.position) {
            velocity.y = 0.2;
        }
        *self.velocity = velocity;
        Some(LivingStep { falling, friction })
    }

    /// `Entity.move(mover, delta)` by something else (a shulker lid): blocks,
    /// the world border and `entities` cut the move short, a move cut to
    /// almost nothing is not made, and an axis that hits something loses its
    /// speed (`restituteMovementAfterCollisions`: no bounce, so the speed
    /// becomes `-speed * 0`). `travels_after`: the body's own travel follows
    /// this push in the same tick (see `ArmorStand::shove`).
    pub fn shove(
        &mut self,
        scene: SceneCollisions,
        delta: Vec3,
        border: Option<&BorderGeometry>,
        entities: &[Aabb],
        travels_after: bool,
        mover: horizon_world::entity_move::MoverType,
        context: &horizon_world::native_load_context::LoopContext,
    ) {
        let mut scene = SceneCollisions {
            subject: self.powder_snow_subject(),
            ..scene
        };
        // `Entity.move(mover, delta)` goes through the move gate
        // (arismoko/horizon#2414); PR 1a records the movement unchanged.
        let mut gate = crate::move_gate::armed(context, self.id, self.shape.kind, mover, delta);
        let delta = gate.delta();
        let Ok(moved) = crate::move_gate::scene_resolve_in_border_with(
            &mut gate,
            SceneCollisions {
                world: scene.world,
                binding: scene.binding,
                subject: scene.subject,
            },
            self.bounds(),
            delta,
            0.0,
            *self.on_ground,
            border.map(|b| (b, [self.position.x, self.position.z])),
            entities,
        ) else {
            return;
        };
        let length = moved.length_squared();
        if length > 1.0e-7 || delta.length_squared() - length < 1.0e-7 {
            *self.position = self.position.add_vector(moved);
        }
        // Mth.equal on x and z; y only when the push had a vertical part.
        if (delta.x - moved.x).abs() >= f64::from(1.0e-5_f32) {
            self.velocity.x = -self.velocity.x * 0.0;
        }
        if (delta.z - moved.z).abs() >= f64::from(1.0e-5_f32) {
            self.velocity.z = -self.velocity.z * 0.0;
        }
        let vertical = moved.y != delta.y;
        if travels_after {
            if vertical {
                *self.on_ground = false;
                if delta.y > 0.0 {
                    self.velocity.y = -self.velocity.y * 0.0;
                }
                self.ground_support(&mut scene, moved);
            }
        } else {
            if vertical && delta.y != 0.0 {
                self.velocity.y = -self.velocity.y * 0.0;
            }
            *self.on_ground = vertical && delta.y < 0.0;
            self.ground_support(&mut scene, moved);
        }
    }

    /// `Entity.checkSupportingBlock` after a move by `movement`: the block
    /// under the feet while `on_ground`, none otherwise.
    fn ground_support(&mut self, scene: &mut SceneCollisions, movement: Vec3) {
        let context = horizon_world::collision_context::CollisionContext::for_entity(
            horizon_world::collision_context::CollisionEntity {
                identity: horizon_world::entity_obstruction::EntityIdentity(u64::from(self.id)),
                kind: horizon_world::collision_context::EntityContextKind::Living,
                y: self.position.y,
                descending: false,
                main_hand: Some(horizon_world::collision_context::CollisionItem::EMPTY),
            },
            false,
        );
        let (position, bounds) = (*self.position, self.bounds());
        let _ = self
            .support
            .check_support(*self.on_ground, bounds, Some(movement), |area| {
                horizon_world::ground_support::find_supporting_block(
                    scene, context, position, area, 1_000_000,
                )
            });
    }

    /// `HoneyBlock.entityInside`: side-slide movement after the body's travel,
    /// for a body that is affected by blocks. `overlap` is the slide's reach
    /// from a honey cell's centre (`0.4375 + getBbWidth() / 2`).
    pub fn honey_slide(&mut self, world: &World, facts: &BlockFacts, overlap: f64) {
        let old_delta_y = |dy: f64| dy / f64::from(0.98_f32) + 0.08;
        if *self.on_ground || old_delta_y(self.velocity.y) >= -0.08 {
            return;
        }
        // Entity.checkInsideBlocks: test the deflated body against block cells,
        // not their inset collision shape (the body slides beside that shape).
        let bounds = self.bounds().deflate(Vec3::new(1e-5, 1e-5, 1e-5));
        let (lo, hi) = (bounds.min_position(), bounds.max_position());
        for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
            for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
                for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                    if &*facts.get(world.block(x, y, z)).name != "minecraft:honey_block"
                        || self.position.y > f64::from(y) + 0.9375 - 1.0e-7
                    {
                        continue;
                    }
                    let dx = (f64::from(x) + 0.5 - self.position.x).abs();
                    let dz = (f64::from(z) + 0.5 - self.position.z).abs();
                    if dx + 1.0e-7 <= overlap && dz + 1.0e-7 <= overlap {
                        continue;
                    }
                    // HoneyBlock.doSlideMovement, including its float-widened
                    // getOldDeltaY/getNewDeltaY conversion around air travel.
                    let old = old_delta_y(self.velocity.y);
                    if old < -0.13 {
                        let factor = -0.05 / old;
                        self.velocity.x *= factor;
                        self.velocity.z *= factor;
                    }
                    self.velocity.y = (-0.05 - 0.08) * f64::from(0.98_f32);
                    *self.fall_distance = 0.0;
                    // The new oldDeltaY is -0.05, so another honey cell cannot slide again.
                    return;
                }
            }
        }
    }

    /// The rest of the travel: the water slowdown, the lava slowdown and
    /// sink, or the air drag and gravity. The blocks' push (a bubble column)
    /// comes after this, as `LivingEntity.aiStep` applies the block effects
    /// after `travel`.
    pub fn settle(&mut self, medium: LivingMedium, step: LivingStep) {
        let LivingStep { falling, friction } = step;
        let velocity = *self.velocity;
        let gravity = self.shape.gravity;
        *self.velocity = if let LivingMedium::Lava { shallow } = medium {
            // travelInLava: halve it (a shallow lava also slows the fall by
            // 0.8 with `getFluidFallingAdjustedMovement`), then sink by
            // gravity / 4.
            let (x, y, z) = if shallow {
                let y = velocity.y * f64::from(0.8f32);
                let sink = gravity / 16.0;
                let adjusted = if falling && (y - 0.005).abs() >= 0.003 && (y - sink).abs() < 0.003
                {
                    -0.003
                } else {
                    y - sink
                };
                (velocity.x * 0.5, adjusted, velocity.z * 0.5)
            } else {
                (velocity.x * 0.5, velocity.y * 0.5, velocity.z * 0.5)
            };
            Vec3::new(x, y - gravity / 4.0, z)
        } else if medium == LivingMedium::Water {
            // travelInWater: slowDown 0.8, then `getFluidFallingAdjustedMovement`
            // with gravity / 16 (the body is falling while its fall is not
            // already nearly stopped).
            let slow = f64::from(0.8f32);
            let y = velocity.y * slow;
            let sink = gravity / 16.0;
            let adjusted = if falling && (y - 0.005).abs() >= 0.003 && (y - sink).abs() < 0.003 {
                -0.003
            } else {
                y - sink
            };
            Vec3::new(velocity.x * slow, adjusted, velocity.z * slow)
        } else {
            let drag = f64::from(friction * 0.91f32);
            Vec3::new(
                velocity.x * drag,
                (velocity.y - gravity) * f64::from(0.98f32),
                velocity.z * drag,
            )
        };
    }
}

/// Whether water and lava meet the box `bounds`, which a living body's fire
/// rules read (`Entity.baseTick`: water puts it out, `LAVA_IGNITE`).
pub fn fluid_contacts(world: &World, facts: &BlockFacts, bounds: Aabb) -> (bool, bool) {
    let (lo, hi) = (bounds.min_position(), bounds.max_position());
    let (mut water, mut lava) = (false, false);
    for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
        for y in lo.y.floor() as i32..hi.y.ceil() as i32 {
            for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                match facts.get(world.block(x, y, z)).fluid {
                    Fluid::Water => water = true,
                    Fluid::Lava => lava = true,
                    _ => {}
                }
            }
        }
    }
    (water, lava)
}

/// The fire blocks the box `bounds` (less 1e-5) is in, each visited once by
/// `Entity.checkInsideBlocks`, for a body that is affected by blocks.
pub fn fire_blocks(
    world: &World,
    registry: &horizon_world::registry::BlockRegistry,
    bounds: Aabb,
) -> u32 {
    let b = bounds.deflate(Vec3::new(1e-5, 1e-5, 1e-5));
    let (lo, hi) = (b.min_position(), b.max_position());
    let mut n = 0;
    for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
        for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
            for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                let name = registry
                    .state(world.block(x, y, z))
                    .map(|s| s.name.as_str());
                if matches!(name, Some("minecraft:fire" | "minecraft:soul_fire")) {
                    n += 1;
                }
            }
        }
    }
    n
}

/// `BaseFireBlock.fireIgnite` (`FIRE_IGNITE`), once a tick however many fire
/// blocks: a negative `Fire` counts up by one; from 0 the body is lit for 8
/// seconds, every tick it stays in the fire.
pub fn fire_ignite(fire_ticks: &mut i32) {
    if *fire_ticks < 0 {
        *fire_ticks += 1;
    }
    if *fire_ticks >= 0 {
        *fire_ticks = (*fire_ticks).max(160);
    }
}
