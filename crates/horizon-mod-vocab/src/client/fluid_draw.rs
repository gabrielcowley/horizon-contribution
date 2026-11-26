//! Fluids drawn by block entity, mob and player submit hooks: still boxes
//! ([`BlockEntityDrawContext::fluid_box`](crate::BlockEntityDrawContext::fluid_box)) and
//! flowing streams ([`BlockEntityDrawContext::fluid_stream`](crate::BlockEntityDrawContext::fluid_stream)).

/// How [`BlockEntityDrawContext::fluid_box`](crate::BlockEntityDrawContext::fluid_box) draws a
/// still fluid: a machine's tank, a basin's contents.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct FluidBoxOptions {
    /// `0xAARRGGBB` drawn in place of the stack's own tint: the first answer of the fluid's
    /// parts to `fluid_tint`, else its look's (plain water's `0xFF3F76E4`, never the biome's
    /// water colour, and lava's white). `None` keeps the stack's.
    pub argb: Option<u32>,
    /// Whether the box's bottom face is drawn; the other five always are.
    pub render_bottom: bool,
    /// `TextureAtlasSprite.uvShrinkRatio`, `0.0..=1.0`: how far each face's UVs pull in towards
    /// the sprite's centre. A port passes its sprite's own, 4 over the largest dimension of the
    /// atlas it was stitched into; Horizon keeps block sprites in arrays, so it asks for it.
    pub uv_shrink_ratio: f32,
    /// Whether a gas (a fluid declared `lighter_than_air`) is drawn upside down, turned half
    /// round the box's x axis through its centre: catnip's `invertGasses`, which Create's tanks
    /// and spouts pass and its basins don't. A gas with `render_bottom` false then leaves out
    /// its top face. Other fluids ignore it.
    pub invert_gases: bool,
}

impl FluidBoxOptions {
    /// The stack's own tint, `render_bottom` and `uv_shrink_ratio`. The draw call checks the ratio.
    #[inline]
    pub fn new(render_bottom: bool, uv_shrink_ratio: f32) -> Self {
        Self {
            argb: None,
            render_bottom,
            uv_shrink_ratio,
            invert_gases: false,
        }
    }

    /// The same options, a gas drawn upside down (see [`invert_gases`](Self::invert_gases)).
    #[inline]
    pub fn with_invert_gases(self) -> Self {
        Self {
            invert_gases: true,
            ..self
        }
    }

    /// The same options drawn in `argb` (`0xAARRGGBB`) instead of the stack's tint.
    #[inline]
    pub fn with_argb(self, argb: u32) -> Self {
        Self {
            argb: Some(argb),
            ..self
        }
    }
}

/// Why [`BlockEntityDrawContext::fluid_box`](crate::BlockEntityDrawContext::fluid_box) or
/// [`fluid_stream`](crate::BlockEntityDrawContext::fluid_stream) drew nothing. A refused draw
/// appends no geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub enum FluidBoxError {
    /// The stack's fluid is unresolved: its key names no registered fluid.
    UnsupportedFluid,
    /// The loaded resources have no sprite the fluid draws with: its still sprite for a box,
    /// its flowing sprite (or its still one, for a stream's cap) for a stream.
    MissingSprite,
    /// A bound is not finite or beyond 1,048,576 blocks, a minimum exceeds its maximum, the
    /// box would tile into more than 65,536 quads, or the shrink ratio is outside `0.0..=1.0`.
    /// For a stream: a radius outside `0.0..=1.0` or a progress that is not finite.
    InvalidBox,
}
