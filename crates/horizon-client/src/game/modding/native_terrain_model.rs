//! Standalone models under a block entity's pose with the terrain's own materials
//! (`BlockEntityDrawContext::terrain_model`): explicit solid/cutout/cutout-mipped selection,
//! directional shading, UV shift (scrolling on the animation clock) and shrink, and tint.
use super::{Game, native_block_entity_draw::BlockEntityDraw};
use horizon_client::{
    models::catalog::BakedFace,
    terrain_model::{
        NativeModelMaterial, NativeModelRenderOptions, NativeModelShade, NativeModelUvShift,
        draw_model_marked,
    },
};
use mod_api::{
    ModelError, ModelRef, TerrainMaterial, TerrainModelError, TerrainModelOptions, TerrainShade,
};

impl Game<'_> {
    /// Draws the resolved standalone `model` under `draw`'s pose from the block sprite arrays.
    /// See `BlockEntityDrawContext::terrain_model`.
    pub(super) fn native_draw_terrain_model(
        &self,
        draw: &mut BlockEntityDraw,
        model: ModelRef,
        options: TerrainModelOptions<'_>,
    ) -> Result<(), TerrainModelError> {
        use mod_api::__private::game::Fields;
        let fields = model.fields();
        if fields.generation != self.standalone_geometry.generation() {
            return Err(TerrainModelError::Model(ModelError::StaleModel));
        }
        let model = self
            .standalone_geometry
            .instance_slot(fields.slot)
            .and_then(|_| self.pack.render.model_catalog.model_by_slot(fields.slot))
            .ok_or(TerrainModelError::Model(ModelError::UnknownModel))?;
        let light = self.native_draw_light(draw);
        let layers = &self.pack.render.sprite_layers;
        draw_terrain_model(
            draw,
            &model.faces,
            light,
            |name| {
                layers
                    .get(name)
                    .or_else(|| layers.get(name.strip_prefix("minecraft:")?))
                    .copied()
            },
            options,
        )
    }
}

/// The renderer's options for the API's.
fn engine_options(options: TerrainModelOptions<'_>) -> NativeModelRenderOptions<'_> {
    NativeModelRenderOptions {
        material: match options.material {
            TerrainMaterial::Model => NativeModelMaterial::Model,
            TerrainMaterial::Solid => NativeModelMaterial::Solid,
            TerrainMaterial::Cutout => NativeModelMaterial::Cutout,
            TerrainMaterial::CutoutMipped => NativeModelMaterial::CutoutMipped,
            _ => unreachable!("TerrainMaterial: unhandled variant"),
        },
        shade: match options.shade {
            TerrainShade::Model => NativeModelShade::Model,
            TerrainShade::Off => NativeModelShade::Off,
            TerrainShade::Entity => NativeModelShade::Entity,
            _ => unreachable!("TerrainShade: unhandled variant"),
        },
        uv_shift: options.uv_shift.map(|shift| NativeModelUvShift {
            source_sprite: shift.source_sprite,
            target_sprite: shift.target_sprite,
            scale: shift.scale,
            offset: shift.offset,
        }),
        uv_shrink_ratio: options.uv_shrink_ratio,
        tint: options.tint,
    }
}

/// Appends `faces` under `draw`'s pose at `light`, or appends nothing and returns `Refused` when
/// the renderer refuses the model or `options`. The faces a scrolling shift moves are recorded
/// as runs that scroll on the animation clock (`BlockEntityDraw::scroll_run`).
fn draw_terrain_model(
    draw: &mut BlockEntityDraw,
    faces: &[BakedFace],
    light: u32,
    sprite_layer: impl Fn(&str) -> Option<u32>,
    options: TerrainModelOptions<'_>,
) -> Result<(), TerrainModelError> {
    let scroll = options.uv_shift.and_then(|shift| shift.scroll);
    if scroll.is_some_and(|scroll| !scroll.is_finite()) {
        return Err(TerrainModelError::Refused);
    }
    let pose = draw.pose();
    let vertices = draw.vertices_mut();
    // Runs of shifted vertices, as (start, end).
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let drawn = draw_model_marked(
        faces,
        pose,
        light,
        engine_options(options),
        sprite_layer,
        |vertex, shifted| {
            let at = vertices.len();
            vertices.push(vertex);
            if scroll.is_some() && shifted {
                match runs.last_mut() {
                    Some((_, end)) if *end == at => *end = at + 1,
                    _ => runs.push((at, at + 1)),
                }
            }
        },
    );
    if !drawn {
        return Err(TerrainModelError::Refused);
    }
    if let Some(scroll) = scroll {
        for (start, end) in runs {
            draw.scroll_run(start, end, scroll);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::native_block_entity_draw::{Moving, place_turning};
    use super::*;
    use glam::{Mat4, Vec3};
    use horizon_assets::{
        bake::{RenderType, bake_flat_face},
        model::{Direction, Material},
    };
    use horizon_client::terrain_model::draw_model;
    use horizon_client::{
        block_entity_render::diffuse_lights,
        gfx::{
            TERRAIN_MODEL_CUTOUT_MIPPED_OVERLAY, TERRAIN_MODEL_CUTOUT_OVERLAY,
            TERRAIN_MODEL_SOLID_OVERLAY,
        },
        mesher::Vertex,
    };
    use mod_api::{TerrainUvShift, UvScroll};

    /// A full south face of `sprite` (layer 7) in `render_type`, tinted when `tint_index` >= 0.
    fn face(render_type: Option<RenderType>, tint_index: i32) -> BakedFace {
        let mut quad = bake_flat_face(
            [0.0; 3],
            [16.0; 3],
            [0.0, 0.0, 16.0, 16.0],
            Direction::South,
            Material {
                sprite: "test:casing".into(),
                force_translucent: false,
            },
            tint_index,
        )
        .unwrap();
        quad.render_type = render_type;
        BakedFace::new(quad, 7, true, false, None)
    }

    fn sprite(name: &str) -> Option<u32> {
        match name {
            "test:casing" => Some(7),
            "test:frame" => Some(9),
            "test:belt" => Some(11),
            _ => None,
        }
    }

    /// A full south face of `test:frame` (layer 9), which a shift of `test:casing` leaves be.
    fn frame_face() -> BakedFace {
        let mut face = face(None, -1);
        face.quad.material.sprite = "test:frame".into();
        face.layer = 9;
        face
    }

    /// The draw's frame after `terrain_model` with `options`.
    fn drawn_frame(
        faces: &[BakedFace],
        options: TerrainModelOptions<'_>,
    ) -> (Result<(), TerrainModelError>, BlockEntityDraw) {
        let mut draw = BlockEntityDraw::posed(
            0.0,
            0,
            0xF0F0,
            [0; 3],
            Mat4::IDENTITY,
            diffuse_lights(false),
            Vec::new(),
        );
        let result = draw_terrain_model(&mut draw, faces, 0xF0F0, sprite, options);
        (result, draw)
    }

    /// Each vertex's UV as `place_turning` puts it at animation time `time`.
    fn uvs_at(vertices: &[Vertex], moving: &Moving, time: f64) -> Vec<[f32; 2]> {
        let mut out = Vec::new();
        place_turning(vertices, moving, time, Vec3::ZERO, &mut out);
        out.iter().map(|v| v.uv).collect()
    }

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
    }

    /// Full bright, a block east of the camera.
    fn drawn(
        faces: &[BakedFace],
        options: TerrainModelOptions<'_>,
    ) -> (Result<(), TerrainModelError>, Vec<Vertex>) {
        let mut draw = BlockEntityDraw::posed(
            0.0,
            0,
            0xF0F0,
            [0; 3],
            Mat4::from_translation(Vec3::X),
            diffuse_lights(false),
            Vec::new(),
        );
        let result = draw_terrain_model(&mut draw, faces, 0xF0F0, sprite, options);
        (result, draw.into_vertices())
    }

    fn materials(vertices: &[Vertex]) -> Vec<u32> {
        vertices
            .iter()
            .step_by(4)
            .map(|v| v.layer & 0xFFFF)
            .collect()
    }

    #[test]
    fn each_quad_keeps_its_cutout_material_unless_one_is_chosen_for_all() {
        let faces = [
            face(None, -1),
            face(Some(RenderType::Cutout), -1),
            face(Some(RenderType::CutoutMipped), -1),
        ];
        let (result, vertices) = drawn(&faces, TerrainModelOptions::default());
        assert_eq!(result, Ok(()));
        assert_eq!(
            materials(&vertices),
            [
                TERRAIN_MODEL_SOLID_OVERLAY,
                TERRAIN_MODEL_CUTOUT_OVERLAY,
                TERRAIN_MODEL_CUTOUT_MIPPED_OVERLAY
            ]
        );
        assert!(vertices.iter().all(|v| v.layer >> 16 == 7));
        assert_eq!(vertices[0].pos[0], 1.0, "drawn under the pose");
        let (result, vertices) = drawn(
            &faces,
            TerrainModelOptions::default().with_material(TerrainMaterial::CutoutMipped),
        );
        assert_eq!(result, Ok(()));
        assert_eq!(
            materials(&vertices),
            [TERRAIN_MODEL_CUTOUT_MIPPED_OVERLAY; 3]
        );
    }

    #[test]
    fn a_translucent_quad_refuses_the_models_materials_but_not_a_chosen_one() {
        let faces = [face(None, -1), face(Some(RenderType::Translucent), -1)];
        let (result, vertices) = drawn(&faces, TerrainModelOptions::default());
        assert_eq!(result, Err(TerrainModelError::Refused));
        assert!(vertices.is_empty());
        let (result, vertices) = drawn(
            &faces,
            TerrainModelOptions::default().with_material(TerrainMaterial::Cutout),
        );
        assert_eq!(result, Ok(()));
        assert_eq!(materials(&vertices), [TERRAIN_MODEL_CUTOUT_OVERLAY; 2]);
    }

    #[test]
    fn shading_is_chosen_apart_from_full_bright_light() {
        let shaded =
            |vertices: &[Vertex]| vertices[0].glint_uv != [0.0; 2] || vertices[0].normal != [0; 4];
        // A loader quad that takes no directional shading.
        let flat = || {
            let mut face = face(None, -1);
            face.quad.shade = false;
            face
        };
        for (faces, shade, expected) in [
            ([face(None, -1)], TerrainShade::Model, true),
            ([flat()], TerrainShade::Model, false),
            ([face(None, -1)], TerrainShade::Off, false),
            ([flat()], TerrainShade::Entity, true),
        ] {
            let (result, vertices) =
                drawn(&faces, TerrainModelOptions::default().with_shade(shade));
            assert_eq!(result, Ok(()));
            assert_eq!(shaded(&vertices), expected, "{shade:?}");
            assert_eq!(vertices[0].light, 0xF0F0);
        }
    }

    #[test]
    fn the_tint_multiplies_only_quads_with_a_tint_index() {
        let (result, vertices) = drawn(
            &[face(None, 0), face(None, -1)],
            TerrainModelOptions::default().with_tint(0x80_4020),
        );
        assert_eq!(result, Ok(()));
        // Vertex colours are ABGR.
        assert_eq!(vertices[0].color, 0xFF20_4080);
        assert_eq!(vertices[4].color, u32::MAX);
    }

    #[test]
    fn a_shift_moves_its_sprite_and_bad_uv_options_refuse_the_draw() {
        let belt = TerrainUvShift::new("test:casing", "test:belt", [0.5; 2], [0.5, 0.0]);
        let (result, vertices) = drawn(
            &[face(None, -1)],
            TerrainModelOptions::default().with_uv_shift(belt),
        );
        assert_eq!(result, Ok(()));
        assert!(vertices.iter().all(|v| v.layer >> 16 == 11));
        assert!(vertices.iter().all(|v| v.uv[0] >= 0.5 && v.uv[1] <= 0.5));
        for options in [
            TerrainModelOptions::default().with_uv_shift(TerrainUvShift::new(
                "test:belt",
                "test:casing",
                [1.0; 2],
                [0.0; 2],
            )),
            TerrainModelOptions::default().with_uv_shift(TerrainUvShift::new(
                "test:casing",
                "test:missing",
                [1.0; 2],
                [0.0; 2],
            )),
            TerrainModelOptions::default().with_uv_shift(TerrainUvShift::new(
                "test:casing",
                "test:belt",
                [f32::NAN; 2],
                [0.0; 2],
            )),
            TerrainModelOptions::default().with_uv_shrink_ratio(1.5),
        ] {
            let (result, vertices) = drawn(&[face(None, -1)], options);
            assert_eq!(result, Err(TerrainModelError::Refused), "{options:?}");
            assert!(vertices.is_empty(), "{options:?}");
        }
    }

    /// A shift with no scroll draws exactly what the renderer's own `draw_model` draws, and
    /// nothing in the frame moves by itself.
    #[test]
    fn a_shift_without_a_scroll_draws_the_same_vertices_and_nothing_moves() {
        let faces = [face(None, -1), frame_face()];
        let shift = TerrainUvShift::new("test:casing", "test:belt", [1.0, 0.5], [0.0, 0.25]);
        let options = TerrainModelOptions::default().with_uv_shift(shift);
        let mut expected = Vec::new();
        assert!(draw_model(
            &faces,
            Mat4::IDENTITY,
            0xF0F0,
            engine_options(options),
            sprite,
            |vertex| expected.push(vertex),
        ));
        let (result, draw) = drawn_frame(&faces, options);
        assert_eq!(result, Ok(()));
        let (vertices, moving) = draw.into_turning();
        assert_eq!(
            bytemuck::cast_slice::<Vertex, u8>(&vertices),
            bytemuck::cast_slice::<Vertex, u8>(&expected)
        );
        assert!(moving.is_empty());
    }

    /// The shifted faces' UVs are `uv * scale + offset + fract(rate * t + phase) * span` at every
    /// animation time, wrapping at the span; the faces of other sprites never move.
    #[test]
    fn a_scrolling_shift_moves_only_its_faces_by_the_sawtooth_on_the_animation_clock() {
        // A belt's: V only, half the sprite a cycle, the bottom face half a cycle on.
        let rate = 64.0 / 504.0;
        let scroll = UvScroll::new([0.0, rate], [0.0, 0.5], [1.0, 0.5]);
        let shift = TerrainUvShift::new("test:casing", "test:belt", [1.0, 0.5], [0.0, 0.0])
            .with_scroll(scroll);
        let faces = [frame_face(), face(None, -1), frame_face()];
        let (result, draw) =
            drawn_frame(&faces, TerrainModelOptions::default().with_uv_shift(shift));
        assert_eq!(result, Ok(()));
        let (vertices, moving) = draw.into_turning();
        assert!(!moving.is_empty());
        let base: Vec<[f32; 2]> = vertices.iter().map(|v| v.uv).collect();
        for time in [0.0, 1.0, 2.5, 3.9375, 7.875, 1_000.25, 4_000_000_000.5] {
            let uvs = uvs_at(&vertices, &moving, time);
            let cycle = rate as f64 * time + 0.5;
            let expected = ((cycle - cycle.floor()) * 0.5) as f32;
            for (i, (uv, base)) in uvs.iter().zip(&base).enumerate() {
                let want = if (4..8).contains(&i) {
                    [base[0], base[1] + expected]
                } else {
                    *base
                };
                assert!(close(*uv, want), "t {time} vertex {i}: {uv:?} != {want:?}");
            }
        }
        // It wraps: one whole cycle later every UV is back where it was, and just before the
        // wrap the scroll is almost the whole span.
        let period = 504.0 / 64.0;
        let later = uvs_at(&vertices, &moving, 1.0 + period);
        for (a, b) in uvs_at(&vertices, &moving, 1.0).iter().zip(&later) {
            assert!(close(*a, *b), "{a:?} != {b:?}");
        }
        let late = uvs_at(&vertices, &moving, period / 2.0 - 1e-4);
        assert!(late[4][1] - base[4][1] > 0.499, "{:?}", late[4]);
        let wrapped = uvs_at(&vertices, &moving, period / 2.0);
        assert!(close(wrapped[4], base[4]), "{:?}", wrapped[4]);
    }

    /// A mob's frame has no animation clock: its scroll is held at its phase.
    #[test]
    fn a_frame_drawn_once_holds_its_scroll_at_its_phase() {
        let scroll = UvScroll::new([0.25, 0.5], [0.25, 0.5], [0.5, 0.5]);
        let shift =
            TerrainUvShift::new("test:casing", "test:belt", [1.0; 2], [0.0; 2]).with_scroll(scroll);
        let (_, plain) = drawn(
            &[face(None, -1)],
            TerrainModelOptions::default().with_uv_shift(TerrainUvShift::new(
                "test:casing",
                "test:belt",
                [1.0; 2],
                [0.0; 2],
            )),
        );
        let (result, scrolled) = drawn(
            &[face(None, -1)],
            TerrainModelOptions::default().with_uv_shift(shift),
        );
        assert_eq!(result, Ok(()));
        for (a, b) in scrolled.iter().zip(&plain) {
            assert!(close(a.uv, [b.uv[0] + 0.125, b.uv[1] + 0.25]), "{:?}", a.uv);
        }
    }

    #[test]
    fn a_scroll_that_is_not_finite_refuses_the_draw() {
        for bad in [
            UvScroll::new([f32::NAN, 0.0], [0.0; 2], [1.0; 2]),
            UvScroll::new([0.0; 2], [f32::INFINITY, 0.0], [1.0; 2]),
            UvScroll::new([0.0; 2], [0.0; 2], [1.0, f32::NEG_INFINITY]),
        ] {
            let shift = TerrainUvShift::new("test:casing", "test:belt", [1.0; 2], [0.0; 2])
                .with_scroll(bad);
            let (result, draw) = drawn_frame(
                &[face(None, -1)],
                TerrainModelOptions::default().with_uv_shift(shift),
            );
            assert_eq!(result, Err(TerrainModelError::Refused), "{bad:?}");
            let (vertices, moving) = draw.into_turning();
            assert!(vertices.is_empty() && moving.is_empty(), "{bad:?}");
        }
    }
}
