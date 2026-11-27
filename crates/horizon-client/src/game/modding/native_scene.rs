//! A few blocks drawn in a screen as a small scene seen from an angle: what
//! Create's Ponder shows its machines in (docs/native-mods.md, "Scenes in
//! screens"). The blocks' models and their mod block entities' drawing are
//! projected into the GUI with their own depth, as the smithing screen draws
//! its armour stand.
use super::Game;
use super::native_block_entity_draw::{BlockEntityDraw, place_turning};
use super::native_scene_live::LiveBlockEntity;
use glam::{Affine3A, EulerRot, Mat4, Quat, Vec3};
use horizon_client::{
    block_entity_render::diffuse_lights,
    containers::BlockEntity,
    hud::{HudAtlas, HudBatch, OverlayDepth},
    hud_layout::abgr,
    mesher::Vertex,
};
use horizon_world::level::data::state_flags;
use mod_api::{
    SCENE_FULL_BRIGHT, SCENE_MAX_OVERLAYS, SCENE_MAX_TEXTURED_QUADS, SceneDepth, SceneError,
    SceneFace, SceneGroup, SceneOverlay, SceneOverlayError, SceneParts, ScenePartsError,
    SceneSection, SceneSectionError, SceneShading, SceneTexturedQuad, SceneView,
    SceneWithQuadsError,
};
/// Prepared scene data. Entity fields remain borrowed for the duration of the draw.
pub(super) struct SceneBlock<'a> {
    pub pos: [i32; 3],
    pub state: horizon_world::registry::BlockStateId,
    pub entity: Option<&'a mod_api::SceneBlockEntity>,
    /// Destroy stage 0 to 9 drawn over it, or none.
    pub breaking: Option<u8>,
    /// Its game block entity's saved data, or none for the type's defaults.
    pub game_data: Option<&'a mod_api::Compound>,
}

/// Gives `cells` the saved data and destroy stages `parts` names them by index.
pub(super) fn apply_block_parts<'a>(
    cells: &mut [SceneBlock<'a>],
    parts: &'a SceneParts,
) -> Result<(), ScenePartsError> {
    for (index, data) in &parts.block_data {
        let cell = cells
            .get_mut(*index as usize)
            .ok_or(ScenePartsError::NoBlock { index: *index })?;
        cell.game_data = Some(data);
    }
    for &(index, stage) in &parts.breaking {
        let cell = cells
            .get_mut(index as usize)
            .ok_or(ScenePartsError::NoBlock { index })?;
        cell.breaking = (stage <= 9).then_some(stage);
    }
    Ok(())
}

/// How a scene is drawn, beyond its blocks and view, checked. The default is `draw_scene`'s.
#[derive(Clone, Copy, Default)]
pub(super) struct SceneLook {
    pub(super) shading: SceneShading,
    /// The biome its tints come from.
    pub(super) biome: Option<u16>,
    pub(super) fluids: bool,
    pub(super) game_block_entities: bool,
    pub(super) keep_particles: bool,
    /// The scene's clock as its tick and partial tick: the game's block entities are
    /// posed on it, not on the client's clock.
    pub(super) clock: Option<(u64, f32)>,
}

impl SceneLook {
    /// `parts`' look, its biome found by `biome`.
    pub(super) fn of(
        parts: &SceneParts,
        biome: impl FnOnce(&str) -> Option<u16>,
    ) -> Result<Self, ScenePartsError> {
        let biome = match &parts.biome {
            Some(name) => Some(biome(&name.to_string()).ok_or(ScenePartsError::BadBiome)?),
            None => None,
        };
        Ok(Self {
            shading: parts.shading,
            biome,
            fluids: parts.fluids,
            game_block_entities: parts.game_block_entities,
            keep_particles: parts.keep_particles,
            clock: None,
        })
    }
}

/// Where a scene entity stands among the groups, and its age: `SceneParts::entities`,
/// checked.
#[derive(Clone, Copy, Default)]
pub(super) struct EntityPlace {
    /// Its group's index in `SceneParts::groups`.
    pub(super) group: Option<usize>,
    pub(super) age: Option<u32>,
}

/// `parts`' entity parts for a scene of `entities` entities, one place per entity (none at
/// all when no part names one).
pub(super) fn entity_places(
    parts: &SceneParts,
    entities: usize,
) -> Result<Vec<EntityPlace>, ScenePartsError> {
    if parts.entities.is_empty() {
        return Ok(Vec::new());
    }
    let mut places = vec![None; entities];
    for (index, part) in (0u32..).zip(&parts.entities) {
        let bad = ScenePartsError::BadEntityPart { index };
        let group = part.group.map(|g| g as usize);
        if group.is_some_and(|g| g >= parts.groups.len()) {
            return Err(bad);
        }
        let place = places.get_mut(part.entity as usize).ok_or(bad)?;
        if place.is_some() {
            return Err(bad);
        }
        *place = Some(EntityPlace {
            group,
            age: part.age,
        });
    }
    Ok(places.into_iter().map(Option::unwrap_or_default).collect())
}

/// Vanilla's default water colour (`Biome`'s `water_color` default, plains'), for a scene
/// that names no biome.
const DEFAULT_WATER: u32 = 0x3F_76E4;

struct SectionPose {
    range: std::ops::Range<usize>,
    transform: Affine3A,
    light: u32,
    drawn: bool,
}

pub(super) struct SceneBlocks<'a, 'b> {
    pub(super) cells: &'a [SceneBlock<'b>],
    poses: Vec<SectionPose>,
}

impl<'a, 'b> SceneBlocks<'a, 'b> {
    pub(super) fn stationary(cells: &'a [SceneBlock<'b>]) -> Self {
        Self {
            cells,
            poses: Vec::new(),
        }
    }

    pub(super) fn posed(
        cells: &'a [SceneBlock<'b>],
        sections: &[SceneSection],
    ) -> Result<Self, SceneSectionError> {
        let mut poses = Vec::with_capacity(sections.len().min(cells.len()));
        let mut previous_end = 0;
        for (index, section) in (0u32..).zip(sections) {
            let invalid = SceneSectionError::BadSection { index };
            let start = section.first as usize;
            let end = start.checked_add(section.count as usize).ok_or(invalid)?;
            if start < previous_end || start == end || end > cells.len() {
                return Err(invalid);
            }
            let [x, y, z] = section.rotation.map(f32::to_radians);
            let pivot = Vec3::from_array(section.pivot);
            let translation = Vec3::from_array(section.translation);
            if ![x, y, z].into_iter().all(f32::is_finite)
                || !pivot.is_finite()
                || !translation.is_finite()
            {
                return Err(invalid);
            }
            let rotation = Quat::from_euler(EulerRot::ZYX, z, y, x);
            let transform = Affine3A::from_rotation_translation(
                rotation,
                translation + (pivot - rotation * pivot),
            );
            if !transform.is_finite() {
                return Err(invalid);
            }
            poses.push(SectionPose {
                range: start..end,
                transform,
                light: SCENE_FULL_BRIGHT,
                drawn: true,
            });
            previous_end = end;
        }
        Ok(Self { cells, poses })
    }

    /// The same, each section in its group's light and drawn or not.
    pub(super) fn grouped(
        cells: &'a [SceneBlock<'b>],
        groups: &[SceneGroup],
    ) -> Result<Self, SceneSectionError> {
        let sections: Vec<_> = groups.iter().map(|group| group.section).collect();
        let mut blocks = Self::posed(cells, &sections)?;
        for (pose, group) in blocks.poses.iter_mut().zip(groups) {
            (pose.light, pose.drawn) = (group.light, group.drawn);
        }
        Ok(blocks)
    }

    /// Group `group`'s pose (an index in `SceneParts::groups`), light and whether it is
    /// drawn.
    pub(super) fn group(&self, group: usize) -> Option<(Affine3A, u32, bool)> {
        let pose = self.poses.get(group)?;
        Some((pose.transform, pose.light, pose.drawn))
    }

    /// Culling group `group`'s pose; none for the stationary blocks.
    fn pose(&self, group: usize) -> Option<&SectionPose> {
        group.checked_sub(1).and_then(|at| self.poses.get(at))
    }

    /// Blocks and their culling group, in the original order; group 0 is stationary.
    fn iter(&self) -> impl Iterator<Item = (&SceneBlock<'b>, usize, Option<&SectionPose>)> {
        let mut sections = self.poses.iter().enumerate().peekable();
        self.cells.iter().enumerate().map(move |(index, block)| {
            if sections
                .peek()
                .is_some_and(|(_, pose)| pose.range.end <= index)
            {
                sections.next();
            }
            match sections.peek() {
                Some((group, pose)) if pose.range.contains(&index) => {
                    (block, group + 1, Some(*pose))
                }
                _ => (block, 0, None),
            }
        })
    }
}

/// A scene quad projected into GUI coordinates, with atlas UVs and ABGR colour.
struct ProjectedQuad {
    depth: f32,
    corners: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    colors: [u32; 4],
}

impl ProjectedQuad {
    fn new(m: Mat4, corners: [Vec3; 4], uv: [[f32; 2]; 4], colors: [u32; 4]) -> Self {
        let pos = corners.map(|c| m.transform_point3(c));
        Self {
            depth: pos.iter().map(|v| v.z).sum::<f32>() / 4.0,
            corners: pos.map(|v| v.to_array()),
            uv,
            colors,
        }
    }

    fn finite(&self) -> bool {
        self.depth.is_finite() && self.corners.iter().flatten().all(|v| v.is_finite())
    }
}

/// An overlay projected into GUI coordinates, with its ABGR colour.
struct ProjectedOverlay {
    corners: [[f32; 3]; 4],
    color: u32,
    depth: OverlayDepth,
}

/// The view for one draw, with any textured faces and overlays already resolved
/// and projected.
pub(super) struct SceneFrame {
    pub(super) rect: [f32; 4],
    pub(super) view: SceneView,
    pub(super) look: SceneLook,
    /// Each scene entity's place, by its index; empty when every entity stands still.
    pub(super) entities: Vec<EntityPlace>,
    matrix: Mat4,
    projected: Vec<ProjectedQuad>,
    overlays: Vec<ProjectedOverlay>,
}

impl SceneFrame {
    pub(super) fn new(rect: [f32; 4], view: SceneView) -> Self {
        Self {
            rect,
            view,
            look: SceneLook::default(),
            entities: Vec::new(),
            matrix: scene_matrix(rect, view),
            projected: Vec::new(),
            overlays: Vec::new(),
        }
    }

    /// The same frame with `parts`' faces, overlays and look, all checked.
    pub(super) fn with_parts(
        mut self,
        atlas: &HudAtlas,
        look: SceneLook,
        parts: &SceneParts,
    ) -> Result<Self, ScenePartsError> {
        self.look = look;
        let faces = parts.faces.iter().map(
            |SceneFace {
                 quad, corner_tints, ..
             }| { (quad, corner_tints.unwrap_or([quad.tint; 4])) },
        );
        Ok(self
            .with_faces(atlas, faces)?
            .with_overlays(&parts.overlays)?)
    }

    /// Project the entire overlay list before a living scene advances or any
    /// geometry is emitted.
    pub(super) fn with_overlays(
        mut self,
        overlays: &[SceneOverlay],
    ) -> Result<Self, SceneOverlayError> {
        if !self.view.scale.is_finite() || self.view.scale <= 0.0 {
            return Err(SceneError::BadView.into());
        }
        if overlays.len() > SCENE_MAX_OVERLAYS {
            return Err(SceneOverlayError::TooManyOverlays);
        }
        let m = self.matrix;
        self.overlays.reserve_exact(overlays.len());
        let point = |p| m.transform_point3(Vec3::from_array(p));
        for (index, overlay) in (0u32..).zip(overlays) {
            let bad = SceneOverlayError::BadOverlay { index };
            let (corners, argb, depth) = match *overlay {
                SceneOverlay::Line {
                    from,
                    to,
                    width,
                    argb,
                    depth,
                } => {
                    let (from, to) = (point(from), point(to));
                    if !from.is_finite() || !to.is_finite() || !width.is_finite() || width <= 0.0 {
                        return Err(bad);
                    }
                    // The screen-space perpendicular expands each endpoint without changing
                    // its depth. Use f64 for the direction so large finite deltas don't overflow.
                    let delta = (to.as_dvec3() - from.as_dvec3()).truncate();
                    let length = delta.length();
                    if length == 0.0 {
                        continue;
                    }
                    let offset = (delta.perp() * (f64::from(width) * 0.5 / length))
                        .as_vec2()
                        .extend(0.0);
                    (
                        [from + offset, from - offset, to - offset, to + offset],
                        argb,
                        depth,
                    )
                }
                SceneOverlay::Quad {
                    corners,
                    argb,
                    depth,
                } => (corners.map(point), argb, depth),
                _ => return Err(bad),
            };
            if !corners.iter().all(|p| p.is_finite()) {
                return Err(bad);
            }
            self.overlays.push(ProjectedOverlay {
                corners: corners.map(|p| p.to_array()),
                color: abgr(argb),
                depth: match depth {
                    SceneDepth::Tested => OverlayDepth::Tested,
                    SceneDepth::ThroughWalls => OverlayDepth::ThroughWalls,
                    _ => return Err(bad),
                },
            });
        }
        Ok(self)
    }

    /// Resolve and validate the caller's faces before any draw or live scene work.
    pub(super) fn with_textured_quads(
        self,
        atlas: &HudAtlas,
        quads: &[SceneTexturedQuad],
    ) -> Result<Self, SceneWithQuadsError> {
        self.with_faces(atlas, quads.iter().map(|q| (q, [q.tint; 4])))
    }

    /// The same for faces with a tint per corner.
    fn with_faces<'a>(
        mut self,
        atlas: &HudAtlas,
        quads: impl ExactSizeIterator<Item = (&'a SceneTexturedQuad, [u32; 4])>,
    ) -> Result<Self, SceneWithQuadsError> {
        if quads.len() > SCENE_MAX_TEXTURED_QUADS {
            return Err(SceneWithQuadsError::TooManyTexturedQuads);
        }
        if !self.view.scale.is_finite()
            || self.view.scale <= 0.0
            || !self.matrix.is_finite()
            || !self.rect.iter().all(|v| v.is_finite())
        {
            return Err(SceneWithQuadsError::Scene(SceneError::BadView));
        }
        self.projected.reserve_exact(quads.len());
        for (index, (q, tints)) in (0u32..).zip(quads) {
            if !q.corners.iter().flatten().all(|v| v.abs() <= 30_000_000.0)
                || !q.uv.iter().flatten().all(|v| (0.0..=1.0).contains(v))
            {
                return Err(SceneWithQuadsError::BadTexturedQuad { index });
            }
            // The atlas packs Minecraft textures by their path and other namespaces verbatim.
            let name = q.sprite.strip_prefix("minecraft:").unwrap_or(&q.sprite);
            let sprite = atlas
                .sprites
                .get(name)
                .ok_or(SceneWithQuadsError::UnknownSprite { index })?;
            let uv = q.uv.map(|[u, v]| {
                [
                    sprite.u0 + u * (sprite.u1 - sprite.u0),
                    sprite.v0 + v * (sprite.v1 - sprite.v0),
                ]
            });
            let quad = ProjectedQuad::new(
                self.matrix,
                q.corners.map(Vec3::from_array),
                uv,
                tints.map(abgr),
            );
            if !quad.finite() {
                return Err(SceneWithQuadsError::Scene(SceneError::BadView));
            }
            self.projected.push(quad);
        }
        Ok(self)
    }
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
    pub(super) color: u32,
    pub(super) normal: Vec3,
    /// Dropped when it faces away: block model faces, and the quads a block
    /// entity marks for culling as `entity.wgsl` does (`CULL_BACK`,
    /// `SHADED_CULL_W`). Other quads draw both sides, as in the world.
    pub(super) cull: bool,
    pub(super) shade: Shade,
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
pub(super) fn lit(color: u32, light: u32) -> u32 {
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

/// The normal a drawn quad faces along: its corners', else the one it was submitted with.
fn quad_normal(corners: [Vec3; 4], submitted: [i8; 4]) -> Vec3 {
    let normal = (corners[1] - corners[0])
        .cross(corners[2] - corners[0])
        .normalize_or_zero();
    if normal != Vec3::ZERO {
        return normal;
    }
    let [nx, ny, nz, _] = submitted;
    Vec3::new(f32::from(nx), f32::from(ny), f32::from(nz)).normalize_or_zero()
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
                world
                    .block_entities
                    .insert((b.pos[0], b.pos[1], b.pos[2]), entity);
            }
        }
        world
    }

    /// The colour (ABGR) packed light `light` gives, as the client's lightmap lights the
    /// world this frame: block light in bits 8..16 and sky light in bits 0..8, each 16
    /// times its level. Full bright is white, so a scene that never sets a light draws as
    /// it did. Without a renderer (a test) it is the lightmap's grey curve.
    pub(super) fn scene_light(&self, light: u32) -> u32 {
        if light == SCENE_FULL_BRIGHT {
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

    /// The fluid surfaces of culling group `group`'s blocks, meshed as the world meshes them
    /// against that group's own blocks (air elsewhere), before its pose and light.
    fn scene_group_fluids(
        &self,
        blocks: &SceneBlocks<'_, '_>,
        group: usize,
        look: SceneLook,
        state_at: &dyn Fn([i32; 3]) -> u32,
        quads: &mut Vec<SceneQuad>,
    ) {
        let cells: Vec<[i32; 3]> = blocks
            .iter()
            .filter(|(_, g, _)| *g == group)
            .map(|(block, ..)| block.pos)
            .collect();
        let water = look.biome.map_or(DEFAULT_WATER, |id| {
            self.res.biomes().get(id).water & 0xFF_FFFF
        });
        let fluid = horizon_client::mesher::scene_fluids(
            &self.pack.context.tables,
            &self.pack.context.part_tables,
            &cells,
            state_at,
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
            let corners = q.each_ref().map(|v| Vec3::from_array(v.pos));
            quads.push(SceneQuad {
                corners,
                uv: q.each_ref().map(|v| {
                    let [u, w] = v.uv();
                    [
                        sprite.u0 + u * (sprite.u1 - sprite.u0),
                        sprite.v0 + w * (sprite.v1 - sprite.v0),
                    ]
                }),
                color: q[0].color,
                normal: quad_normal(corners, [0; 4]),
                cull: true,
                shade: Shade::Normal,
            });
        }
    }

    /// The game's own block entities of culling group `group` (chests, signs, banners…),
    /// drawn by their renderers as in the world, full bright, against a small level of the
    /// group's blocks with the saved data each gives, before its pose and light. Nothing
    /// ticks. What moves with time (a banner's sway, a gateway's beam) follows `clock`, the
    /// scene's tick and partial tick, when it has one, and the client's otherwise. A mod's
    /// block entity keeps its own path.
    fn scene_group_entities(
        &self,
        batch: &mut HudBatch,
        blocks: &SceneBlocks<'_, '_>,
        group: usize,
        clock: Option<(u64, f32)>,
        quads: &mut Vec<SceneQuad>,
    ) {
        let renderer = self.res.block_entity_render();
        let members = || {
            blocks
                .iter()
                .filter(|(_, g, _)| *g == group)
                .map(|(block, ..)| block)
        };
        let drawn = || members().filter(|b| b.entity.is_none() && renderer.renders(b.state.0));
        if drawn().next().is_none() {
            return;
        }
        let world = self.scene_entity_level(members());
        let (partial, game_time) = match clock {
            Some((tick, partial)) => (partial, i64::try_from(tick).unwrap_or(i64::MAX)),
            None => (self.clock.fraction() as f32, self.world.game_time),
        };
        let sign = |p: [i32; 3]| match world.block_entities.get(&(p[0], p[1], p[2])) {
            Some(BlockEntity::Sign(sign)) => Some(sign),
            _ => None,
        };
        let banner = |p: [i32; 3], visit: &mut dyn FnMut(&str, u32)| {
            if let Some(BlockEntity::Banner(b)) = world.block_entities.get(&(p[0], p[1], p[2])) {
                let banners = &self.stations().banners;
                for layer in b.layers.iter().take(16) {
                    visit(
                        &banners.layer_texture_cow(layer),
                        horizon_client::banner::dye_rgb(&layer.color),
                    );
                }
            }
        };
        let scene = horizon_client::block_entity_render::Scene {
            camera: glam::DVec3::ZERO,
            spyglass_outline: false,
            orientation: Quat::IDENTITY,
            anim: &|_, _| horizon_client::block_entity_render::BlockAnim::default(),
            texts: None,
            partial,
            game_time,
            max_y: world.shape.max_y() - 1,
            world: &world,
            sprites: &self.gui.atlas.sprites,
            emissive: &self.gui.atlas.emissive,
            translucent_sprites: &self.gui.atlas.translucent,
            atlas_revision: self.gui.atlas.revision,
            nether: false,
            lid: &|_: [i32; 3]| 0.0,
            sign: &sign,
            sign_revision: None,
            banner: &banner,
            skull: &|_| None,
            glyph: &|c| self.gui.atlas.glyph_cell(c),
            glyphs: self.gui.atlas.font.epoch(),
            font: Some(&self.gui.atlas.font),
            dynamic_visible: None,
        };
        let mut placed: Vec<Vertex> = Vec::new();
        for block in drawn() {
            placed.clear();
            let at = Vec3::from_array(block.pos.map(|v| v as f32));
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
                let corners = q.each_ref().map(|v| Vec3::from_array(v.pos));
                // `entity.wgsl`: |w| >= 0.5 is shaded by the normal; smaller keeps the
                // colour baked in.
                let unbaked = q[0].normal[3].unsigned_abs() >= 64;
                quads.push(SceneQuad {
                    corners,
                    uv: q.each_ref().map(|v| v.uv),
                    color: q[0].color,
                    normal: quad_normal(corners, q[0].normal),
                    cull: horizon_client::mesher::culls_back(q[0].normal),
                    shade: if unbaked { Shade::Normal } else { Shade::Baked },
                });
            }
        }
    }

    /// Draws `blocks`, a scene of block states at positions of its own (not
    /// the world's), into the box `rect` (x0, y0, x1, y1, GUI pixels) of a
    /// screen's `batch`, seen as `view` says: Create's Ponder scenes. Each
    /// block is drawn with its model, full bright and shaded by face, a face
    /// against a solid neighbour in the scene left out; a block with an
    /// `entity` (a mod's) is drawn by `native_block_entity_submit` as in the
    /// world too. Its animation follows the scene clock for a kept ticking copy,
    /// and otherwise the client clock. `frame.look` and each section's light and drawn
    /// flag change that as `SceneParts` says. What lies outside the box is cut off, and
    /// the scene has a depth of its own, so
    /// it never covers the screen's other drawing or is covered by it but in
    /// the order drawn. `extra` adds quads in the scene's coordinates before
    /// the scene is seen and sorted (a living scene's entities and particles,
    /// `native_scene_live`). Nothing is drawn when a quad seen through the
    /// view isn't finite. API 422.
    pub(super) fn draw_scene_quads(
        &self,
        batch: &mut HudBatch,
        blocks: &SceneBlocks<'_, '_>,
        frame: SceneFrame,
        animation: Option<(u64, f32, &[LiveBlockEntity])>,
        extra: impl FnOnce(&mut Vec<SceneQuad>),
    ) -> Result<(), mod_api::SceneError> {
        let SceneFrame {
            rect,
            view,
            look,
            matrix: m,
            projected: mut seen,
            overlays,
            ..
        } = frame;
        if !self.native_window() {
            return Err(mod_api::SceneError::Unavailable);
        }
        if !view.scale.is_finite() || view.scale <= 0.0 {
            return Err(mod_api::SceneError::BadView);
        }
        // The Game boundary prepared every state before any drawing began.
        let occupied: std::collections::HashMap<_, _> = blocks
            .iter()
            .map(|(block, group, _)| ((group, block.pos), block.state))
            .collect();
        let solid = |group, pos| {
            occupied.get(&(group, pos)).is_some_and(|&state| {
                self.res.behaviours().facts(state).flags & state_flags::SOLID_RENDER != 0
            })
        };
        let grass = self.pack.item_renderer.grass_color(0.5, 1.0) as u32;
        let tables = &self.pack.context.tables;
        let tint = |state, index| -> Option<u32> {
            use mod_api::client::mesher::TintSource;
            let Some(id) = look.biome else {
                return tables.default_tint(state, index, grass);
            };
            let biomes = self.res.biomes();
            let info = biomes.get(id);
            Some(
                match tables.tint_source(state, index)? {
                    TintSource::Grass => biomes.grass(id, 0.0, 0.0),
                    TintSource::Foliage => info.foliage,
                    TintSource::DryFoliage => info.dry_foliage,
                    TintSource::Water => info.water,
                    TintSource::Fixed(c) => c,
                    _ => return tables.default_tint(state, index, grass),
                } & 0xFF_FFFF,
            )
        };
        let in_view_shading = matches!(look.shading, SceneShading::View { .. });
        let mut quads = Vec::new();
        let (tick, partial, kept) = animation.unwrap_or((0, 0.0, &[]));
        let mut kept = kept.iter().peekable();
        for (i, (block, group, pose)) in blocks.iter().enumerate() {
            let transform = pose.map(|pose| &pose.transform);
            let ticking = if kept.peek().is_some_and(|entry| entry.index == i) {
                kept.next().and_then(|entry| entry.ticking.as_ref())
            } else {
                None
            };
            if pose.is_some_and(|pose| !pose.drawn) {
                continue;
            }
            let packed = pose.map_or(SCENE_FULL_BRIGHT, |pose| pose.light);
            let start = quads.len();
            let point = |pos| transform.map_or(pos, |pose| pose.transform_point3(pos));
            let direction =
                |normal| transform.map_or(normal, |pose| pose.transform_vector3(normal));
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
                    if solid(
                        group,
                        [block.pos[0] + dx, block.pos[1] + dy, block.pos[2] + dz],
                    ) {
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
                let normal = direction(Vec3::from_array(facing.map(|v| v as f32)));
                // Face shading is baked here, before the tint, as scenes always drew it. View
                // shading waits for the view.
                let (mut color, shade) = if !in_view_shading {
                    (shaded(0xFFFF_FFFF, face_shade(normal)), Shade::Baked)
                } else if face.quad.shade {
                    (0xFFFF_FFFF, Shade::Normal)
                } else {
                    (0xFFFF_FFFF, Shade::Unshaded)
                };
                if let Some(t) = tint(block.state, face.quad.tint_index) {
                    color = horizon_client::hud_layout::multiply(
                        color,
                        horizon_client::mobs::render::abgr(t),
                    );
                }
                let corners = face.quad.positions.map(|p| point(at + Vec3::from_array(p)));
                quads.push(SceneQuad {
                    corners,
                    uv: face.quad.uv.map(|uv| {
                        [
                            sprite.u0 + uv[0] * (sprite.u1 - sprite.u0),
                            sprite.v0 + uv[1] * (sprite.v1 - sprite.v0),
                        ]
                    }),
                    color,
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
                        color: 0xFFFF_FFFF,
                        normal,
                        cull: true,
                        shade: Shade::Baked,
                    });
                }
            }
            if let Some(seed) = block.entity {
                let original;
                let (entity, partial, game_time, animation_time) =
                    if let Some((_, entity)) = ticking {
                        (
                            entity,
                            partial,
                            i64::try_from(tick).unwrap_or(i64::MAX),
                            tick as f64 + f64::from(partial),
                        )
                    } else {
                        original =
                            mod_api::ModBlockEntity::new(seed.id.to_string(), seed.fields.clone());
                        (
                            &original,
                            self.clock.fraction() as f32,
                            self.world.game_time,
                            self.animation_time(),
                        )
                    };
                let name = &*self.res.facts().get(block.state).name;
                let mut draw = BlockEntityDraw::posed(
                    partial,
                    game_time,
                    packed,
                    block.pos,
                    Mat4::IDENTITY,
                    diffuse_lights(false),
                    Vec::new(),
                )
                .animated(animation_time);
                self.native_block_entity_submit(block.pos, name, block.state, entity, &mut draw);
                // Its text's new glyphs upload with this screen's batch.
                batch.patches.append(&mut self.text_patches.borrow_mut());
                let time = draw.animation();
                let (drawn, spins) = draw.into_turning();
                let mut placed: Vec<Vertex> = Vec::with_capacity(drawn.len());
                place_turning(&drawn, &spins, time, at, &mut placed);
                for q in placed.as_chunks::<4>().0 {
                    let corners = q.each_ref().map(|v| Vec3::from_array(v.pos));
                    let normal = direction(quad_normal(corners, q[0].normal));
                    // `entity.wgsl`: |w| >= 0.5 is shaded by the normal; smaller
                    // keeps the colour baked in (quads, cubes, items, text).
                    let unbaked = q[0].normal[3].unsigned_abs() >= 64;
                    quads.push(SceneQuad {
                        corners: corners.map(point),
                        uv: q.each_ref().map(|v| v.uv),
                        color: q[0].color,
                        normal,
                        cull: horizon_client::mesher::culls_back(q[0].normal),
                        shade: if unbaked { Shade::Normal } else { Shade::Baked },
                    });
                }
            }
            if packed != SCENE_FULL_BRIGHT {
                let light = self.scene_light(packed);
                for q in &mut quads[start..] {
                    q.color = lit(q.color, light);
                }
            }
        }
        let air = self.res.facts().air.0;
        for group in 0..=blocks.poses.len() {
            let pose = blocks.pose(group);
            if pose.is_some_and(|pose| !pose.drawn) {
                continue;
            }
            let start = quads.len();
            if look.fluids {
                let state_at = |p| occupied.get(&(group, p)).map_or(air, |s| s.0);
                self.scene_group_fluids(blocks, group, look, &state_at, &mut quads);
            }
            if look.game_block_entities {
                self.scene_group_entities(batch, blocks, group, look.clock, &mut quads);
            }
            let Some(pose) = pose else {
                continue;
            };
            let light = self.scene_light(pose.light);
            for q in &mut quads[start..] {
                q.corners = q.corners.map(|c| pose.transform.transform_point3(c));
                q.normal = pose.transform.transform_vector3(q.normal);
                q.color = lit(q.color, light);
            }
        }
        extra(&mut quads);
        // A normal in the view's frame: x right, y up (the GUI's y runs down), z toward the
        // viewer.
        let in_view = |n: Vec3| {
            let v = m.transform_vector3(n);
            Vec3::new(v.x, -v.y, v.z).normalize_or_zero()
        };
        let shaded_color = |q: &SceneQuad| match (q.shade, look.shading) {
            (Shade::Baked, _) => q.color,
            (Shade::Normal, SceneShading::View { lights }) => {
                shaded(q.color, view_shade(in_view(q.normal), lights))
            }
            (Shade::Unshaded, SceneShading::View { lights }) => {
                shaded(q.color, view_shade(Vec3::Y, lights))
            }
            _ => shaded(q.color, face_shade(q.normal)),
        };
        let projected = quads
            .into_iter()
            .filter(|q| !q.cull || m.transform_vector3(q.normal).z > 0.0)
            .map(|q| ProjectedQuad::new(m, q.corners, q.uv, [shaded_color(&q); 4]));
        let textured_count = seen.len();
        if textured_count == 0 {
            seen = projected.collect();
        } else {
            seen.extend(projected);
        }
        // Textured faces were checked before the live scene could advance.
        if !seen[textured_count..].iter().all(ProjectedQuad::finite) {
            return Err(SceneError::BadView);
        }
        // Far to near, for the faces that let others show through.
        seen.sort_by(|a, b| a.depth.total_cmp(&b.depth));
        let start = batch.vertices.len();
        for face in seen {
            batch.quad_3d_colors(face.corners, face.uv, face.colors);
        }
        self.finish_scene(batch, start, rect, &overlays);
        Ok(())
    }

    /// Submits the scene's depth and clip, drawing `overlays` after its geometry
    /// in the same isolated depth pass.
    fn finish_scene(
        &self,
        batch: &mut HudBatch,
        start: usize,
        rect: [f32; 4],
        overlays: &[ProjectedOverlay],
    ) {
        if overlays.is_empty() {
            batch.depth_since(start, rect);
            return;
        }
        let overlay_start = batch.vertices.len();
        let uv = horizon_client::gfx::lightning::white_uv(&self.gui.atlas.font.white());
        for overlay in overlays {
            batch.quad_3d(overlay.corners, [uv; 4], overlay.color);
        }
        let runs = overlays
            .chunk_by(|a, b| a.depth == b.depth)
            .scan(overlay_start, |end, run| {
                *end += run.len() * 6;
                Some((*end, run[0].depth))
            });
        batch.depth_overlays_since(start, overlay_start, rect, runs);
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

    /// A Java GUI pose stack that turns a scene about x by `a`, then about y by `b`, flips
    /// it for GUI rendering (`scale(1, -1, 1)`) and scales it by `s` is the same view with
    /// `pitch = -a` and `yaw = b + 180`. A constant screen shift on top of it is the same
    /// view with its centre moved by the shift seen back through the view.
    #[test]
    fn a_pose_stack_turned_x_then_y_and_flipped_is_a_view() {
        let rect = [10.0, 20.0, 410.0, 270.0];
        let middle = Vec3::new(210.0, 145.0, 0.0);
        let center = Vec3::new(2.5, 1.0, 2.5);
        let close = |a: Mat4, b: Mat4| {
            a.to_cols_array()
                .iter()
                .zip(b.to_cols_array())
                .all(|(x, y)| (x - y).abs() < 1e-3)
        };
        for a in [-35.0f32, -90.0, 0.0, 12.5, 60.0] {
            for b in [145.0f32, 0.0, -55.0, 90.0, 270.0] {
                for s in [30.0f32, 18.0, 45.0] {
                    let stack = Mat4::from_translation(middle)
                        * Mat4::from_rotation_x(a.to_radians())
                        * Mat4::from_rotation_y(b.to_radians())
                        * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
                        * Mat4::from_scale(Vec3::splat(s))
                        * Mat4::from_translation(-center);
                    let view = SceneView::new(center.to_array(), b + 180.0, -a, s);
                    let m = scene_matrix(rect, view);
                    assert!(close(m, stack), "a = {a}, b = {b}, s = {s}");

                    let shift = Mat4::from_rotation_x((-35.0f32).to_radians()).transform_vector3(
                        Mat4::from_rotation_y(55.0f32.to_radians())
                            .transform_vector3(Vec3::new(-40.0, 0.0, 0.0)),
                    );
                    let linear = glam::Mat3::from_mat4(m);
                    let moved = center - linear.inverse() * shift;
                    let slid =
                        scene_matrix(rect, SceneView::new(moved.to_array(), b + 180.0, -a, s));
                    assert!(
                        close(slid, Mat4::from_translation(shift) * stack),
                        "slid: a = {a}, b = {b}, s = {s}"
                    );
                }
            }
        }
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
        assert_eq!(
            view_shade(Vec3::NEG_Y, [[0.0, -1.0, 0.0]; 2]),
            1.0,
            "capped at 1"
        );
    }

    #[test]
    fn a_look_names_a_biome_the_game_has() {
        let plains = |name: &str| (name == "minecraft:plains").then_some(1);
        let named = |name| SceneParts::new().with_biome(mod_api::Identifier::parse(name).unwrap());
        let look = SceneLook::of(&named("minecraft:plains"), plains).unwrap();
        assert_eq!(look.biome, Some(1));
        assert_eq!(
            SceneLook::of(&named("minecraft:nowhere"), plains).err(),
            Some(ScenePartsError::BadBiome)
        );
        assert_eq!(
            SceneLook::of(&SceneParts::new(), plains).unwrap().biome,
            None
        );
    }

    #[test]
    fn each_entity_part_names_one_entity_of_the_scene_and_a_group_it_has() {
        use mod_api::SceneEntityPart as Part;
        let group = SceneGroup::new(SceneSection::new(0, 1, [0.0; 3], [0.0; 3], [0.0; 3]));
        let parts = |entities| {
            SceneParts::new()
                .with_groups(vec![group; 2])
                .with_entities(entities)
        };
        assert!(entity_places(&parts(Vec::new()), 3).unwrap().is_empty());
        let places =
            entity_places(&parts(vec![Part::new(2).in_group(1).aged(9), Part::new(0)]), 3).unwrap();
        let seen: Vec<_> = places.iter().map(|p| (p.group, p.age)).collect();
        assert_eq!(seen, [(None, None), (None, None), (Some(1), Some(9))]);
        for (entities, index) in [
            (vec![Part::new(0), Part::new(3)], 1),
            (vec![Part::new(0).in_group(2)], 0),
            (vec![Part::new(1), Part::new(0), Part::new(1).aged(2)], 2),
        ] {
            assert_eq!(
                entity_places(&parts(entities), 3).err(),
                Some(ScenePartsError::BadEntityPart { index })
            );
        }
    }

    #[test]
    fn block_parts_land_on_the_block_they_name() {
        let cell = |x| SceneBlock {
            pos: [x, 0, 0],
            state: horizon_world::registry::BlockStateId(1),
            entity: None,
            breaking: None,
            game_data: None,
        };
        let parts = SceneParts::new()
            .with_breaking(vec![(1, 4), (0, 10)])
            .with_game_block_entities(vec![(0, mod_api::Compound::new())]);
        let mut cells = [cell(0), cell(1)];
        apply_block_parts(&mut cells, &parts).unwrap();
        assert_eq!(cells.each_ref().map(|c| c.breaking), [None, Some(4)]);
        assert!(cells[0].game_data.is_some() && cells[1].game_data.is_none());
        for parts in [
            SceneParts::new().with_breaking(vec![(2, 0)]),
            SceneParts::new().with_game_block_entities(vec![(2, mod_api::Compound::new())]),
        ] {
            assert_eq!(
                apply_block_parts(&mut [cell(0), cell(1)], &parts),
                Err(ScenePartsError::NoBlock { index: 2 })
            );
        }
    }

    #[test]
    fn light_and_cracks() {
        assert_eq!(
            lit(0xFF80_4020, 0xFFFF_FFFF),
            0xFF80_4020,
            "full bright changes nothing"
        );
        assert_eq!(lit(0xFF80_4020, 0x0080_8080), 0xFF40_2010);
        // A crack lies on a face as the decal does: top seen from above, x across, z down.
        assert_eq!(crack_uv([0.25, 1.0, 0.75], [0, 1, 0]), [0.25, 0.75]);
        assert_eq!(crack_uv([0.25, 0.0, 0.0], [0, 0, 1]), [0.25, 1.0]);
        assert_eq!(crack_uv([1.0, 1.0, 0.25], [1, 0, 0]), [0.75, 0.0]);
    }
}
