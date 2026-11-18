//! Standalone models drawn with the terrain's own materials by block entity, mob and player
//! submit hooks ([`BlockEntityDrawContext::terrain_model`](crate::BlockEntityDrawContext::terrain_model)).

/// Which terrain material a [`TerrainModelOptions`] draw uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub enum TerrainMaterial {
    /// Each quad's own: solid, `cutout` or `cutout_mipped` as its model's `render_type` names
    /// it. A translucent quad, or a render type the game doesn't know, refuses the whole draw.
    #[default]
    Model,
    /// Opaque, with no alpha test.
    Solid,
    /// Texels under alpha 0.1 discarded, sampled without mipmaps.
    Cutout,
    /// Texels under alpha 0.5 discarded, sampled with mipmaps.
    CutoutMipped,
}

/// How a [`TerrainModelOptions`] draw shades faces by their direction. Light never changes it:
/// a full-bright model stays shaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub enum TerrainShade {
    /// Each quad's own `shade` flag, as its model bakes it.
    #[default]
    Model,
    /// No directional shading.
    Off,
    /// Every quad shaded by the level's two entity light directions.
    Entity,
}

/// A [`TerrainUvShift`] that keeps moving by itself on the client's animation clock: a belt's
/// or a conveyor's texture, a flame, a scrolling screen. On each axis the shifted UVs move by
/// `fract(rate * animation_time + phase) * span` sprite units after the shift's own `scale` and
/// `offset`: a sawtooth that runs from `0` to `span` and jumps back, as Flywheel's
/// `scrolling` material does. `animation_time` is the clock
/// [`BlockEntityDrawContext::spin`](crate::BlockEntityDrawContext::spin) turns by, in ticks with
/// the partial tick, so a scroll pauses and freezes with it, and a
/// [`keep`](crate::BlockEntityDrawContext::keep) model goes on scrolling with no new submit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct UvScroll {
    /// Cycles per tick of the animation clock, per axis (`u`, `v`); negative runs backwards.
    pub rate: [f32; 2],
    /// Where in its cycle each axis is at animation time 0, in cycles.
    pub phase: [f32; 2],
    /// How far one cycle moves the UVs, in sprite units (1 is the whole sprite).
    pub span: [f32; 2],
}

impl UvScroll {
    /// A scroll of `rate` cycles a tick, starting at `phase`, over `span` sprite units.
    #[inline]
    pub fn new(rate: [f32; 2], phase: [f32; 2], span: [f32; 2]) -> Self {
        Self { rate, phase, span }
    }
    /// The UV offset at animation time `time`: `fract(rate * time + phase) * span` per axis.
    /// Counted in `f64`: a long session has more ticks than an `f32` holds exactly.
    pub fn offset_at(&self, time: f64) -> [f32; 2] {
        std::array::from_fn(|i| {
            let cycle = f64::from(self.rate[i]) * time + f64::from(self.phase[i]);
            ((cycle - cycle.floor()) * f64::from(self.span[i])) as f32
        })
    }
    /// Whether every value is finite, as a draw requires.
    pub fn is_finite(&self) -> bool {
        self.rate
            .iter()
            .chain(&self.phase)
            .chain(&self.span)
            .all(|v| v.is_finite())
    }
}

// no wire: borrowed call-scoped view; transport uses owned snapshots rather than serializing references.
/// Faces textured by `source_sprite` drawn with `target_sprite` instead, their unit UVs
/// multiplied by `scale` then moved by `offset`: a belt's or a scrolling screen's texture.
/// Only faces of that exact sprite change. With a [`scroll`](Self::scroll) they keep moving
/// on the animation clock.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct TerrainUvShift<'a> {
    /// The sprite the model's faces name (`mymod:block/belt`).
    pub source_sprite: &'a str,
    /// The sprite drawn in its place (`mymod:block/belt_scroll`).
    pub target_sprite: &'a str,
    /// Multiplies each UV, in sprite units (1 is the whole sprite).
    pub scale: [f32; 2],
    /// Added to each UV after `scale`, in sprite units.
    pub offset: [f32; 2],
    /// Moves the shifted UVs further on the animation clock, each frame; `None` holds them.
    pub scroll: Option<UvScroll>,
}

impl<'a> TerrainUvShift<'a> {
    /// Faces of `source_sprite` drawn with `target_sprite`, UVs times `scale` plus `offset`.
    #[inline]
    pub fn new(
        source_sprite: &'a str,
        target_sprite: &'a str,
        scale: [f32; 2],
        offset: [f32; 2],
    ) -> Self {
        Self {
            source_sprite,
            target_sprite,
            scale,
            offset,
            scroll: None,
        }
    }
    /// The same shift, scrolling by `scroll` on the animation clock.
    #[inline]
    pub fn with_scroll(self, scroll: UvScroll) -> Self {
        Self {
            scroll: Some(scroll),
            ..self
        }
    }
}

// no wire: borrowed call-scoped view; transport uses owned snapshots rather than serializing references.
/// How [`BlockEntityDrawContext::terrain_model`](crate::BlockEntityDrawContext::terrain_model)
/// draws a model. The default is the model's own materials and shading, its baked UVs and no
/// tint.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct TerrainModelOptions<'a> {
    /// The material every quad uses, or each quad's own.
    pub material: TerrainMaterial,
    /// Directional shading.
    pub shade: TerrainShade,
    /// One sprite drawn as another, with its UVs moved.
    pub uv_shift: Option<TerrainUvShift<'a>>,
    /// How far each face's UVs pull in towards their centre before the shift, `0.0..=1.0`:
    /// `0.0` keeps the baked UVs. A model baked by an older `FaceBakery` passes the atlas shrink
    /// ratio it was made with; a loader that doesn't shrink keeps `0.0`.
    pub uv_shrink_ratio: f32,
    /// `0xRRGGBB` multiplied into quads with a tint index; other quads keep their colour.
    pub tint: u32,
}

impl Default for TerrainModelOptions<'_> {
    fn default() -> Self {
        Self {
            material: TerrainMaterial::Model,
            shade: TerrainShade::Model,
            uv_shift: None,
            uv_shrink_ratio: 0.0,
            tint: 0xFF_FFFF,
        }
    }
}

impl<'a> TerrainModelOptions<'a> {
    /// The same options with every quad in `material`.
    #[inline]
    pub fn with_material(self, material: TerrainMaterial) -> Self {
        Self { material, ..self }
    }
    /// The same options shaded by `shade`.
    #[inline]
    pub fn with_shade(self, shade: TerrainShade) -> Self {
        Self { shade, ..self }
    }
    /// The same options with `shift` applied.
    #[inline]
    pub fn with_uv_shift(self, shift: TerrainUvShift<'a>) -> Self {
        Self {
            uv_shift: Some(shift),
            ..self
        }
    }
    /// The same options with UVs shrunk by `ratio`. The draw call checks it.
    #[inline]
    pub fn with_uv_shrink_ratio(self, ratio: f32) -> Self {
        Self {
            uv_shrink_ratio: ratio,
            ..self
        }
    }
    /// The same options with tinted quads multiplied by `0xRRGGBB` `tint`.
    #[inline]
    pub fn with_tint(self, tint: u32) -> Self {
        Self { tint, ..self }
    }
}

/// Why [`BlockEntityDrawContext::terrain_model`](crate::BlockEntityDrawContext::terrain_model)
/// drew nothing. A refused draw appends no geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub enum TerrainModelError {
    /// The handle is stale or not a resolved standalone model.
    Model(crate::ModelError),
    /// A quad could not be drawn, so none was: under [`TerrainMaterial::Model`] a translucent
    /// quad or an unknown render type; a sprite missing from the block sprites; a pose or UV
    /// that is not finite, or a pose that flattens the model; a shrink ratio outside
    /// `0.0..=1.0`; or a shift whose scale, offset or scroll is not finite, whose sprites are
    /// unknown, or whose source sprite no face of the model uses.
    Refused,
}

#[cfg(test)]
mod tests {
    use super::UvScroll;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
    }

    /// `fract(rate * t + phase) * span` per axis, Flywheel's `scrolling` sawtooth.
    #[test]
    fn the_offset_is_the_sawtooth_of_rate_time_and_phase_times_the_span() {
        let scroll = UvScroll::new([0.25, 0.1], [0.5, 0.0], [0.5, 0.375]);
        assert!(close(scroll.offset_at(0.0), [0.25, 0.0]));
        assert!(close(scroll.offset_at(1.0), [0.375, 0.0375]));
        // 0.25 * 2 + 0.5 = 1.0 wraps to 0; 0.1 * 2 = 0.2.
        assert!(close(scroll.offset_at(2.0), [0.0, 0.075]));
        assert!(close(scroll.offset_at(13.0), [0.375, 0.1125]));
    }

    /// A negative rate runs backwards and still stays in `0..span`, as GLSL's `fract` does.
    #[test]
    fn a_negative_rate_runs_backwards_within_the_span() {
        let scroll = UvScroll::new([0.0, -0.25], [0.0; 2], [1.0, 0.5]);
        assert!(close(scroll.offset_at(1.0), [0.0, 0.375]));
        assert!(close(scroll.offset_at(3.0), [0.0, 0.125]));
        for t in 0..200 {
            let [u, v] = scroll.offset_at(f64::from(t) * 0.37);
            assert!(u == 0.0 && (0.0..0.5).contains(&v), "{t}: {v}");
        }
    }

    /// Late in a long session the clock has more ticks than an `f32` holds exactly; the offset
    /// is still the one a short session has at the same point of the cycle.
    #[test]
    fn a_long_sessions_late_clock_is_no_less_exact() {
        let scroll = UvScroll::new([1.0 / 64.0, 1.0 / 32.0], [0.0; 2], [0.5; 2]);
        let late = 64.0 * 62_500_000.0 + 1.5;
        assert!(close(scroll.offset_at(late), scroll.offset_at(1.5)));
    }

    #[test]
    fn only_finite_scrolls_are_finite() {
        assert!(UvScroll::default().is_finite());
        assert!(!UvScroll::new([f32::NAN, 0.0], [0.0; 2], [1.0; 2]).is_finite());
        assert!(!UvScroll::new([0.0; 2], [0.0; 2], [f32::INFINITY, 1.0]).is_finite());
    }
}
