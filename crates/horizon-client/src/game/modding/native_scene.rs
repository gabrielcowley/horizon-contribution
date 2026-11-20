//! A few blocks drawn in a screen as a small scene seen from an angle: what
//! Create's Ponder shows its machines in (docs/native-mods.md, "Scenes in
//! screens"). The blocks' models and their mod block entities' drawing are
//! projected into the GUI with their own depth, as the smithing screen draws
//! its armour stand.
use super::Game;
use super::native_block_entity_draw::{BlockEntityDraw, place_turning};
use glam::{Mat3, Mat4, Vec3};
use horizon_client::{
    block_entity_render::diffuse_lights, containers::BlockEntity, hud::HudBatch, mesher::Vertex,
};
use horizon_world::level::data::state_flags;
use mod_api::SceneView;
/// Prepared scene data. Entity fields remain borrowed for the duration of the draw.
pub(super) struct SceneBlock<'a> {
    pub pos: [i32; 3],
    pub state: horizon_world::registry::BlockStateId,
    pub entity: Option<&'a mod_api::SceneBlockEntity>,
    /// Its section's index; 0 in a scene with no sections.
    pub section: usize,
    /// Destroy stage 0 to 9 drawn over it, or none.
    pub breaking: Option<u8>,
    /// Its game block entity's saved data, or none for the type's defaults.
    pub game_data: Option<&'a mod_api::Compound>,
}

/// What a scene draws besides its blocks, checked: its sections (empty for one in place,
/// full bright), its own quads, its shading and the biome its tints come from.
pub(super) struct SceneLook<'a> {
    pub sections: &'a [mod_api::SceneSection],
    pub quads: &'a [mod_api::SceneQuad],
    pub shading: mod_api::SceneShading,
    pub biome: Option<u16>,
}

/// Vanilla's default water colour (`Biome`'s `water_color` default, plains'), for a scene
/// that names no biome.
const DEFAULT_WATER: u32 = 0x3F_76E4;

/// A section's transform as a matrix, when it is finite and affine (a bottom row of
/// `0, 0, 0, 1`): what `draw_scene` takes.
pub(super) fn section_matrix(section: &mod_api::SceneSection) -> Option<Mat4> {
    let t = section.transform;
    let affine = t[3] == 0.0 && t[7] == 0.0 && t[11] == 0.0 && t[15] == 1.0;
    (affine && t.iter().all(|v| v.is_finite())).then(|| Mat4::from_cols_array(&t))
}

/// `scene`'s sections, quads, shading and biome, checked before anything is drawn. A block
/// naming a section the scene doesn't have is refused by `scene_blocks`.
pub(super) fn check_look<'a>(
    scene: &'a mod_api::Scene,
    biome: impl FnOnce(&str) -> Option<u16>,
) -> Result<SceneLook<'a>, mod_api::SceneError> {
    use mod_api::SceneError;
    if scene.sections.len() > mod_api::SCENE_MAX_SECTIONS {
        return Err(SceneError::BadSection {
            index: mod_api::SCENE_MAX_SECTIONS as u32,
        });
    }
    for (index, section) in (0u32..).zip(&scene.sections) {
        if section_matrix(section).is_none() {
            return Err(SceneError::BadSection { index });
        }
    }
    if scene.quads.len() > mod_api::SCENE_MAX_QUADS {
        return Err(SceneError::TooManyQuads);
    }
    for (index, quad) in (0u32..).zip(&scene.quads) {
        if let Some(s) = quad.section
            && s as usize >= scene.sections.len()
        {
            return Err(SceneError::BadSection { index: s });
        }
        let finite = quad.corners.iter().flatten().all(|v| v.is_finite())
            && quad.uv.iter().flatten().all(|v| v.is_finite());
        if !finite {
            return Err(SceneError::BadQuad { index });
        }
    }
    let biome = match &scene.biome {
        Some(name) => Some(biome(&name.to_string()).ok_or(SceneError::BadBiome)?),
        None => None,
    };
    Ok(SceneLook {
        sections: &scene.sections,
        quads: &scene.quads,
        shading: scene.shading,
        biome,
    })
}

/// A quad in the scene's blocks, before it is seen: its corners, UVs in the
/// atlas, ABGR colour, and its outward normal.
struct ProjectedQuad {
    depth: f32,
    corners: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    colors: [u32; 4],
}

/// How a quad's colour is shaded when it is seen.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Shade {
    /// Its colour is final (entities, particles, a block entity's baked quads).
    Baked,
    /// By its normal, as the scene's shading says.
    Normal,
    /// A model face that takes no directional shading: as a face pointing straight up
    /// under view shading (catnip's `unshadedDiffuse`), by its normal under face shading
    /// (as scenes always drew it).
    Unshaded,
}

pub(super) struct SceneQuad {
    pub(super) corners: [Vec3; 4],
    pub(super) uv: [[f32; 2]; 4],
    /// ABGR, one per corner.
    pub(super) colors: [u32; 4],
    pub(super) normal: Vec3,
    /// Dropped when it faces away: block model faces, and the quads a block
    /// entity marks for culling as `entity.wgsl` does (`CULL_BACK`,
    /// `SHADED_CULL_W`). Other quads draw both sides, as in the world.
    pub(super) cull: bool,
    pub(super) shade: Shade,
}

impl SceneQuad {
    /// A quad of one colour whose shading is already in it.
    pub(super) fn baked(corners: [Vec3; 4], uv: [[f32; 2]; 4], color: u32, normal: Vec3, cull: bool) -> Self {
        Self {
            corners,
            uv,
            colors: [color; 4],
            normal,
            cull,
            shade: Shade::Baked,
        }
    }
}

/// The GUI transform of `view` in the box `rect` (x0, y0, x1, y1): the view's
/// centre at the box's middle, a block `scale` pixels across. Unturned, the
/// scene is seen from the north looking south, as a player at yaw 0 sees it,
/// up the screen's up; `yaw` turns it counterclockwise seen from above, and
/// `pitch` tilts its top towards the viewer. The GUI's y runs down and its z
/// towards the viewer, so the point reflection keeps the picture unmirrored.
pub(super) fn scene_matrix(rect: [f32; 4], view: SceneView) -> Mat4 {
    let middle = Vec3::new((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0, 0.0);
    Mat4::from_translation(middle)
        * Mat4::from_scale(Vec3::splat(-view.scale))
        * Mat4::from_rotation_x(-view.pitch.to_radians())
        * Mat4::from_rotation_y(view.yaw.to_radians())
        * Mat4::from_translation(-Vec3::from_array(view.center))
}

/// The step a model face's direction points: the model's `Direction` lists
/// its sides in the order the game's does.
fn step(direction: horizon_assets::model::Direction) -> [i32; 3] {
    horizon_core::geometry::Direction::ALL[direction as usize].normal()
}

/// How bright a face is with `normal` (world space, unit), as blocks are
/// shaded: 1 for the top, 0.5 underneath, 0.8 north and south, 0.6 east and
/// west, in between for a slanted face.
pub(super) fn face_shade(normal: Vec3) -> f32 {
    let up = if normal.y > 0.0 { 1.0 } else { 0.5 };
    (normal.x * normal.x * 0.6 + normal.y * normal.y * up + normal.z * normal.z * 0.8).min(1.0)
}

/// How bright a face is with `normal` (unit, in the view's frame: x right, y up, z toward
/// the viewer) under two view-fixed lights, as the game lights models in its GUI and
/// catnip's `diffuseLight` does: `min(1, (max(0, l0·n) + max(0, l1·n)) · 0.6 + 0.4)`, the
/// lights normalised first.
pub(super) fn view_shade(normal: Vec3, lights: [[f32; 3]; 2]) -> f32 {
    let lit = |l: [f32; 3]| Vec3::from_array(l).normalize_or_zero().dot(normal).max(0.0);
    ((lit(lights[0]) + lit(lights[1])) * 0.6 + 0.4).min(1.0)
}

/// `color` (ABGR) with its red, green and blue times `shade`.
pub(super) fn shaded(color: u32, shade: f32) -> u32 {
    let ch = |s: u32| (((color >> s & 255) as f32 * shade) as u32).min(255) << s;
    (color & 0xFF00_0000) | ch(0) | ch(8) | ch(16)
}

/// `color` (ABGR) times `light` (ABGR, alpha ignored), channel by channel.
fn lit(color: u32, light: u32) -> u32 {
    let ch = |s: u32| ((color >> s & 255) * (light >> s & 255) / 255) << s;
    (color & 0xFF00_0000) | ch(0) | ch(8) | ch(16)
}

/// The decal UV a crack takes on a model face, as `SheetedDecalTextureGenerator` lays the
/// destroy stage over a block: the corner's place within the block on the two axes the
/// face spans, `u` across and `v` down.
fn crack_uv(position: [f32; 3], facing: [i32; 3]) -> [f32; 2] {
    let [x, y, z] = position;
    match facing {
        [0, 1, 0] => [x, z],
        [0, -1, 0] => [x, 1.0 - z],
        [0, 0, -1] => [1.0 - x, 1.0 - y],
        [0, 0, 1] => [x, 1.0 - y],
        [-1, 0, 0] => [z, 1.0 - y],
        _ => [1.0 - z, 1.0 - y],
    }
}

impl Game<'_> {
    /// A small level of `blocks` with the game block entities their saved data gives (a
    /// block with no data gets none): what the game's block entity renderers read in a scene,
    /// a chest its neighbour, a sign its text.
    fn scene_entity_level<'b>(
        &self,
        blocks: impl Iterator<Item = &'b SceneBlock<'b>>,
    ) -> horizon_client::world::World {
        use horizon_core::nbt::Tag;
        let mut world = horizon_client::world::World::new(self.res.facts().air, 0);
        for b in blocks {
            let at = (b.pos[0] >> 4, b.pos[2] >> 4);
            if world.chunks.get(&at).is_none() {
                world.chunks.insert(
                    at,
                    std::sync::Arc::new(horizon_client::chunk::Chunk::empty(at.0, at.1)),
                );
            }
            world.set_stored(b.pos[0], b.pos[1], b.pos[2], b.state);
            let Some(data) = b.game_data else {
                continue;
            };
            let name = &*self.res.facts().get(b.state).name;
            let mut tag = data.clone();
            for (key, v) in ["x", "y", "z"].into_iter().zip(b.pos) {
                tag.insert(key.into(), Tag::Int(v));
            }
            // Saved data from a world names its type; a mod's may leave it to the block.
            if !tag.contains_key(&horizon_core::nbt::JavaString::from("id"))
                && let Some(id) = horizon_client::block_entity_types::type_for(name)
                    .or_else(|| horizon_client::lan::chunks::block_entity_type(name))
            {
                tag.insert("id".into(), Tag::String(id.into()));
            }
            if let Some((_, entity)) =
                horizon_client::save::decode_block_entity_for_block(&Tag::Compound(tag), name)
            {
                world.block_entities.insert((b.pos[0], b.pos[1], b.pos[2]), entity);
            }
        }
        world
    }

    /// The colour (ABGR) packed light `light` gives, as the client's lightmap lights the
    /// world this frame: block light in bits 8..16 and sky light in bits 0..8, each 16
    /// times its level. Full bright is white, so a scene that never sets a light draws as
    /// it did. Without a renderer (a test) it is the lightmap's grey curve.
    fn scene_light(&self, light: u32) -> u32 {
        if light == mod_api::SCENE_FULL_BRIGHT {
            return 0xFFFF_FFFF;
        }
        let sky = ((light & 0xFF) >> 4).min(15) as usize;
        let block = ((light >> 8 & 0xFF) >> 4).min(15) as usize;
        if let Some(r) = &self.renderer {
            let i = (sky * 16 + block) * 4;
            let p = &r.lightmap_pixels[i..i + 3];
            return horizon_client::hud::rgba(p[0], p[1], p[2], 255);
        }
        let level = sky.max(block) as u8;
        let v = (horizon_client::screen_effects::lightmap_brightness(0.0, level) * 255.0) as u8;
        horizon_client::hud::rgba(v, v, v, 255)
    }

    /// Draws `blocks`, a scene of block states at positions of its own (not
    /// the world's), into the box `rect` (x0, y0, x1, y1, GUI pixels) of a
    /// screen's `batch`, seen as `view` says: Create's Ponder scenes. Each
    /// block is drawn with its model, shaded as `look.shading` says and tinted by
    /// `look.biome`, a face against a solid neighbour in its own section left out;
    /// a block with an `entity` (a mod's) is drawn by `native_block_entity_submit` as in
    /// the world too, its `spin` parts turning with the game time. Every block, block
    /// entity and scene quad goes through its section's transform and light, and an
    /// undrawn section is left out; all sections share the call's one depth, so they hide
    /// each other where they cross. A block with `breaking` gets its destroy stage drawn
    /// over its model. What lies outside the box is cut off, and the scene has a depth of
    /// its own, so it never covers the screen's other drawing or is covered by it but in
    /// the order drawn. `extra` adds quads in the scene's coordinates before
    /// the scene is seen and sorted (a living scene's entities and particles,
    /// `native_scene_live`); a scene of blocks alone passes none. Nothing is
    /// drawn when a quad seen through the view isn't finite. API 422.
    pub(super) fn draw_scene_quads(
        &self,
        batch: &mut HudBatch,
        rect: [f32; 4],
        blocks: &[SceneBlock<'_>],
        look: &SceneLook<'_>,
        view: SceneView,
        extra: impl FnOnce(&mut Vec<SceneQuad>),
    ) -> Result<(), mod_api::SceneError> {
        if !self.native_window() {
            return Err(mod_api::SceneError::Unavailable);
        }
        if !view.scale.is_finite() || view.scale <= 0.0 {
            return Err(mod_api::SceneError::BadView);
        }
        // Each section's transform, normal matrix and light; one section in place when the
        // scene has none.
        let sectioned = !look.sections.is_empty();
        let frames: Vec<(Mat4, Mat3, u32, bool)> = if sectioned {
            look.sections
                .iter()
                .map(|s| {
                    let m = section_matrix(s).unwrap_or(Mat4::IDENTITY);
                    let normals = Mat3::from_mat4(m).inverse().transpose();
                    (m, normals, self.scene_light(s.light), s.drawn)
                })
                .collect()
        } else {
            vec![(Mat4::IDENTITY, Mat3::IDENTITY, 0xFFFF_FFFF, true)]
        };
        // The Game boundary prepared every state before any drawing began. Faces are
        // culled against solid neighbours in their own section only, as Ponder masks its
        // level to the section it draws.
        let occupied: std::collections::HashMap<(usize, [i32; 3]), _> = blocks
            .iter()
            .map(|b| ((if sectioned { b.section } else { 0 }, b.pos), b.state))
            .collect();
        let solid = |section: usize, pos: [i32; 3]| {
            occupied.get(&(section, pos)).is_some_and(|&state| {
                self.res.behaviours().facts(state).flags & state_flags::SOLID_RENDER != 0
            })
        };
        let grass = self.pack.item_renderer.grass_color(0.5, 1.0) as u32;
        let tables = &self.pack.context.tables;
        let biome_tint = |state, index| -> Option<u32> {
            let Some(id) = look.biome else {
                return tables.default_tint(state, index, grass);
            };
            let biomes = self.res.biomes();
            let info = biomes.get(id);
            Some(match tables.tint_source(state, index)? {
                mod_api::client::mesher::TintSource::Grass => biomes.grass(id, 0.0, 0.0),
                mod_api::client::mesher::TintSource::Foliage => info.foliage,
                mod_api::client::mesher::TintSource::DryFoliage => info.dry_foliage,
                mod_api::client::mesher::TintSource::Water => info.water,
                mod_api::client::mesher::TintSource::Fixed(c) => c,
                _ => return tables.default_tint(state, index, grass),
            } & 0xFF_FFFF)
        };
        let mut quads = Vec::new();
        for block in blocks {
            let section = if sectioned { block.section } else { 0 };
            let Some(&(frame, normals, light, drawn)) = frames.get(section) else {
                continue;
            };
            if !drawn {
                continue;
            }
            let start = quads.len();
            let at = Vec3::new(
                block.pos[0] as f32,
                block.pos[1] as f32,
                block.pos[2] as f32,
            );
            let Ok(models) = self
                .pack
                .render
                .model_catalog
                .collect_seeded(block.state, 42)
            else {
                continue;
            };
            let crack = block
                .breaking
                .and_then(|stage| {
                    self.gui
                        .atlas
                        .sprites
                        .get(&format!("block/destroy_stage_{stage}"))
                })
                .copied();
            for face in models.iter().flat_map(|(_, model)| &model.faces) {
                if let Some(cull) = face.quad.cull {
                    let [dx, dy, dz] = step(cull);
                    if solid(section, [block.pos[0] + dx, block.pos[1] + dy, block.pos[2] + dz]) {
                        continue;
                    }
                }
                let Some(sprite) = self
                    .pack
                    .layer_names
                    .get(&face.layer)
                    .and_then(|name| self.gui.atlas.sprites.get(name))
                else {
                    continue;
                };
                let facing = step(face.quad.facing);
                let normal = Vec3::new(facing[0] as f32, facing[1] as f32, facing[2] as f32);
                // Face shading is baked here, before the tint, as scenes always drew it; it
                // takes the normal its section turns the face to. View shading waits for
                // the view.
                let (mut color, shade) = match look.shading {
                    mod_api::SceneShading::View { .. } if face.quad.shade => {
                        (0xFFFF_FFFF, Shade::Normal)
                    }
                    mod_api::SceneShading::View { .. } => (0xFFFF_FFFF, Shade::Unshaded),
                    _ => {
                        let turned = (normals * normal).normalize_or_zero();
                        (shaded(0xFFFF_FFFF, face_shade(turned)), Shade::Baked)
                    }
                };
                if let Some(t) = biome_tint(block.state, face.quad.tint_index) {
                    color = horizon_client::hud_layout::multiply(
                        color,
                        horizon_client::mobs::render::abgr(t),
                    );
                }
                let corners = face.quad.positions.map(|p| at + Vec3::from_array(p));
                quads.push(SceneQuad {
                    corners,
                    uv: face.quad.uv.map(|uv| {
                        [
                            sprite.u0 + uv[0] * (sprite.u1 - sprite.u0),
                            sprite.v0 + uv[1] * (sprite.v1 - sprite.v0),
                        ]
                    }),
                    colors: [color; 4],
                    normal,
                    cull: true,
                    shade,
                });
                if let Some(c) = crack {
                    // The destroy stage laid over the face, as the world draws a block
                    // being mined.
                    quads.push(SceneQuad {
                        corners,
                        uv: face.quad.positions.map(|p| {
                            let [u, v] = crack_uv(p, facing);
                            [c.u0 + u * (c.u1 - c.u0), c.v0 + v * (c.v1 - c.v0)]
                        }),
                        colors: [0xFFFF_FFFF; 4],
                        normal,
                        cull: true,
                        shade: Shade::Baked,
                    });
                }
            }
            if let Some(entity) = &block.entity {
                let entity =
                    mod_api::ModBlockEntity::new(entity.id.to_string(), entity.fields.clone());
                let name = &*self.res.facts().get(block.state).name;
                let mut draw = BlockEntityDraw::posed(
                    self.clock.fraction() as f32,
                    self.world.game_time,
                    if sectioned {
                        look.sections[section].light
                    } else {
                        240 | 240 << 8
                    },
                    block.pos,
                    Mat4::IDENTITY,
                    diffuse_lights(false),
                    Vec::new(),
                )
                .animated(self.animation_time());
                self.native_block_entity_submit(block.pos, name, block.state, &entity, &mut draw);
                // Its text's new glyphs upload with this screen's batch.
                batch.patches.append(&mut self.text_patches.borrow_mut());
                let time = draw.animation();
                let (drawn, spins) = draw.into_turning();
                let mut placed: Vec<Vertex> = Vec::with_capacity(drawn.len());
                place_turning(&drawn, &spins, time, at, &mut placed);
                for q in placed.as_chunks::<4>().0 {
                    let corners = q.each_ref().map(|v| Vec3::from_array(v.pos));
                    let [nx, ny, nz, w] = q[0].normal;
                    let mut normal = (corners[1] - corners[0])
                        .cross(corners[2] - corners[0])
                        .normalize_or_zero();
                    if normal == Vec3::ZERO {
                        normal = Vec3::new(f32::from(nx), f32::from(ny), f32::from(nz))
                            .normalize_or_zero();
                    }
                    // `entity.wgsl`: |w| >= 0.5 is shaded by the normal; smaller
                    // keeps the colour baked in (quads, cubes, items, text).
                    let unbaked = w.unsigned_abs() >= 64;
                    quads.push(SceneQuad {
                        corners,
                        uv: q.each_ref().map(|v| v.uv),
                        colors: [q[0].color; 4],
                        normal,
                        cull: horizon_client::mesher::culls_back(q[0].normal),
                        shade: if unbaked { Shade::Normal } else { Shade::Baked },
                    });
                }
            }
            if sectioned {
                for q in &mut quads[start..] {
                    q.corners = q.corners.map(|c| frame.transform_point3(c));
                    q.normal = (normals * q.normal).normalize_or_zero();
                    q.colors = q.colors.map(|c| lit(c, light));
                }
            }
        }
        // Fluids: each drawn section's fluid cells meshed as the world meshes them, against
        // that section's own blocks (air elsewhere), so heights, flow and hidden faces match.
        let air = self.res.facts().air.0;
        let water = match look.biome {
            Some(id) => self.res.biomes().get(id).water & 0xFF_FFFF,
            None => DEFAULT_WATER,
        };
        for (section, &(frame, normals, light, drawn)) in frames.iter().enumerate() {
            let cells: Vec<[i32; 3]> = blocks
                .iter()
                .filter(|b| !sectioned || b.section == section)
                .map(|b| b.pos)
                .collect();
            if !drawn || cells.is_empty() {
                continue;
            }
            let state_at = |p: [i32; 3]| occupied.get(&(section, p)).map_or(air, |s| s.0);
            let fluid = horizon_client::mesher::scene_fluids(
                tables,
                &self.pack.context.part_tables,
                &cells,
                &state_at,
                water,
            );
            for q in fluid.as_chunks::<4>().0 {
                let layer = u32::from(q[0].layer & !horizon_client::mesher::OPAQUE_LAYER);
                let Some(sprite) = self
                    .pack
                    .layer_names
                    .get(&layer)
                    .and_then(|name| self.gui.atlas.sprites.get(name))
                else {
                    continue;
                };
                let corners = q.each_ref().map(|v| frame.transform_point3(Vec3::from_array(v.pos)));
                let flat = q.each_ref().map(|v| Vec3::from_array(v.pos));
                let normal = (flat[1] - flat[0]).cross(flat[2] - flat[0]).normalize_or_zero();
                let color = q[0].color;
                // Face shading is baked before the light, as for block faces.
                let (color, shade) = match look.shading {
                    mod_api::SceneShading::View { .. } => (color, Shade::Normal),
                    _ => (
                        shaded(color, face_shade((normals * normal).normalize_or_zero())),
                        Shade::Baked,
                    ),
                };
                quads.push(SceneQuad {
                    corners,
                    uv: q.each_ref().map(|v| {
                        let [u, w] = v.uv();
                        [
                            sprite.u0 + u * (sprite.u1 - sprite.u0),
                            sprite.v0 + w * (sprite.v1 - sprite.v0),
                        ]
                    }),
                    colors: [lit(color, light); 4],
                    normal: (normals * normal).normalize_or_zero(),
                    cull: true,
                    shade,
                });
            }
        }
        // The game's own block entities (chests, signs, banners…), drawn by their renderers as
        // in the world, against a small level of their section's blocks holding the saved data
        // each block gives, nothing ticking. A mod's block entity keeps its own path above.
        let renderer = self.res.block_entity_render();
        let game_entities = |section: usize| {
            blocks.iter().filter(move |b| {
                (!sectioned || b.section == section)
                    && b.entity.is_none()
                    && renderer.renders(b.state.0)
            })
        };
        for (section, &(frame, normals, light, drawn)) in frames.iter().enumerate() {
            if !drawn || game_entities(section).next().is_none() {
                continue;
            }
            let world = self.scene_entity_level(
                blocks.iter().filter(|b| !sectioned || b.section == section),
            );
            let sign = |p: [i32; 3]| match world.block_entities.get(&(p[0], p[1], p[2])) {
                Some(BlockEntity::Sign(sign)) => Some(sign),
                _ => None,
            };
            let banner = |p: [i32; 3]| match world.block_entities.get(&(p[0], p[1], p[2])) {
                Some(BlockEntity::Banner(b)) => {
                    super::banners::layer_textures(&self.stations().banners, &b.layers)
                }
                _ => Vec::new(),
            };
            let scene = horizon_client::block_entity_render::Scene {
                camera: glam::DVec3::ZERO,
                spyglass_outline: false,
                orientation: glam::Quat::IDENTITY,
                anim: &|_| horizon_client::block_entity_render::BlockAnim::default(),
                texts: None,
                partial: self.clock.fraction() as f32,
                game_time: self.world.game_time,
                max_y: world.shape.max_y() - 1,
                world: &world,
                sprites: &self.gui.atlas.sprites,
                emissive: &self.gui.atlas.emissive,
                translucent_sprites: &self.gui.atlas.translucent,
                atlas_revision: self.gui.atlas.revision,
                nether: false,
                lid: &|_: [i32; 3]| 0.0,
                sign: &sign,
                banner: &banner,
                skull: &|_| None,
                glyph: &|c| self.gui.atlas.glyph_cell(c),
                glyphs: self.gui.atlas.font.epoch(),
                font: Some(&self.gui.atlas.font),
                dynamic_visible: None,
            };
            let mut placed: Vec<Vertex> = Vec::new();
            for block in game_entities(section) {
                placed.clear();
                let at = Vec3::new(block.pos[0] as f32, block.pos[1] as f32, block.pos[2] as f32);
                // Full bright: the section's light is applied below, as to its blocks.
                renderer.render_block(
                    &scene,
                    block.pos,
                    block.state.0,
                    Mat4::from_translation(at),
                    0xFF,
                    &mut placed,
                );
                // Sign text's new glyphs upload with this screen's batch.
                batch.patches.append(&mut self.text_patches.borrow_mut());
                for q in placed.as_chunks::<4>().0 {
                    let flat = q.each_ref().map(|v| Vec3::from_array(v.pos));
                    let [nx, ny, nz, w] = q[0].normal;
                    let mut normal = (flat[1] - flat[0]).cross(flat[2] - flat[0]).normalize_or_zero();
                    if normal == Vec3::ZERO {
                        normal = Vec3::new(f32::from(nx), f32::from(ny), f32::from(nz))
                            .normalize_or_zero();
                    }
                    // `entity.wgsl`: |w| >= 0.5 is shaded by the normal; smaller keeps the
                    // colour baked in.
                    let unbaked = w.unsigned_abs() >= 64;
                    quads.push(SceneQuad {
                        corners: flat.map(|c| frame.transform_point3(c)),
                        uv: q.each_ref().map(|v| v.uv),
                        colors: [lit(q[0].color, light); 4],
                        normal: (normals * normal).normalize_or_zero(),
                        cull: horizon_client::mesher::culls_back(q[0].normal),
                        shade: if unbaked { Shade::Normal } else { Shade::Baked },
                    });
                }
            }
        }
        // The scene's own quads: outlines, shadows, flashes.
        let white = self.gui.atlas.white_uv();
        for (index, q) in (0u32..).zip(look.quads) {
            let (frame, light) = match q.section {
                Some(s) => {
                    let (m, _, l, drawn) = frames[s as usize];
                    if !drawn {
                        continue;
                    }
                    (m, l)
                }
                None => (Mat4::IDENTITY, 0xFFFF_FFFF),
            };
            let uv = match &q.sprite {
                Some(name) => {
                    let r = self
                        .gui
                        .atlas
                        .sprites
                        .get(name.as_str())
                        .ok_or(mod_api::SceneError::BadQuad { index })?;
                    q.uv.map(|[u, v]| [r.u0 + u * (r.u1 - r.u0), r.v0 + v * (r.v1 - r.v0)])
                }
                None => [white; 4],
            };
            let corners = q.corners.map(|c| frame.transform_point3(Vec3::from_array(c)));
            let normal = (corners[1] - corners[0])
                .cross(corners[2] - corners[0])
                .normalize_or_zero();
            quads.push(SceneQuad {
                corners,
                uv,
                colors: q
                    .colors
                    .map(|c| lit(horizon_client::hud_layout::abgr(c), light)),
                normal,
                cull: q.cull,
                shade: if q.shade { Shade::Normal } else { Shade::Baked },
            });
        }
        extra(&mut quads);
        let m = scene_matrix(rect, view);
        // A normal in the view's frame: x right, y up (the GUI's y runs down), z toward the
        // viewer.
        let in_view = |n: Vec3| {
            let v = m.transform_vector3(n);
            Vec3::new(v.x, -v.y, v.z).normalize_or_zero()
        };
        let shade_of = |q: &SceneQuad| -> Option<f32> {
            match (q.shade, look.shading) {
                (Shade::Baked, _) => None,
                (_, mod_api::SceneShading::Faces) => Some(face_shade(q.normal)),
                (Shade::Normal, mod_api::SceneShading::View { lights }) => {
                    Some(view_shade(in_view(q.normal), lights))
                }
                (Shade::Unshaded, mod_api::SceneShading::View { lights }) => {
                    Some(view_shade(Vec3::Y, lights))
                }
                (_, _) => Some(face_shade(q.normal)),
            }
        };
        let mut seen: Vec<ProjectedQuad> = quads
            .into_iter()
            .filter(|q| !q.cull || m.transform_vector3(q.normal).z > 0.0)
            .map(|q| {
                let colors = match shade_of(&q) {
                    Some(s) => q.colors.map(|c| shaded(c, s)),
                    None => q.colors,
                };
                let p = q.corners.map(|c| m.transform_point3(c));
                let depth = p.iter().map(|v| v.z).sum::<f32>() / 4.0;
                ProjectedQuad {
                    depth,
                    corners: p.map(|v| v.to_array()),
                    uv: q.uv,
                    colors,
                }
            })
            .collect();
        // Mod-chosen views and positions may overflow: refuse before any vertex.
        let finite = |q: &ProjectedQuad| {
            q.depth.is_finite() && q.corners.iter().flatten().all(|v| v.is_finite())
        };
        if !seen.iter().all(finite) {
            return Err(mod_api::SceneError::BadView);
        }
        // Far to near, for the faces that let others show through.
        seen.sort_by(|a, b| a.depth.total_cmp(&b.depth));
        let start = batch.vertices.len();
        for face in seen {
            batch.quad_3d_colors(face.corners, face.uv, face.colors);
        }
        batch.depth_since(start, rect);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(yaw: f32, pitch: f32) -> SceneView {
        SceneView::new([0.5, 0.5, 0.5], yaw, pitch, 20.0)
    }

    /// Unturned, the scene is seen from the north as a player at yaw 0 sees
    /// it: the centre in the box's middle, up up the screen, east to the
    /// viewer's left, south away and its north face towards the viewer.
    #[test]
    fn an_unturned_scene_is_seen_from_the_north() {
        let m = scene_matrix([0.0, 0.0, 100.0, 60.0], view(0.0, 0.0));
        let at = |p: [f32; 3]| m.transform_point3(Vec3::from_array(p));
        assert_eq!(at([0.5, 0.5, 0.5]), Vec3::new(50.0, 30.0, 0.0));
        assert!(at([0.5, 1.5, 0.5]).y < 30.0, "up is up the screen");
        assert!(at([1.5, 0.5, 0.5]).x < 50.0, "east is on the left");
        assert!(at([0.5, 0.5, 1.5]).z < 0.0, "south is away");
        assert!(
            m.transform_vector3(Vec3::NEG_Z).z > 0.0,
            "the north face is seen"
        );
        assert!(
            m.transform_vector3(Vec3::Y).z.abs() < 1e-5,
            "the top is edge on"
        );
    }

    /// A tilt shows the top and hides the bottom; a quarter turn brings the
    /// east face round to the viewer (counterclockwise seen from above: east
    /// goes to the north, the side the viewer is on).
    #[test]
    fn pitch_shows_the_top_and_yaw_turns_the_scene() {
        let tilted = scene_matrix([0.0, 0.0, 100.0, 100.0], view(0.0, 30.0));
        assert!(tilted.transform_vector3(Vec3::Y).z > 0.0);
        assert!(tilted.transform_vector3(Vec3::NEG_Y).z < 0.0);
        let turned = scene_matrix([0.0, 0.0, 100.0, 100.0], view(90.0, 0.0));
        assert!(turned.transform_vector3(Vec3::X).z > 0.99);
    }

    #[test]
    fn faces_are_shaded_as_blocks_are() {
        assert_eq!(face_shade(Vec3::Y), 1.0);
        assert_eq!(face_shade(Vec3::NEG_Y), 0.5);
        assert!((face_shade(Vec3::Z) - 0.8).abs() < 1e-6);
        assert!((face_shade(Vec3::X) - 0.6).abs() < 1e-6);
        assert_eq!(shaded(0xFF80_4020, 0.5), 0xFF40_2010);
    }

    /// catnip's `diffuseLight` with Ponder's two GUI lights, worked by hand: a face toward
    /// the viewer, one straight up, and one facing away from both lights.
    #[test]
    fn view_shading_is_the_two_light_formula() {
        let ponder = [[0.4, -1.0, 0.7], [-0.4, -0.5, 0.7]];
        let l0 = Vec3::new(0.4, -1.0, 0.7).normalize();
        let l1 = Vec3::new(-0.4, -0.5, 0.7).normalize();
        let toward = ((l0.z + l1.z) * 0.6 + 0.4).min(1.0);
        assert!((view_shade(Vec3::Z, ponder) - toward).abs() < 1e-6);
        // Both lights come from below: a face straight up gets the ambient 0.4 alone.
        assert!((view_shade(Vec3::Y, ponder) - 0.4).abs() < 1e-6);
        assert!((view_shade(Vec3::NEG_Z, ponder) - 0.4).abs() < 1e-6);
        assert_eq!(view_shade(Vec3::NEG_Y, [[0.0, -1.0, 0.0]; 2]), 1.0, "capped at 1");
    }

    #[test]
    fn a_section_transform_must_be_finite_and_affine() {
        let moved = mod_api::SceneSection::new().transformed(
            Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0)).to_cols_array(),
        );
        assert_eq!(
            section_matrix(&moved).map(|m| m.transform_point3(Vec3::ZERO)),
            Some(Vec3::new(0.0, 0.5, 0.0))
        );
        let mut projective = Mat4::IDENTITY.to_cols_array();
        projective[3] = 0.5;
        assert_eq!(section_matrix(&moved.transformed(projective)), None);
        let mut infinite = Mat4::IDENTITY.to_cols_array();
        infinite[12] = f32::NAN;
        assert_eq!(section_matrix(&moved.transformed(infinite)), None);
    }

    #[test]
    fn the_look_is_checked_before_anything_is_drawn() {
        use mod_api::{Scene, SceneError, SceneQuad, SceneSection};
        let plains = |name: &str| (name == "minecraft:plains").then_some(1);
        let quad = SceneQuad::new([[0.0; 3]; 4], 0xFF00_0000);
        let scene = Scene::new(Vec::new())
            .with_sections(vec![SceneSection::new(), SceneSection::new().lit(5, 5)])
            .with_quads(vec![quad.clone().in_section(1)])
            .with_biome(mod_api::Identifier::parse("minecraft:plains").unwrap());
        let look = check_look(&scene, plains).unwrap();
        assert_eq!((look.sections.len(), look.quads.len(), look.biome), (2, 1, Some(1)));

        let mut bad = Mat4::IDENTITY.to_cols_array();
        bad[15] = 2.0;
        let mut sections = scene.sections.clone();
        sections[1] = SceneSection::new().transformed(bad);
        let refused = scene.clone().with_sections(sections);
        assert_eq!(
            check_look(&refused, plains).err(),
            Some(SceneError::BadSection { index: 1 })
        );
        let named = scene.clone().with_quads(vec![quad.clone().in_section(2)]);
        assert_eq!(
            check_look(&named, plains).err(),
            Some(SceneError::BadSection { index: 2 })
        );
        let mut nan = quad.clone();
        nan.corners[2][1] = f32::INFINITY;
        let unfinite = scene.clone().with_quads(vec![quad.clone(), nan]);
        assert_eq!(
            check_look(&unfinite, plains).err(),
            Some(SceneError::BadQuad { index: 1 })
        );
        let many = scene
            .clone()
            .with_quads(vec![quad; mod_api::SCENE_MAX_QUADS + 1]);
        assert_eq!(check_look(&many, plains).err(), Some(SceneError::TooManyQuads));
        let elsewhere = scene.with_biome(mod_api::Identifier::parse("minecraft:nowhere").unwrap());
        assert_eq!(check_look(&elsewhere, plains).err(), Some(SceneError::BadBiome));
    }

    #[test]
    fn light_and_cracks() {
        assert_eq!(lit(0xFF80_4020, 0xFFFF_FFFF), 0xFF80_4020, "full bright changes nothing");
        assert_eq!(lit(0xFF80_4020, 0x0080_8080), 0xFF40_2010);
        // A crack lies on a face as the decal does: top seen from above, x across, z down.
        assert_eq!(crack_uv([0.25, 1.0, 0.75], [0, 1, 0]), [0.25, 0.75]);
        assert_eq!(crack_uv([0.25, 0.0, 0.0], [0, 0, 1]), [0.25, 1.0]);
        assert_eq!(crack_uv([1.0, 1.0, 0.25], [1, 0, 0]), [0.75, 0.0]);
    }
}
