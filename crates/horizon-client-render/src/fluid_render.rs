// Still-fluid tiling follows Catnip/Ponder 1.0.82 FluidRenderHelper.renderTiledFace and
// renderFluidBox's invertGasses; fluid_stream follows the placement of Create 6.0.10's
// FluidRenderer.renderFluidStream, drawn with that same tiling.
// Copyright (c) 2022 The Create Team
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use horizon_core::geometry::Direction;

/// Controls a still-fluid box. The tint is stack ARGB, independent of biome tint.
#[derive(Clone, Copy, Debug)]
pub struct NativeFluidRenderOptions {
    pub argb: Option<u32>,
    pub render_bottom: bool,
    /// TextureAtlasSprite.uvShrinkRatio: 4 / the largest stitched atlas dimension.
    /// Explicit because Horizon's block sprites occupy arrays rather than a stitched atlas.
    pub uv_shrink_ratio: f32,
    /// Turn the box half round its x axis through its centre: a gas drawn by a caller that
    /// asked to invert gases (catnip's `invertGasses` and `isLighterThanAir`).
    pub invert: bool,
}

/// A corner before the caller's pose, with UVs within the still sprite, and its outward normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub normal: [i8; 3],
}

/// Preserve sky light and increase block light to the fluid's luminosity (native light packing).
pub fn fluid_light(light: u32, luminosity: u8) -> u32 {
    (light & 0xf0) | (((light >> 8 & 0xf0).max(u32::from(luminosity.min(15)) * 16)) << 8)
}

/// Emit whole tiled quads, or refuse invalid/unreasonably large geometry before emitting anything.
/// Negative coordinates and exact integer boundaries retain the source's local UV fractions.
pub fn fluid_box(
    bounds: [f32; 6],
    options: NativeFluidRenderOptions,
    mut emit: impl FnMut(FluidVertex),
) -> bool {
    if !options.uv_shrink_ratio.is_finite()
        || !(0.0..=1.0).contains(&options.uv_shrink_ratio)
        || bounds
            .iter()
            .any(|x| !x.is_finite() || x.abs() > 1_048_576.0)
        || (0..3).any(|i| bounds[i] > bounds[i + 3])
    {
        return false;
    }
    let tiles =
        std::array::from_fn::<_, 3, _>(|i| (bounds[i + 3].ceil() - bounds[i].floor()) as u64);
    let quads = tiles[0] * tiles[1] * 2
        + tiles[1] * tiles[2] * 2
        + tiles[0] * tiles[2] * if options.render_bottom { 2 } else { 1 };
    if quads > 65_536 {
        return false;
    }
    let [x0, y0, z0, x1, y1, z1] = bounds;
    let (cy, cz) = (y0 + (y1 - y0) / 2.0, z0 + (z1 - z0) / 2.0);
    let mut emit = |v: FluidVertex| {
        if options.invert {
            emit(FluidVertex {
                pos: [v.pos[0], 2.0 * cy - v.pos[1], 2.0 * cz - v.pos[2]],
                normal: [v.normal[0], -v.normal[1], -v.normal[2]],
                ..v
            });
        } else {
            emit(v);
        }
    };
    for (side, face) in [
        (Direction::Down, [x0, z0, x1, z1, y0]),
        (Direction::Up, [x0, z0, x1, z1, y1]),
        (Direction::North, [x0, y0, x1, y1, z0]),
        (Direction::South, [x0, y0, x1, y1, z1]),
        (Direction::West, [z0, y0, z1, y1, x0]),
        (Direction::East, [z0, y0, z1, y1, x1]),
    ] {
        if side != Direction::Down || options.render_bottom {
            tiled_face(&mut emit, side, face, options.uv_shrink_ratio, 1.0);
        }
    }
    true
}

/// Which sprite a stream's quad samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamSprite {
    /// The four sides: the flowing sprite at half scale.
    Flowing,
    /// The cap on the leading end while the stream is filling: the still sprite.
    Still,
}

/// A stream of fluid through a block's `direction` face (`FluidRenderer.renderFluidStream`),
/// with the block at `0..1`: a square column `radius` blocks each way of the face's centre
/// line, from the block's centre (outbound) or the face (`inbound`, which turns it round) for
/// `progress` of half a block. Its four sides tile the flowing sprite at half scale; while
/// `progress` is not 1 a still cap closes its far end. Emits whole quads, or refuses a radius
/// outside `0.0..=1.0`, a progress or shrink ratio that isn't finite or a ratio outside
/// `0.0..=1.0` before emitting anything.
pub fn fluid_stream(
    direction: Direction,
    radius: f32,
    progress: f32,
    inbound: bool,
    uv_shrink_ratio: f32,
    mut emit: impl FnMut(FluidVertex, StreamSprite),
) -> bool {
    if !radius.is_finite()
        || !(0.0..=1.0).contains(&radius)
        || !progress.is_finite()
        || !uv_shrink_ratio.is_finite()
        || !(0.0..=1.0).contains(&uv_shrink_ratio)
    {
        return false;
    }
    let direction = if inbound {
        direction.opposite()
    } else {
        direction
    };
    // `AngleHelper.horizontalAngle`: `toYRot`, negated on the x axis; then up or down is
    // turned onto the column, which runs down the y axis.
    let yaw = match direction {
        Direction::South | Direction::Up | Direction::Down => 0,
        // West's -90 degrees, east's -270.
        Direction::West => 3,
        Direction::East => 1,
        Direction::North => 2,
    };
    let pitch = match direction {
        Direction::Up => 2,
        Direction::Down => 0,
        _ => 3,
    };
    let centre = glam::Mat4::from_translation(glam::Vec3::splat(0.5));
    let pose = centre
        * quarter_turns(glam::Vec3::Y, yaw)
        * quarter_turns(glam::Vec3::X, pitch)
        * centre.inverse()
        * glam::Mat4::from_translation(glam::vec3(0.5, 0.0, 0.5));
    let y = if inbound { 1.0 } else { 0.5 };
    let (low, high) = (y - (progress * 0.5).clamp(0.0, 1.0), y);
    for side in 0..4 {
        let pose = pose * quarter_turns(glam::Vec3::Y, side);
        tiled_face(
            &mut |v| emit(posed(pose, v), StreamSprite::Flowing),
            Direction::South,
            [-radius, low, radius, high, radius],
            uv_shrink_ratio,
            0.5,
        );
    }
    if progress != 1.0 {
        tiled_face(
            &mut |v| emit(posed(pose, v), StreamSprite::Still),
            Direction::Down,
            [-radius, -radius, radius, radius, low],
            uv_shrink_ratio,
            1.0,
        );
    }
    true
}

/// `v` moved by `pose`, a placement made of quarter turns and moves, its normal turned with it.
fn posed(pose: glam::Mat4, v: FluidVertex) -> FluidVertex {
    let normal = pose.transform_vector3(glam::Vec3::from_array(v.normal.map(f32::from)));
    FluidVertex {
        pos: pose
            .transform_point3(glam::Vec3::from_array(v.pos))
            .to_array(),
        normal: normal.round().to_array().map(|n| n as i8),
        ..v
    }
}

/// A turn of `quarters` right angles about `axis` (`Axis.rotationDegrees`, right-handed), exact.
fn quarter_turns(axis: glam::Vec3, quarters: i32) -> glam::Mat4 {
    let (sin, cos) = match quarters.rem_euclid(4) {
        0 => (0.0, 1.0),
        1 => (1.0, 0.0),
        2 => (0.0, -1.0),
        _ => (-1.0, 0.0),
    };
    let [x, y, z] = axis.to_array();
    // Rodrigues' formula for a unit axis, with sin and cos exact.
    let k = glam::Mat3::from_cols(
        glam::vec3(0.0, z, -y),
        glam::vec3(-z, 0.0, x),
        glam::vec3(y, -x, 0.0),
    );
    glam::Mat4::from_mat3(glam::Mat3::IDENTITY + k * sin + k * k * (1.0 - cos))
}

/// One face tiled one tile per block, its UVs `scale` of the sprite per block (catnip's
/// `textureScale`: 1 for a still face, 0.5 for a flowing one) and pulled in towards the
/// scaled sprite's centre by the shrink ratio.
fn tiled_face(
    out: &mut impl FnMut(FluidVertex),
    side: Direction,
    face: [f32; 5],
    ratio: f32,
    scale: f32,
) {
    let [a0, b0, a1, b1, plane] = face;
    let positive = matches!(side, Direction::Up | Direction::South | Direction::East);
    let shrink = ratio * 0.25 * scale;
    let lerp = |v: f32| {
        let v = v * scale;
        v + shrink * (0.5 * scale - v)
    };
    let mut a = a0;
    while a < a1 {
        let end_a = (a.floor() + 1.0).min(a1);
        let (u0, u1) = if matches!(side, Direction::North | Direction::East) {
            (end_a.ceil() - end_a, end_a.ceil() - a)
        } else {
            (a - a.floor(), end_a - a.floor())
        };
        let (u0, u1) = (lerp(u0), lerp(u1));
        let mut b = b0;
        while b < b1 {
            let end_b = (b.floor() + 1.0).min(b1);
            let (v0, v1) = if side == Direction::Up {
                (b - b.floor(), end_b - b.floor())
            } else {
                (end_b.ceil() - end_b, end_b.ceil() - b)
            };
            let (v0, v1) = (lerp(v0), lerp(v1));
            let positions = match side {
                Direction::West | Direction::East => {
                    let (near, far) = if positive { (end_a, a) } else { (a, end_a) };
                    [
                        [plane, end_b, near],
                        [plane, b, near],
                        [plane, b, far],
                        [plane, end_b, far],
                    ]
                }
                Direction::North | Direction::South => {
                    let (near, far) = if positive { (a, end_a) } else { (end_a, a) };
                    [
                        [near, end_b, plane],
                        [near, b, plane],
                        [far, b, plane],
                        [far, end_b, plane],
                    ]
                }
                Direction::Down | Direction::Up => {
                    let (near, far) = if positive { (b, end_b) } else { (end_b, b) };
                    [
                        [a, plane, near],
                        [a, plane, far],
                        [end_a, plane, far],
                        [end_a, plane, near],
                    ]
                }
            };
            let normal = match side {
                Direction::Down => [0, -1, 0],
                Direction::Up => [0, 1, 0],
                Direction::North => [0, 0, -1],
                Direction::South => [0, 0, 1],
                Direction::West => [-1, 0, 0],
                Direction::East => [1, 0, 0],
            };
            for (pos, uv) in positions
                .into_iter()
                .zip([[u0, v0], [u0, v1], [u1, v1], [u1, v0]])
            {
                out(FluidVertex { pos, uv, normal });
            }
            b = end_b;
        }
        a = end_a;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_tiling_is_outward_and_does_not_inset_geometry() {
        let bounds = [-1.25, 0.125, -0.25, 1.5, 1.125, 0.75];
        let mut result = Vec::new();
        assert!(fluid_box(
            bounds,
            NativeFluidRenderOptions {
                argb: None,
                render_bottom: false,
                uv_shrink_ratio: 4.0 / 2048.0,
                invert: false,
            },
            |v| result.push(v),
        ));
        assert_eq!(result.len(), (4 * 2 * 2 + 2 * 2 * 2 + 4 * 2) * 4);
        for quad in result.as_chunks::<4>().0 {
            let [a, b, c, _] = quad.map(|v| glam::Vec3::from_array(v.pos));
            assert!(
                (b - a)
                    .cross(c - a)
                    .dot(glam::Vec3::from_array(quad[0].normal.map(f32::from)))
                    > 0.0
            );
            assert!(
                quad.iter()
                    .all(|v| (0..3).all(|i| v.pos[i] >= bounds[i] && v.pos[i] <= bounds[i + 3]))
            );
        }
        assert!(
            result
                .iter()
                .any(|v| v.pos == [bounds[0], bounds[4], bounds[2]])
        );
        assert!(!fluid_box(
            [0.0, 0.0, 0.0, 1_000.0, 1_000.0, 1_000.0],
            NativeFluidRenderOptions {
                argb: None,
                render_bottom: true,
                uv_shrink_ratio: 0.0,
                invert: false,
            },
            |_| panic!("invalid geometry must fail before emitting"),
        ));
    }
    #[test]
    fn luminosity_changes_only_block_light() {
        assert_eq!(fluid_light(0x20a0, 15), 0xf0a0);
        assert_eq!(fluid_light(0x25a5, 15), 0xf0a0);
        assert_eq!(fluid_light(0xe030, 3), 0xe030);
    }

    fn options(render_bottom: bool, invert: bool) -> NativeFluidRenderOptions {
        NativeFluidRenderOptions {
            argb: None,
            render_bottom,
            uv_shrink_ratio: 0.0,
            invert,
        }
    }

    /// catnip's `invertGasses`: a gas is turned half round x through the box's centre, so its
    /// still top sits at the box's floor facing down, and a box drawn without its bottom face
    /// leaves out its top instead.
    #[test]
    fn a_gas_box_is_drawn_upside_down() {
        let bounds = [0.0, 0.25, 0.0, 1.0, 0.75, 1.0];
        let mut drawn = Vec::new();
        assert!(fluid_box(bounds, options(false, true), |v| drawn.push(v)));
        assert_eq!(drawn.len(), 5 * 4);
        let horizontal: Vec<_> = drawn.iter().filter(|v| v.normal[1] != 0).collect();
        assert_eq!(horizontal.len(), 4, "one horizontal face");
        assert!(
            horizontal
                .iter()
                .all(|v| v.normal == [0, -1, 0] && v.pos[1] == 0.25)
        );
        for quad in drawn.as_chunks::<4>().0 {
            let [a, b, c, _] = quad.map(|v| glam::Vec3::from_array(v.pos));
            let normal = glam::Vec3::from_array(quad[0].normal.map(f32::from));
            assert!(
                (b - a).cross(c - a).dot(normal) > 0.0,
                "still faces outwards"
            );
            assert!(
                quad.iter()
                    .all(|v| (0..3).all(|i| v.pos[i] >= bounds[i] && v.pos[i] <= bounds[i + 3]))
            );
        }
        let mut upright = Vec::new();
        fluid_box(bounds, options(false, false), |v| upright.push(v));
        assert!(
            upright
                .iter()
                .any(|v| v.normal == [0, 1, 0] && v.pos[1] == 0.75)
        );
    }

    fn unit(direction: Direction) -> glam::Vec3 {
        let [x, y, z] = direction.normal();
        glam::vec3(x as f32, y as f32, z as f32)
    }

    fn stream(
        direction: Direction,
        progress: f32,
        inbound: bool,
    ) -> Vec<(FluidVertex, StreamSprite)> {
        let mut drawn = Vec::new();
        assert!(fluid_stream(
            direction,
            0.25,
            progress,
            inbound,
            0.0,
            |v, s| { drawn.push((v, s)) }
        ));
        drawn
    }

    /// `renderFluidStream` through each face: a column a quarter block each way of the face's
    /// centre line, between the block's centre and its face. Outbound it fills from the centre,
    /// inbound from the face, and while filling a still cap closes the leading end, facing the
    /// way the fluid flows. Its sides face out of the column, in the flowing sprite at half scale.
    #[test]
    fn a_stream_runs_between_the_centre_and_its_face() {
        let centre = glam::Vec3::splat(0.5);
        for direction in Direction::ALL {
            let n = unit(direction);
            for inbound in [false, true] {
                // How far along the face's normal the stream's ends are.
                let (from, lead) = if inbound { (0.5, 0.25) } else { (0.0, 0.25) };
                let drawn = stream(direction, 0.5, inbound);
                let (sides, cap): (Vec<_>, Vec<_>) = drawn
                    .iter()
                    .partition(|(_, sprite)| *sprite == StreamSprite::Flowing);
                // Tiles are cut at whole-block lines of the column's own frame, centred on its axis:
                // two per side, four on the cap.
                assert_eq!(
                    (sides.len(), cap.len()),
                    (4 * 2 * 4, 4 * 4),
                    "{direction:?} {inbound}"
                );
                for (v, _) in &drawn {
                    let p = glam::Vec3::from_array(v.pos) - centre;
                    let along = p.dot(n);
                    let across = p - n * along;
                    assert!(
                        across.abs().max_element() <= 0.25 + 1e-6,
                        "{direction:?} {inbound} {p}"
                    );
                    let (low, high) = (f32::min(from, lead), f32::max(from, lead));
                    assert!(
                        along >= low - 1e-6 && along <= high + 1e-6,
                        "{direction:?} {inbound} {along}"
                    );
                }
                for (v, _) in &sides {
                    let p = glam::Vec3::from_array(v.pos) - centre;
                    let out = p - n * p.dot(n);
                    let normal = glam::Vec3::from_array(v.normal.map(f32::from));
                    assert!(
                        normal.dot(n) == 0.0 && normal.dot(out) > 0.0,
                        "{direction:?}"
                    );
                    assert!(
                        v.uv.iter().all(|&uv| (0.0..=0.5).contains(&uv)),
                        "half scale"
                    );
                }
                let flow = if inbound { -n } else { n };
                for (v, _) in &cap {
                    let along = (glam::Vec3::from_array(v.pos) - centre).dot(n);
                    assert!((along - lead).abs() < 1e-6, "{direction:?} {inbound}");
                    assert_eq!(glam::Vec3::from_array(v.normal.map(f32::from)), flow);
                }
                for quad in drawn.as_chunks::<4>().0 {
                    let [a, b, c, _] = quad.map(|(v, _)| glam::Vec3::from_array(v.pos));
                    let normal = glam::Vec3::from_array(quad[0].0.normal.map(f32::from));
                    assert!(
                        (b - a).cross(c - a).dot(normal) > 0.0,
                        "{direction:?} {inbound}"
                    );
                }
            }
        }
    }

    /// A full stream has no cap and reaches the face; one beyond the block's half is held to
    /// it (`Mth.clamp(progress * .5f, 0, 1)` only clamps past a whole block). A radius outside
    /// a block or a progress that isn't a number is refused before anything is emitted.
    #[test]
    fn a_full_stream_has_no_cap_and_bad_inputs_are_refused() {
        let full = stream(Direction::Down, 1.0, false);
        assert_eq!(full.len(), 4 * 2 * 4);
        assert!(full.iter().all(|(_, s)| *s == StreamSprite::Flowing));
        assert_eq!(
            full.iter().map(|(v, _)| v.pos[1]).fold(f32::MAX, f32::min),
            0.0
        );
        for (radius, progress) in [(1.5, 1.0), (-0.1, 1.0), (f32::NAN, 1.0), (0.25, f32::NAN)] {
            assert!(!fluid_stream(
                Direction::Up,
                radius,
                progress,
                false,
                0.0,
                |_, _| { panic!("refused before emitting") }
            ));
        }
    }
}
