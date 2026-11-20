//! Owned drawing data for a screen scene, separate from live world entities.

use crate::{BlockState, Compound, Identifier, item::containers::ModBlockEntityFields};

/// A mod block entity's type and public fields supplied to a screen's drawing Part.
/// This is not a live entity: internal cache and revision identities never cross the API.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct SceneBlockEntity {
    /// The immutable type id of the temporary drawing entity.
    pub id: Identifier,
    /// Saved fields, item slots and menu data, in their existing public form.
    pub fields: ModBlockEntityFields,
}

impl SceneBlockEntity {
    /// Describe an entity to draw without retaining a world entity or its cache identity.
    pub fn new(id: Identifier, fields: ModBlockEntityFields) -> Self {
        Self { id, fields }
    }
}

/// One block of a scene a mod draws in a screen: its local position and state, the
/// optional mod block entity drawn by its Part as in the world, the section it belongs to,
/// and how far it is broken.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct SceneBlock {
    /// Position in the scene, independent of the world's blocks.
    pub pos: [i32; 3],
    /// Owned block state, including properties, whose baked model is drawn.
    /// An invalid or unavailable state refuses the entire scene before drawing.
    pub state: BlockState,
    /// Optional owned block entity drawing data.
    pub entity: Option<SceneBlockEntity>,
    /// The index in `Scene::sections` of the section the block belongs to: it moves, turns
    /// and is lit with it, and its faces are culled against that section's blocks only.
    /// Ignored in a scene with no sections.
    pub section: u32,
    /// How far the block is broken: destroy stage 0 to 9, drawn over its model as the world
    /// draws a block being mined. Past 9 draws no overlay.
    pub breaking: Option<u8>,
    /// The saved data of the game's own block entity at this block (a sign's text, a
    /// banner's patterns, a lectern's book), as a world loads it. A block whose state has a
    /// game block entity with a renderer (a chest, a sign, a banner, a skull, a bell…) is
    /// drawn by that renderer as in the world, with this data, or with the type's defaults
    /// without it. Nothing in a scene ticks, so a chest stays shut. Ignored when `entity`
    /// is set.
    pub game_data: Option<Compound>,
}

impl SceneBlock {
    /// Describe one scene-local block and optional entity, in section 0, unbroken.
    pub fn new(pos: [i32; 3], state: BlockState, entity: Option<SceneBlockEntity>) -> Self {
        Self {
            pos,
            state,
            entity,
            section: 0,
            breaking: None,
            game_data: None,
        }
    }
    /// The same block with its game block entity's saved data.
    pub fn with_game_data(mut self, data: Compound) -> Self {
        self.game_data = Some(data);
        self
    }
    /// The same block in section `section`.
    pub fn in_section(mut self, section: u32) -> Self {
        self.section = section;
        self
    }
    /// The same block broken to destroy stage `stage` (0 to 9).
    pub fn breaking(mut self, stage: u8) -> Self {
        self.breaking = Some(stage);
        self
    }
}

/// Light packed as `BlockEntityDrawContext::light` packs it, full bright: 16 times sky
/// light 15 in bits 0..8 and 16 times block light 15 in bits 8..16.
pub const SCENE_FULL_BRIGHT: u32 = 240 | 240 << 8;

/// A part of a scene that moves, turns and is lit as one: what Ponder calls a world
/// section. Every block, block entity and quad in it is drawn through `transform` and
/// lit by `light`, in the same depth as the rest of the scene, so sections hide each
/// other where they cross.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct SceneSection {
    /// A column-major affine 4 by 4 matrix, in scene blocks, applied to everything in the
    /// section before the view. The bottom row must be `0, 0, 0, 1`.
    pub transform: [f32; 16],
    /// The light everything in the section is drawn with, packed as [`SCENE_FULL_BRIGHT`]
    /// is: through the client's lightmap, as world blocks are lit.
    pub light: u32,
    /// Whether the section is drawn. A section that isn't still stands in the scene's
    /// level, so particles land on its blocks.
    pub drawn: bool,
}

impl SceneSection {
    /// A drawn section, in place and full bright.
    pub fn new() -> Self {
        Self {
            transform: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            light: SCENE_FULL_BRIGHT,
            drawn: true,
        }
    }
    /// The same section through `transform` (column major).
    pub fn transformed(mut self, transform: [f32; 16]) -> Self {
        self.transform = transform;
        self
    }
    /// The same section lit by block light `block` and sky light `sky`, each 0 to 15.
    pub fn lit(mut self, block: u8, sky: u8) -> Self {
        self.light = (u32::from(sky.min(15)) * 16) | ((u32::from(block.min(15)) * 16) << 8);
        self
    }
    /// The same section drawn or not.
    pub fn drawn(mut self, drawn: bool) -> Self {
        self.drawn = drawn;
        self
    }
}

impl Default for SceneSection {
    fn default() -> Self {
        Self::new()
    }
}

/// How a scene's faces are shaded.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub enum SceneShading {
    /// By the face's direction in the scene, as world blocks are: 1 on top, 0.5 underneath,
    /// 0.8 north and south, 0.6 east and west.
    Faces,
    /// By two lights fixed to the view, as the game lights models in its GUI:
    /// `min(1, (max(0, l0·n) + max(0, l1·n)) · 0.6 + 0.4)`, with `n` the face's normal after
    /// its section's transform and the view, in the view's frame (x right, y up, z toward
    /// the viewer). A face its model marks as not shaded takes the value for `n` straight up.
    View {
        /// The two lights' directions, in the view's frame.
        lights: [[f32; 3]; 2],
    },
}

/// A quad drawn in a scene's own depth, among its blocks: an outline's edge or face, a
/// shadow under the scene, a flash over it. Blocks in front of it hide it.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct SceneQuad {
    /// The corners in scene blocks, in a model face's order (top left, bottom left, bottom
    /// right, top right as seen from the front).
    pub corners: [[f32; 3]; 4],
    /// Each corner's colour, ARGB; the quad's colour runs between them.
    pub colors: [u32; 4],
    /// The GUI sprite the quad shows (as `DrawContext::sprite` names one), or none for
    /// plain colour.
    pub sprite: Option<String>,
    /// Each corner's place on the sprite, 0 to 1. Ignored without a sprite.
    pub uv: [[f32; 2]; 4],
    /// The section whose transform and light the quad follows; none for the scene's own
    /// frame, full bright.
    pub section: Option<u32>,
    /// Whether the scene's shading darkens it by its facing, as it does a block's face.
    pub shade: bool,
    /// Whether it is left out when it faces away from the viewer.
    pub cull: bool,
}

impl SceneQuad {
    /// A quad of plain colour `argb` with `corners`, in the scene's own frame, unshaded and
    /// seen from both sides.
    pub fn new(corners: [[f32; 3]; 4], argb: u32) -> Self {
        Self {
            corners,
            colors: [argb; 4],
            sprite: None,
            uv: [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
            section: None,
            shade: false,
            cull: false,
        }
    }
    /// The same quad with a colour per corner.
    pub fn colored(mut self, colors: [u32; 4]) -> Self {
        self.colors = colors;
        self
    }
    /// The same quad showing `sprite` at `uv`.
    pub fn sprite(mut self, sprite: impl Into<String>, uv: [[f32; 2]; 4]) -> Self {
        self.sprite = Some(sprite.into());
        self.uv = uv;
        self
    }
    /// The same quad in section `section`.
    pub fn in_section(mut self, section: u32) -> Self {
        self.section = Some(section);
        self
    }
    /// The same quad shaded by the scene's shading, or not.
    pub fn shaded(mut self, shade: bool) -> Self {
        self.shade = shade;
        self
    }
    /// The same quad culled when it faces away, or not.
    pub fn culled(mut self, cull: bool) -> Self {
        self.cull = cull;
        self
    }
}

/// How a scene is seen: its center, vertical rotation and top tilt in degrees, and
/// the number of GUI pixels a block spans.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct SceneView {
    /// Scene point placed at the GUI rectangle's center, in blocks.
    pub center: [f32; 3],
    /// Counterclockwise rotation seen from above, in degrees.
    pub yaw: f32,
    /// Tilt of the scene's top toward the viewer, in degrees.
    pub pitch: f32,
    /// GUI pixels per block; a nonpositive or nonfinite scale draws nothing.
    pub scale: f32,
}

impl SceneView {
    /// Set the scene's camera using the same units as its public fields.
    pub fn new(center: [f32; 3], yaw: f32, pitch: f32, scale: f32) -> Self {
        Self {
            center,
            yaw,
            pitch,
            scale,
        }
    }
}

/// An entity standing in a scene: the same description `DrawContext::entity` draws (a
/// new living entity of a type, with optional saved data, or an entity of the client's
/// level), prepared and drawn by the same path (its type's renderer and the mods'
/// layers, full bright, made for drawing only), placed at `pos` in the scene's own
/// coordinates and turned as the fields say.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct SceneEntity {
    /// Which entity, as `DrawContext::entity` takes it.
    pub entity: crate::GuiEntity,
    /// Its feet, in the scene's blocks (a block at `[0, 0, 0]` spans 0 to 1).
    pub pos: [f64; 3],
    /// Body yaw in degrees, as an entity's: 0 faces south (+z), 90 west.
    pub yaw: f32,
    /// Head yaw in degrees, in the same frame as `yaw`.
    pub head_yaw: f32,
    /// Head pitch in degrees: positive looks down.
    pub head_pitch: f32,
}

impl SceneEntity {
    /// `entity` at `pos`, facing south with its head straight.
    pub fn new(entity: crate::GuiEntity, pos: [f64; 3]) -> Self {
        Self {
            entity,
            pos,
            yaw: 0.0,
            head_yaw: 0.0,
            head_pitch: 0.0,
        }
    }
    /// The same entity turned: body yaw, head yaw and head pitch in degrees.
    pub fn turned(mut self, yaw: f32, head_yaw: f32, head_pitch: f32) -> Self {
        (self.yaw, self.head_yaw, self.head_pitch) = (yaw, head_yaw, head_pitch);
        self
    }
}

/// A particle a mod adds to a scene, as `ClientLevel.addParticle` adds one to a level, at
/// the scene's coordinates. It is added when the scene's clock next advances.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct SceneParticle {
    /// The particle and its parameters, as `add_particle` takes them.
    pub options: crate::ParticleOptions,
    /// Where it starts, in the scene's blocks.
    pub pos: [f64; 3],
    /// Its velocity in blocks a tick (or the provider's own input, as `add_particle`'s).
    pub velocity: [f64; 3],
}

impl SceneParticle {
    /// Particle `options` at `pos` moving at `velocity`.
    pub fn new(options: crate::ParticleOptions, pos: [f64; 3], velocity: [f64; 3]) -> Self {
        Self {
            options,
            pos,
            velocity,
        }
    }
}

/// A scene's own clock. The scene keeps its live particles from one draw to the next
/// under `id` and the calling mod (one id per scene a mod's screens show; two mods'
/// id 1 are two scenes). When `tick` is ahead of the tick the scene last ran, it runs
/// the ticks in between: the blocks' animate ticks when `animate_blocks` is set
/// ([`SCENE_ANIMATE_PICKS`] picks a tick among the scene's own blocks), then the
/// particles'. A jump of more than [`SCENE_CATCH_UP`] ticks (a page resumed) starts the
/// scene over from no particles and runs only the last [`SCENE_CATCH_UP`] ticks. The
/// same tick again is a paused scene: nothing is added and nothing moves. A tick behind
/// the last one, or a scene whose blocks or entities changed, starts the scene over with
/// no particles. Each tick's randomness is seeded from `id` and the tick, so the same
/// ticks give the same picture.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct SceneClock {
    /// Which of the calling mod's scenes this is; a screen showing two gives them two ids.
    pub id: u32,
    /// The scene's tick: the mod's own count, 20 a second for a scene that keeps time.
    pub tick: u64,
    /// How far into the next tick this frame is, 0 to 1, for smooth motion.
    pub partial: f32,
    /// Whether the blocks run their animate ticks (`Block.animateTick`: flames,
    /// smoke, drips) as the client runs them for blocks around the player.
    pub animate_blocks: bool,
}

impl SceneClock {
    /// Scene `id` at `tick`, `partial` into the next tick, with the blocks' animate ticks.
    pub fn new(id: u32, tick: u64, partial: f32) -> Self {
        Self {
            id,
            tick,
            partial,
            animate_blocks: true,
        }
    }
    /// The same clock with the blocks' animate ticks on or off.
    pub fn animate_blocks(mut self, on: bool) -> Self {
        self.animate_blocks = on;
        self
    }
}

/// The most ticks one draw runs to catch a scene's clock up; a longer jump starts over.
pub const SCENE_CATCH_UP: u64 = 20;
/// The animate-tick picks a tick makes among a scene's own blocks (vanilla makes 667
/// pairs around the player for a 16-block radius; a scene is a few blocks).
pub const SCENE_ANIMATE_PICKS: u32 = 64;
/// The most blocks one scene takes ([`SceneError::TooManyBlocks`]).
pub const SCENE_MAX_BLOCKS: usize = 4096;
/// The most entities one scene takes ([`SceneError::TooManyEntities`]).
pub const SCENE_MAX_ENTITIES: usize = 16;
/// The most live particles one scene holds, the blocks' own emissions and the mod's
/// together: past it no new particle is added, and more than this many to add at once
/// is [`SceneError::TooManyParticles`].
pub const SCENE_MAX_PARTICLES: usize = 512;
/// The most quads one scene takes ([`SceneError::TooManyQuads`]).
pub const SCENE_MAX_QUADS: usize = 16_384;
/// The most sections one scene takes ([`SceneError::BadSection`]).
pub const SCENE_MAX_SECTIONS: usize = 4096;
/// The most scenes with entities or particles that animate in one frame
/// ([`SceneError::TooManyScenes`]).
pub const SCENE_MAX_ANIMATING: usize = 4;

/// A scene `DrawContext::draw_scene` draws: its blocks, and optionally the entities
/// standing in it, the particles the mod adds and the clock that ticks them. A scene of
/// blocks alone (no entities, no particles, and no clock or one with `animate_blocks`
/// off) keeps no state and reads no clock. Keep one and change its clock each frame.
///
/// Its blocks and quads may be split into `sections` that move, turn, fade and hide as
/// units, all in one depth. A scene with no sections is one section in place, full
/// bright: as scenes were drawn before sections. `shading` and `biome` say how faces are
/// shaded and which biome's colours tint grass, foliage and water.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct Scene {
    /// The blocks.
    pub blocks: Vec<SceneBlock>,
    /// Entities standing in the scene; they need a clock.
    pub entities: Vec<SceneEntity>,
    /// Particles to add at the clock's next tick; they need a clock.
    pub particles: Vec<SceneParticle>,
    /// The scene's clock, for a scene that lives.
    pub clock: Option<SceneClock>,
    /// The scene's sections; blocks and quads name theirs by index. Empty: one section in
    /// place, full bright.
    pub sections: Vec<SceneSection>,
    /// Quads drawn in the scene's depth: outlines, shadows.
    pub quads: Vec<SceneQuad>,
    /// How faces are shaded.
    pub shading: SceneShading,
    /// The biome whose colours tint the scene's grass, foliage and water (`minecraft:plains`);
    /// none for a fixed default grass colour.
    pub biome: Option<Identifier>,
}

impl Scene {
    /// A scene of `blocks` alone: no entities, particles or clock.
    pub fn new(blocks: Vec<SceneBlock>) -> Self {
        Self {
            blocks,
            entities: Vec::new(),
            particles: Vec::new(),
            clock: None,
            sections: Vec::new(),
            quads: Vec::new(),
            shading: SceneShading::Faces,
            biome: None,
        }
    }
    /// The same scene split into `sections`.
    pub fn with_sections(mut self, sections: Vec<SceneSection>) -> Self {
        self.sections = sections;
        self
    }
    /// The same scene with `quads` in its depth.
    pub fn with_quads(mut self, quads: Vec<SceneQuad>) -> Self {
        self.quads = quads;
        self
    }
    /// The same scene shaded by `shading`.
    pub fn with_shading(mut self, shading: SceneShading) -> Self {
        self.shading = shading;
        self
    }
    /// The same scene tinted by `biome`'s colours.
    pub fn with_biome(mut self, biome: Identifier) -> Self {
        self.biome = Some(biome);
        self
    }
    /// The same scene on `clock`.
    pub fn with_clock(mut self, clock: SceneClock) -> Self {
        self.clock = Some(clock);
        self
    }
    /// The same scene with `entities` standing in it.
    pub fn with_entities(mut self, entities: Vec<SceneEntity>) -> Self {
        self.entities = entities;
        self
    }
    /// The same scene with `particles` to add.
    pub fn with_particles(mut self, particles: Vec<SceneParticle>) -> Self {
        self.particles = particles;
        self
    }
}

/// Why `draw_scene` refused a scene. Nothing of a refused scene is drawn, except for
/// [`TooManyScenes`](Self::TooManyScenes), which draws the blocks and the scene's last
/// state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub enum SceneError {
    /// Not a client's drawing (a server, or no game window).
    Unavailable,
    /// The view or box is not finite, its scale isn't above 0, or the scene seen through
    /// it would not stay in finite GUI coordinates.
    BadView,
    /// More than [`SCENE_MAX_BLOCKS`] blocks.
    TooManyBlocks,
    /// More than [`SCENE_MAX_ENTITIES`] entities.
    TooManyEntities,
    /// More than [`SCENE_MAX_PARTICLES`] particles to add at once.
    TooManyParticles,
    /// [`SCENE_MAX_ANIMATING`] other scenes already animated this frame: this one drew its
    /// blocks and its last state, and its clock did not advance.
    TooManyScenes,
    /// A block state this game doesn't have.
    BadBlock {
        /// The block's index in `blocks`.
        index: u32,
    },
    /// No living entity type has the entity's id, as `DrawContext::entity` refuses it.
    UnknownType {
        /// The entity's index in `entities`.
        index: u32,
    },
    /// The entity's data names another type or doesn't load, as `DrawContext::entity`
    /// refuses it.
    BadData {
        /// The entity's index in `entities`.
        index: u32,
    },
    /// The entity's handle names no living entity of the client's level, as
    /// `DrawContext::entity` refuses it.
    NoEntity {
        /// The entity's index in `entities`.
        index: u32,
    },
    /// An entity's position or turn isn't finite, or lies more than 30 million blocks out.
    BadEntity {
        /// The entity's index in `entities`.
        index: u32,
    },
    /// A particle's options, position or velocity aren't finite, or name a stack.
    BadParticle {
        /// The particle's index in `particles`.
        index: u32,
    },
    /// The clock's `partial` isn't finite.
    BadClock,
    /// Entities or particles without a clock.
    NoClock,
    /// A section's transform isn't finite or isn't affine, more than
    /// [`SCENE_MAX_SECTIONS`] sections, or a block or quad names a section the scene
    /// doesn't have.
    BadSection {
        /// The section's index: the one at fault, or the one named.
        index: u32,
    },
    /// More than [`SCENE_MAX_QUADS`] quads.
    TooManyQuads,
    /// A quad's corners or UVs aren't finite, or it names a sprite the GUI atlas doesn't
    /// have.
    BadQuad {
        /// The quad's index in `quads`.
        index: u32,
    },
    /// A biome this game doesn't have.
    BadBiome,
}
