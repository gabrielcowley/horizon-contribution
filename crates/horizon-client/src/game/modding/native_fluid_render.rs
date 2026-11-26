//! Fluids under a block entity's pose: still boxes (`BlockEntityDrawContext::fluid_box`) and
//! streams (`BlockEntityDrawContext::fluid_stream`), drawn with their own sorted translucent
//! material (`FLUID_OVERLAY`).
use super::{Game, native_block_entity_draw::BlockEntityDraw};
use glam::Vec3;
use horizon_client::{
    fluid_models::{self, FluidDrawLook},
    fluid_render::{
        FluidVertex, NativeFluidRenderOptions, StreamSprite, fluid_box, fluid_light, fluid_stream,
    },
    gfx::FLUID_OVERLAY,
    mesher::Vertex,
};
use horizon_core::geometry::Direction;
use mod_api::{FluidBoxError, FluidBoxOptions, FluidStack};

/// What one stack draws with: its fluid's sprites, the stack's tint, and the light level and
/// gas flag of its fluid type.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::game) struct StackLook {
    pub(in crate::game) look: FluidDrawLook,
    /// `0xAARRGGBB`: what `fluid_tint` answered, else the look's.
    pub(in crate::game) tint: u32,
    pub(in crate::game) light_level: u8,
    pub(in crate::game) lighter_than_air: bool,
}

impl Game<'_> {
    /// Draws `stack` as a still fluid filling `bounds` under `draw`'s pose, sampling the fluid's
    /// still sprite from the block sprite arrays. See `BlockEntityDrawContext::fluid_box`.
    pub(super) fn native_draw_fluid_box(
        &self,
        draw: &mut BlockEntityDraw,
        stack: &FluidStack,
        bounds: [f32; 6],
        options: FluidBoxOptions,
    ) -> Result<(), FluidBoxError> {
        let Some(look) = self.stack_look(stack) else {
            return Ok(());
        };
        draw_fluid_box(
            draw,
            |s| self.fluid_sprite_layer(s),
            &look?,
            bounds,
            options,
        )
    }

    /// Draws `stack` as a stream through `direction`'s face under `draw`'s pose. See
    /// `BlockEntityDrawContext::fluid_stream`.
    pub(super) fn native_draw_fluid_stream(
        &self,
        draw: &mut BlockEntityDraw,
        stack: &FluidStack,
        direction: Direction,
        radius: f32,
        progress: f32,
        inbound: bool,
    ) -> Result<(), FluidBoxError> {
        let Some(look) = self.stack_look(stack) else {
            return Ok(());
        };
        draw_fluid_stream(
            draw,
            |s| self.fluid_sprite_layer(s),
            &look?,
            Stream {
                direction,
                radius,
                progress,
                inbound,
            },
        )
    }

    /// What `stack` draws with: `Err` for an unresolved fluid, `None` for an empty stack, which
    /// draws nothing and asks no part for its tint. Light and the gas flag are its fluid type's.
    pub(in crate::game) fn stack_look(
        &self,
        stack: &FluidStack,
    ) -> Option<Result<StackLook, FluidBoxError>> {
        stack_look(stack, |key| {
            self.fluid_type_properties(key)
                .map_or((0, false), |p| (p.light_level, p.lighter_than_air))
        })
    }

    /// The block sprite array layer of `sprite`, vanilla's named with or without its namespace.
    fn fluid_sprite_layer(&self, sprite: &str) -> Option<u32> {
        let layers = &self.pack.render.sprite_layers;
        layers
            .get(sprite)
            .or_else(|| layers.get(sprite.strip_prefix("minecraft:")?))
            .copied()
    }
}

/// What `stack` draws with, `kind` giving its fluid's light level and whether it is a gas.
fn stack_look(
    stack: &FluidStack,
    kind: impl FnOnce(mod_api::FluidKey) -> (u8, bool),
) -> Option<Result<StackLook, FluidBoxError>> {
    let Some((key, id)) = stack
        .key()
        .and_then(|key| Some((key, mod_api::__private::fluid_id(key)?)))
    else {
        return Some(Err(FluidBoxError::UnsupportedFluid));
    };
    if stack.amount == 0 {
        return None;
    }
    let look = fluid_models::draw_look(id);
    let tint = fluid_models::stack_tint(id, stack, &look);
    let (light_level, lighter_than_air) = kind(key);
    Some(Ok(StackLook {
        look,
        tint,
        light_level,
        lighter_than_air,
    }))
}

/// How a stack's vertices are written: under the draw's pose, in the stack's colour, at its light.
struct Brush {
    pose: glam::Mat4,
    /// ABGR, as vertex colours are.
    color: u32,
    light: u32,
}

impl Brush {
    fn new(draw: &BlockEntityDraw, look: &StackLook, argb: u32) -> Self {
        Self {
            pose: draw.pose(),
            color: (argb & 0xFF00_FF00) | ((argb >> 16) & 0xFF) | ((argb & 0xFF) << 16),
            light: fluid_light(draw.light(), look.light_level),
        }
    }

    fn vertex(&self, v: FluidVertex, layer: u32) -> Vertex {
        Vertex {
            pos: self
                .pose
                .transform_point3(Vec3::from_array(v.pos))
                .to_array(),
            uv: v.uv,
            layer: FLUID_OVERLAY,
            color: self.color,
            light: self.light,
            // The source TRANSLUCENT shader does not use normals for lighting.
            normal: [0; 4],
            glint_uv: [layer as f32, 0.0],
        }
    }
}

fn draw_fluid_box(
    draw: &mut BlockEntityDraw,
    sprite_layer: impl Fn(&str) -> Option<u32>,
    look: &StackLook,
    bounds: [f32; 6],
    options: FluidBoxOptions,
) -> Result<(), FluidBoxError> {
    let layer = sprite_layer(&look.look.still).ok_or(FluidBoxError::MissingSprite)?;
    let brush = Brush::new(draw, look, options.argb.unwrap_or(look.tint));
    let options = NativeFluidRenderOptions {
        argb: options.argb,
        render_bottom: options.render_bottom,
        uv_shrink_ratio: options.uv_shrink_ratio,
        invert: options.invert_gases && look.lighter_than_air,
    };
    let vertices = draw.vertices_mut();
    let drawn = fluid_box(bounds, options, |v| vertices.push(brush.vertex(v, layer)));
    drawn.then_some(()).ok_or(FluidBoxError::InvalidBox)
}

/// Where a stream runs and how far it has filled (`BlockEntityDrawContext::fluid_stream`).
struct Stream {
    direction: Direction,
    radius: f32,
    progress: f32,
    inbound: bool,
}

fn draw_fluid_stream(
    draw: &mut BlockEntityDraw,
    sprite_layer: impl Fn(&str) -> Option<u32>,
    look: &StackLook,
    stream: Stream,
) -> Result<(), FluidBoxError> {
    let flowing = sprite_layer(&look.look.flowing).ok_or(FluidBoxError::MissingSprite)?;
    // The still cap is drawn only while the stream fills.
    let still = if stream.progress == 1.0 {
        flowing
    } else {
        sprite_layer(&look.look.still).ok_or(FluidBoxError::MissingSprite)?
    };
    let brush = Brush::new(draw, look, look.tint);
    let vertices = draw.vertices_mut();
    let drawn = fluid_stream(
        stream.direction,
        stream.radius,
        stream.progress,
        stream.inbound,
        // A sprite's `uvShrinkRatio` keeps a stitched atlas's neighbours out; Horizon's block
        // sprites are array layers with none, so a stream (which takes no ratio) uses none.
        0.0,
        |v, sprite| {
            let layer = match sprite {
                StreamSprite::Flowing => flowing,
                StreamSprite::Still => still,
            };
            vertices.push(brush.vertex(v, layer));
        },
    );
    drawn.then_some(()).ok_or(FluidBoxError::InvalidBox)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Mat4;
    use horizon_client::block_entity_render::diffuse_lights;

    fn stack(fluid: &str, amount: u32) -> FluidStack {
        mod_platform::__private::part_tests::game_thread();
        FluidStack::new(mod_api::__private::fluid_key(fluid).unwrap(), amount)
    }

    /// A frame a block east of the camera, lit by sky 12 and block 3.
    fn frame() -> BlockEntityDraw {
        BlockEntityDraw::posed(
            0.0,
            0,
            0x30C0,
            [0; 3],
            Mat4::from_translation(Vec3::X),
            diffuse_lights(false),
            Vec::new(),
        )
    }

    /// A stack's look as the game resolves it, its fluid type giving `light` and `gas`.
    fn look(stack: &FluidStack, light: u8, gas: bool) -> StackLook {
        stack_look(stack, |_| (light, gas)).unwrap().unwrap()
    }

    /// Sprite layers by name: still sprites on 7, flowing ones on 8.
    fn layer(sprite: &str) -> Option<u32> {
        if sprite.ends_with("_still") {
            Some(7)
        } else if sprite.ends_with("_flow") {
            Some(8)
        } else {
            None
        }
    }

    fn drawn(
        look: &StackLook,
        bounds: [f32; 6],
        options: FluidBoxOptions,
    ) -> (Result<(), FluidBoxError>, Vec<Vertex>) {
        let mut draw = frame();
        let result = draw_fluid_box(&mut draw, layer, look, bounds, options);
        (result, draw.into_vertices())
    }

    #[test]
    fn water_tiles_under_the_pose_in_its_own_material_with_the_stack_tint() {
        let water = look(&stack("minecraft:water", 1000), 0, false);
        let (result, vertices) = drawn(
            &water,
            [0.0, 0.0, 0.0, 2.0, 1.0, 1.0],
            FluidBoxOptions::new(false, 0.0),
        );
        assert_eq!(result, Ok(()));
        // Two tiles on top, front, back; one on each end; the bottom left out.
        assert_eq!(vertices.len(), (2 + 2 + 2 + 1 + 1) * 4);
        assert!(vertices.iter().all(|v| v.layer == FLUID_OVERLAY
            && v.glint_uv == [7.0, 0.0]
            && v.color == 0xFFE4_763F
            && v.light == 0x30C0));
        // The pose moved the box a block east: x spans 1..=3.
        let xs = vertices.iter().map(|v| v.pos[0]);
        assert_eq!(xs.clone().fold(f32::MAX, f32::min), 1.0);
        assert_eq!(xs.fold(f32::MIN, f32::max), 3.0);
    }

    #[test]
    fn a_fluids_light_raises_block_light_and_an_explicit_tint_replaces_the_stack_tint() {
        let lava = look(&stack("minecraft:lava", 1), 15, false);
        let (result, vertices) = drawn(
            &lava,
            [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            FluidBoxOptions::new(true, 0.0).with_argb(0x8011_2233),
        );
        assert_eq!(result, Ok(()));
        assert_eq!(vertices.len(), 6 * 4);
        // Sky 12 kept, block light 3 raised to lava's 15.
        assert!(
            vertices
                .iter()
                .all(|v| v.light == 0xF0C0 && v.color == 0x8033_2211)
        );
    }

    /// Any resolved fluid draws, a stack with components among them: a mod's fluid no look names
    /// draws as water, in its own light. Only an unresolved fluid is refused, and an empty stack
    /// draws nothing without asking its fluid type or its parts.
    #[test]
    fn any_resolved_fluid_draws_with_its_components_and_only_an_unresolved_one_is_refused() {
        let with_components = {
            use horizon_core::nbt::{Compound, Tag};
            let mut components = Compound::new();
            components.insert(
                "minecraft:custom_data".into(),
                Tag::Compound(Compound::new()),
            );
            FluidStack {
                components: Some(Box::new(components.into())),
                ..stack("test:potion", 1000)
            }
        };
        let potion = look(&with_components, 4, false);
        assert_eq!(potion.look.still, "minecraft:block/water_still");
        assert_eq!(potion.tint, 0xFF3F_76E4);
        let (result, vertices) = drawn(
            &potion,
            [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            FluidBoxOptions::new(true, 0.0),
        );
        assert_eq!(result, Ok(()));
        assert!(vertices.iter().all(|v| v.light == 0x40C0));
        let unresolved = FluidStack {
            // A key no fluid is registered under.
            fluid: mod_api::FluidIdentity::Resolved(
                mod_api::__private::parts_installation::fluid_key(u32::MAX),
            ),
            ..stack("minecraft:water", 1000)
        };
        assert_eq!(
            stack_look(&unresolved, |_| unreachable!()),
            Some(Err(FluidBoxError::UnsupportedFluid))
        );
        assert_eq!(
            stack_look(&stack("minecraft:water", 0), |_| unreachable!()),
            None
        );
    }

    /// `invert_gases` turns only a gas upside down: its top face is then drawn at the floor of
    /// the box, facing down.
    #[test]
    fn only_a_gas_is_inverted_and_only_when_asked() {
        let bounds = [0.0, 0.0, 0.0, 1.0, 0.5, 1.0];
        let top_at_floor = |gas: bool, invert: bool| {
            let steam = look(&stack("test:potion", 1000), 0, gas);
            let options = FluidBoxOptions::new(false, 0.0);
            let options = if invert {
                options.with_invert_gases()
            } else {
                options
            };
            let (result, vertices) = drawn(&steam, bounds, options);
            assert_eq!(result, Ok(()));
            // Every horizontal face's corners share one height; is it the floor?
            let ys: Vec<f32> = vertices
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|q| q.iter().all(|v| v.pos[1] == q[0].pos[1]))
                .map(|q| q[0].pos[1])
                .collect();
            assert_eq!(ys.len(), 1, "one horizontal face");
            ys[0] == 0.0
        };
        assert!(top_at_floor(true, true));
        assert!(!top_at_floor(true, false));
        assert!(!top_at_floor(false, true));
    }

    #[test]
    fn refusals_append_nothing() {
        let water = look(&stack("minecraft:water", 1000), 0, false);
        let unit = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let options = FluidBoxOptions::new(true, 0.0);
        for (bounds, options) in [
            ([1.0, 0.0, 0.0, 0.0, 1.0, 1.0], options),
            ([0.0, 0.0, 0.0, f32::NAN, 1.0, 1.0], options),
            ([0.0, 0.0, 0.0, 1_000.0, 1_000.0, 1_000.0], options),
            (unit, FluidBoxOptions::new(true, 1.5)),
        ] {
            let (result, vertices) = drawn(&water, bounds, options);
            assert_eq!(
                result,
                Err(FluidBoxError::InvalidBox),
                "{bounds:?} {options:?}"
            );
            assert!(vertices.is_empty(), "{bounds:?} {options:?}");
        }
        let mut draw = frame();
        assert_eq!(
            draw_fluid_box(&mut draw, |_| None, &water, unit, options),
            Err(FluidBoxError::MissingSprite)
        );
        let stream = || Stream {
            direction: Direction::Down,
            radius: 0.25,
            progress: 0.5,
            inbound: false,
        };
        assert_eq!(
            draw_fluid_stream(
                &mut draw,
                |s| layer(s).filter(|&l| l == 8),
                &water,
                stream()
            ),
            Err(FluidBoxError::MissingSprite),
            "a filling stream needs its still sprite for the cap"
        );
        assert_eq!(
            draw_fluid_stream(
                &mut draw,
                layer,
                &water,
                Stream {
                    radius: 2.0,
                    ..stream()
                }
            ),
            Err(FluidBoxError::InvalidBox)
        );
        assert!(draw.into_vertices().is_empty());
    }

    /// A stream's sides sample the flowing sprite and its cap the still one, in the stack's
    /// tint and light, under the pose.
    #[test]
    fn a_stream_samples_its_flowing_sides_and_still_cap() {
        let water = look(&stack("minecraft:water", 1000), 0, false);
        let mut draw = frame();
        let stream = Stream {
            direction: Direction::Down,
            radius: 0.25,
            progress: 0.5,
            inbound: false,
        };
        assert_eq!(draw_fluid_stream(&mut draw, layer, &water, stream), Ok(()));
        let vertices = draw.into_vertices();
        let on = |layer: f32| vertices.iter().filter(|v| v.glint_uv[0] == layer).count();
        assert_eq!((on(8.0), on(7.0)), (4 * 2 * 4, 4 * 4));
        assert!(vertices.iter().all(|v| v.layer == FLUID_OVERLAY
            && v.color == 0xFFE4_763F
            && v.light == 0x30C0
            && (1.25..=1.75).contains(&v.pos[0])));
    }
}
