//! [`MapData`] in, geometry out. Pure: no `World`, no `Commands`, no assets.
//!
//! # The rule this file lives by
//!
//! Wall colliders are not networked entities -- every peer builds its own from
//! the shared [`MapData`] -- so this function must return the same bits on
//! x86, ARM and wasm. A wall a single ulp out of place on one peer is not a
//! wobble: it is a disagreement rollback corrects and then re-creates every
//! tick, forever.
//!
//! The simulation as a whole is *already* not bit-deterministic across
//! platforms (the car controller uses trig every tick and the rollback layer
//! papers over the drift with host-authoritative snapshots), so the requirement
//! here is narrower than it first looks -- but for the static geometry it is
//! absolute. Concretely, inside this module:
//!
//! - **No transcendentals.** wasm links Rust's `libm`, native links the
//!   platform's, and `sin`/`cos`/`atan2` differ in the last ulp between glibc
//!   versions, between glibc and musl, and between either and wasm. `sqrt` is
//!   exactly specified by IEEE-754 *and* by the wasm spec, so `length` and
//!   `normalize` are fine; angles are not. [`Pose`] carries `sin`/`cos` rather
//!   than an angle precisely so nothing downstream needs `atan2` either.
//! - **No `mul_add`.** It lowers to a hardware FMA where one exists and a soft
//!   `fmaf` where one does not, which are different answers. Plain `a * b + c`
//!   is identical everywhere and at every `opt-level`.
//! - **`Vec2` only**, never `Vec3A`/`Vec4`: glam picks SIMD or scalar
//!   implementations by target feature and the two do not always round the same
//!   way for horizontal operations.
//! - **The bezier evaluation is written out here** rather than taken from
//!   `bevy_math::CubicCurve`. Not because bevy's is wrong, but because it is
//!   upstream: a refactor that reassociates `a*t + b*(1-t)` changes the last bit
//!   and moves every wall in every map, in a point release, with nothing failing
//!   to compile.
//!
//! Only the walls actually carry this requirement. The starting grid is spawned
//! host-only and replicated, item spawner positions only matter on the host, and
//! decor never touches physics at all -- but they all come out of the same pass,
//! so they get it for free.

use bevy::prelude::*;

use super::data::{MIN_HALF_WIDTH, MapData, TrackAnchor, scalar_to_world};

/// Samples taken per segment when flattening the curve into an arc-length table.
///
/// A *fixed* count rather than one chosen from the curve's shape, so the table
/// is a deterministic function of the node data and nothing else.
const FLATTEN_PER_SEGMENT: usize = 64;

/// Target spacing, in world units, of the resampled centreline.
const MESH_STEP: f32 = 2.0;

/// The longest a barrier may be, in centreline samples.
///
/// Walls reuse the centreline rather than resampling, so every wall vertex lies
/// exactly on a road-mesh edge vertex and a barrier can never drift off the
/// tarmac. On a straight nothing else has an opinion, so this is what a straight
/// costs in static bodies.
const WALL_EVERY: usize = 6;

/// How far a barrier is allowed to sit from the road edge it is built from.
///
/// A barrier is a straight chord of a curved edge, so on a corner it cuts inside
/// the arc and leaves a sliver of road showing past it. A fixed stride makes
/// that sliver as wide as the corner is tight: six samples is twelve units,
/// which on a hairpin of radius fourteen is a fifty-degree step, and the chord
/// across it misses the edge by a fifth of the road's width.
///
/// The wall takes another vertex before the error gets that far instead. Half a
/// unit, against a barrier two units thick, means the tarmac is covered
/// everywhere -- and it buys corners their vertices without paying for them on
/// the straights, where a chord of the full stride is exact.
///
/// This replaces pushing each vertex out by the sagitta, which is what used to
/// happen here. That trades a chord cutting into the road for one standing off
/// it at both ends, needs the curvature *at* a vertex to stand for the whole
/// span either side of it, and rests on a small-angle approximation exactly
/// where the angle stops being small. Measuring the miss is both simpler and
/// what the requirement actually says.
const WALL_TOLERANCE: f32 = 0.5;

/// How much of the way to the fold the inner offset is allowed to go.
///
/// The offset at half-width `h` on the inside of a corner of curvature `k`
/// passes through itself when `h * |k| >= 1`. Stopping slightly short keeps the
/// geometry valid rather than merely non-inverted.
const FOLD_LIMIT: f32 = 0.9;

/// Grid slots built regardless of how many players there are, so the editor can
/// draw the whole grid and the host can take a prefix of it.
pub const MAX_GRID: usize = 12;

/// How much of the track is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildLevel {
    /// Centreline, offsets and bounds: what a live editor drag needs, and all
    /// the road mesh is drawn from.
    Preview,
    /// Everything, including the grid, item boxes and the shape checks.
    Full,
}

/// A position and a facing, with the facing kept as the sine and cosine it will
/// be used as. avian's `Rotation` is a unit complex number with public `sin` and
/// `cos`, so a pose goes straight into `Rotation::from_sin_cos` and no angle is
/// ever formed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec2,
    pub sin: f32,
    pub cos: f32,
}

impl Pose {
    /// `direction` must already be normalised.
    #[inline]
    pub fn facing(position: Vec2, direction: Vec2) -> Self {
        Self {
            position,
            sin: direction.y,
            cos: direction.x,
        }
    }
}

/// One resampled point on the centreline, with everything derived at it.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub position: Vec2,
    /// Unit, in the direction of travel.
    pub tangent: Vec2,
    /// Unit, ninety degrees left of `tangent`.
    pub normal: Vec2,
    /// The interpolated road half-width here.
    pub half_width: f32,
    /// Arc length from the start line.
    pub s: f32,
    /// Signed; positive turns left.
    pub curvature: f32,
}

/// Something wrong with the *shape* of a map that still builds and still plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackWarning {
    /// The road is wider than the corner it goes round, so its inner edge folds
    /// through itself. The wall is cut and the road pinches, so it plays -- but
    /// it is not what was drawn. The fix is to narrow the road here.
    CornerTooTight {
        at_s: f32,
        half_width: f32,
        radius: f32,
    },
    /// Two parts of the lap pass close enough that a kart between them can be
    /// measured against the wrong one. Progress is nearest-segment and there are
    /// no checkpoints, so that reads as a jump in race position, or a lap
    /// gained or lost.
    LapPassesItself { a_s: f32, b_s: f32, gap: f32 },
    /// An item box or the start line sits off the road, which happens when a
    /// node is *narrowed* under it.
    AnchorOffRoad { lateral: f32, half_width: f32 },
    /// A node was dragged narrower than the builder will make a road.
    WidthClamped { at_s: f32 },
}

/// Everything the game needs to put a track on the screen.
#[derive(Resource, Clone, Debug)]
pub struct BuiltTrack {
    pub map: MapData,
    /// Equal-arc-length samples. `centre[0]` is the start line, so this is also
    /// the parameterisation the lap counter's wrap is measured in.
    pub centre: Vec<Sample>,
    pub length: f32,
    /// Closed loops, in the convention `spawn_barriers` expects: consecutive
    /// pairs, wrapping.
    ///
    /// Left and right of the *direction of travel*, not inside and outside: on a
    /// left-hand corner the left wall is the inner one, and on a right-hand
    /// corner it is the outer. Which is why the fold test below asks about the
    /// sign of the curvature rather than about a side.
    pub left_wall: Vec<Vec2>,
    pub right_wall: Vec<Vec2>,
    /// Coarse centreline for `ProgressLine`. Deliberately not the full sample
    /// set: progress is a fraction, not a rendered thing, and this is projected
    /// against for every kart on every replayed tick.
    pub progress: Vec<Vec2>,
    pub start_pose: Pose,
    pub grid: Vec<Pose>,
    pub item_boxes: Vec<Vec2>,
    pub bounds: Rect,
    pub warnings: Vec<TrackWarning>,
}

/// A point on the flattened curve, before resampling.
#[derive(Clone, Copy)]
struct Flat {
    position: Vec2,
    half_width: f32,
    /// Cumulative chord length from the start of segment 0.
    s: f32,
}

/// Position on one cubic bezier segment, in Bernstein form.
#[inline]
fn bezier(p: [Vec2; 4], t: f32) -> Vec2 {
    let mt = 1.0 - t;
    let a = mt * mt * mt;
    let b = 3.0 * mt * mt * t;
    let c = 3.0 * mt * t * t;
    let d = t * t * t;
    p[0] * a + p[1] * b + p[2] * c + p[3] * d
}

/// Flatten every segment into one open polyline with a cumulative arc length.
///
/// The last entry closes the loop back onto the first node, so `s` of that entry
/// is the lap length.
fn flatten(map: &MapData) -> Vec<Flat> {
    let segments = map.segment_count();
    let mut out = Vec::with_capacity(segments * FLATTEN_PER_SEGMENT + 1);
    let mut s = 0.0;
    let mut previous: Option<Vec2> = None;
    for segment in 0..segments {
        let control = map.segment_control_points(segment);
        for step in 0..FLATTEN_PER_SEGMENT {
            let u = step as f32 / FLATTEN_PER_SEGMENT as f32;
            let position = bezier(control, u);
            if let Some(previous) = previous {
                s += position.distance(previous);
            }
            previous = Some(position);
            out.push(Flat {
                position,
                // Only knowable here, while the point still knows which segment
                // and how far along it is.
                half_width: map.half_width_at(segment, u),
                s,
            });
        }
    }
    // Close the loop: the first node again, at the full lap length.
    let first = out[0];
    if let Some(previous) = previous {
        s += first.position.distance(previous);
    }
    out.push(Flat { s, ..first });
    out
}

/// Where in the flattened table a given arc length falls, and how far between
/// the two entries either side of it.
fn lookup(flat: &[Flat], s: f32) -> (usize, f32) {
    // The caller walks forward monotonically, but a binary search costs nothing
    // and does not care about call order.
    let mut lo = 0usize;
    let mut hi = flat.len() - 1;
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if flat[mid].s <= s { lo = mid } else { hi = mid }
    }
    let span = flat[hi].s - flat[lo].s;
    let f = if span > f32::EPSILON {
        (s - flat[lo].s) / span
    } else {
        0.0
    };
    (lo, f)
}

fn sample_at(flat: &[Flat], s: f32) -> (Vec2, f32) {
    let (i, f) = lookup(flat, s);
    let a = flat[i];
    let b = flat[i + 1];
    (
        a.position + (b.position - a.position) * f,
        a.half_width + (b.half_width - a.half_width) * f,
    )
}

/// The arc length at which an anchor sits.
fn anchor_s(map: &MapData, flat: &[Flat], anchor: &TrackAnchor) -> f32 {
    let segments = map.segment_count();
    let segment = (anchor.segment as usize).min(segments - 1);
    let index = segment * FLATTEN_PER_SEGMENT;
    let within = anchor.t_fraction() * FLATTEN_PER_SEGMENT as f32;
    let step = within.floor() as usize;
    let f = within - step as f32;
    let a = flat[index + step];
    let b = flat[index + step + 1];
    a.s + (b.s - a.s) * f
}

/// Which centreline samples get a wall vertex.
///
/// Greedily the furthest one that still keeps both road edges within
/// [`WALL_TOLERANCE`] of the chord to it, and never more than [`WALL_EVERY`]
/// away. Both edges, from one shared list, because a vertex on only one side
/// would put a barrier end somewhere the road mesh has no vertex.
///
/// The miss is measured, not modelled: the perpendicular distance from the chord
/// to each edge point it skips over. That is a cross product and a square root,
/// so it is the same arithmetic everywhere -- and unlike a curvature-and-sagitta
/// estimate it does not care whether the corner is a constant-radius arc, which
/// on a spline it never is.
fn wall_posts(centre: &[Sample]) -> Vec<usize> {
    let count = centre.len();
    // `count` is sample 0 again -- the walk below asks for the closing chord by
    // that name, so the wrap belongs here rather than at every call.
    let edge = |i: usize, side: f32| {
        let sample = centre[i % count];
        sample.position + sample.normal * (sample.half_width * side)
    };
    // Does the chord from `anchor` to `i` leave either edge by too much?
    let strays = |anchor: usize, i: usize| {
        [1.0f32, -1.0].iter().any(|&side| {
            let from = edge(anchor, side);
            let along = edge(i, side) - from;
            let span = along.length();
            if span < f32::EPSILON {
                return true;
            }
            let unit = along / span;
            (anchor + 1..i).any(|j| {
                let offset = edge(j, side) - from;
                (offset.x * unit.y - offset.y * unit.x).abs() > WALL_TOLERANCE
            })
        })
    };

    let mut posts = vec![0usize];
    loop {
        let anchor = *posts.last().expect("seeded with the first sample");
        // A one-sample chord skips nothing, so it is always acceptable and the
        // walk always advances.
        let mut next = anchor + 1;
        for i in anchor + 2..=(anchor + WALL_EVERY).min(count) {
            if strays(anchor, i) {
                break;
            }
            next = i;
        }
        // `count` is sample 0 again: the loop has closed. The chord that closes
        // it is the one span nothing checked, which is why the start line is a
        // good place for a seam and the middle of a hairpin is not.
        if next >= count {
            return posts;
        }
        posts.push(next);
    }
}

pub fn build(map: &MapData, level: BuildLevel) -> BuiltTrack {
    let mut warnings = Vec::new();
    let flat = flatten(map);
    let length = flat.last().map(|f| f.s).unwrap_or(0.0);
    let start_s = anchor_s(map, &flat, &map.start.at);

    // Round the sample count to a whole number of wall segments, then divide the
    // *exact* length by it, so the loop closes with no leftover stub.
    let raw = (length / MESH_STEP).ceil().max(WALL_EVERY as f32) as usize;
    let count = raw.div_ceil(WALL_EVERY) * WALL_EVERY;
    let ds = length / count as f32;

    let wrap = |s: f32| {
        let mut s = s % length;
        if s < 0.0 {
            s += length;
        }
        s
    };

    // Positions first, so tangents can be central differences over the resampled
    // points rather than derivatives of the curve. Central differences are what
    // make the tangent agree with the polyline the walls are actually built
    // from, which is what matters for the offsets lining up.
    let mut positions = Vec::with_capacity(count);
    let mut half_widths = Vec::with_capacity(count);
    for i in 0..count {
        let (position, half_width) = sample_at(&flat, wrap(start_s + i as f32 * ds));
        positions.push(position);
        half_widths.push(half_width);
    }

    let min_half_width = scalar_to_world(MIN_HALF_WIDTH);
    let mut centre = Vec::with_capacity(count);
    let mut clamped_width_at = None;
    for i in 0..count {
        let previous = positions[(i + count - 1) % count];
        let next = positions[(i + 1) % count];
        let tangent = (next - previous).normalize_or_zero();
        let normal = Vec2::new(-tangent.y, tangent.x);
        // Discrete curvature from the circle through three consecutive samples:
        // |k| = 2 * area / (|a||b||c|), signed by which way the turn goes.
        let a = positions[i] - previous;
        let b = next - positions[i];
        let cross = a.x * b.y - a.y * b.x;
        let denominator = a.length() * b.length() * (next - previous).length();
        let curvature = if denominator > f32::EPSILON {
            2.0 * cross / denominator
        } else {
            0.0
        };
        let mut half_width = half_widths[i];
        if half_width < min_half_width {
            half_width = min_half_width;
            clamped_width_at.get_or_insert(i as f32 * ds);
        }
        centre.push(Sample {
            position: positions[i],
            tangent,
            normal,
            half_width,
            s: i as f32 * ds,
            curvature,
        });
    }
    if let Some(at_s) = clamped_width_at {
        warnings.push(TrackWarning::WidthClamped { at_s });
    }

    let mut reported_tight = false;
    for sample in &centre {
        if sample.half_width * sample.curvature.abs() >= FOLD_LIMIT && !reported_tight {
            reported_tight = true;
            warnings.push(TrackWarning::CornerTooTight {
                at_s: sample.s,
                half_width: sample.half_width,
                radius: 1.0 / sample.curvature.abs().max(f32::EPSILON),
            });
        }
    }

    // Offsets. The mesh clamps at a fold so every quad stays valid and the road
    // pinches; the walls *drop* the folded samples so the polyline stays simple
    // and a too-tight corner comes out cut rather than knotted. A cut corner is
    // playable; a knot is a hole a kart drives into and sticks in.
    let mut left_wall = Vec::new();
    let mut right_wall = Vec::new();
    for &i in &wall_posts(&centre) {
        let sample = centre[i];
        let folds = sample.half_width * sample.curvature.abs() >= FOLD_LIMIT;
        // Positive curvature turns left, so the left side is the inner one there.
        if !(folds && sample.curvature > 0.0) {
            left_wall.push(sample.position + sample.normal * sample.half_width);
        }
        if !(folds && sample.curvature < 0.0) {
            right_wall.push(sample.position - sample.normal * sample.half_width);
        }
    }

    let progress: Vec<Vec2> = centre
        .iter()
        .step_by(WALL_EVERY)
        .map(|s| s.position)
        .collect();

    let start_pose = Pose::facing(centre[0].position, centre[0].tangent);

    let mut bounds = Rect {
        min: Vec2::splat(f32::MAX),
        max: Vec2::splat(f32::MIN),
    };
    for sample in &centre {
        for side in [1.0f32, -1.0] {
            let edge = sample.position + sample.normal * sample.half_width * side;
            bounds.min = bounds.min.min(edge);
            bounds.max = bounds.max.max(edge);
        }
    }
    let padding = scalar_to_world(map.bounds_padding);
    bounds.min -= Vec2::splat(padding);
    bounds.max += Vec2::splat(padding);

    let mut grid = Vec::new();
    let mut item_boxes = Vec::new();
    if level == BuildLevel::Full {
        // Walk backwards from the line, so the grid follows the track's curve
        // instead of trailing off a straight line behind it.
        let columns = map.start.grid.columns.max(1) as usize;
        let first = scalar_to_world(map.start.grid.first_row_offset);
        let row = scalar_to_world(map.start.grid.row_spacing);
        let column = scalar_to_world(map.start.grid.column_spacing);
        for slot in 0..MAX_GRID {
            let back = first + (slot / columns) as f32 * row;
            let across = (slot % columns) as f32 - (columns as f32 - 1.0) / 2.0;
            let s = wrap(start_s - back);
            let (position, _) = sample_at(&flat, s);
            // Tangent from the resampled centreline rather than the flat table,
            // so a grid slot faces the same way the road does there.
            let index = ((back / ds).round() as usize) % count;
            let tangent = centre[(count - index) % count].tangent;
            let normal = Vec2::new(-tangent.y, tangent.x);
            grid.push(Pose::facing(
                position + normal * (across * column),
                tangent,
            ));
        }

        for anchor in &map.item_boxes {
            let s = wrap(anchor_s(map, &flat, anchor));
            let (position, half_width) = sample_at(&flat, s);
            let index = ((s / ds).round() as usize) % count;
            let lateral = scalar_to_world(anchor.lateral);
            if lateral.abs() > half_width {
                warnings.push(TrackWarning::AnchorOffRoad {
                    lateral,
                    half_width,
                });
            }
            item_boxes.push(position + centre[index].normal * lateral);
        }

        warnings.extend(self_proximity_warnings(&centre, length));
    }

    BuiltTrack {
        map: map.clone(),
        centre,
        length,
        left_wall,
        right_wall,
        progress,
        start_pose,
        grid,
        item_boxes,
        bounds,
        warnings,
    }
}

/// Find places where the lap runs close to itself.
///
/// Progress is nearest-segment, so where two distant parts of the track come
/// within about a road's width, a kart on one can be measured against the other.
/// Hand-authored tracks avoided this by luck; a spline the author is free to
/// drag will not.
fn self_proximity_warnings(centre: &[Sample], length: f32) -> Vec<TrackWarning> {
    let mut worst: Option<(f32, f32, f32)> = None;

    for (i, a) in centre.iter().enumerate() {
        for b in centre.iter().skip(i + 1) {
            let along = (b.s - a.s).abs();
            // Distance the short way round, so the closing seam is not a hit.
            let along = along.min(length - along);
            let reach = a.half_width.max(b.half_width);
            if along <= 4.0 * reach {
                continue;
            }
            let gap = a.position.distance(b.position);
            if gap < 2.5 * reach && worst.is_none_or(|(_, _, w)| gap < w) {
                worst = Some((a.s, b.s, gap));
            }
        }
    }
    worst
        .map(|(a_s, b_s, gap)| vec![TrackWarning::LapPassesItself { a_s, b_s, gap }])
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::track::map::data::*;

    /// The handle length that turns four cubic beziers into a circle, to within
    /// about a part in ten thousand.
    const KAPPA: f32 = 0.552_284_8;

    /// A circle of radius `r`, as four nodes with mirrored handles. Every
    /// property worth checking has a closed form on a circle, which is why the
    /// fixtures are built out of one.
    pub(crate) fn circle(radius: f32, half_width: f32) -> MapData {
        let r = radius;
        let k = radius * KAPPA;
        // Anticlockwise from (r, 0), so the direction of travel turns left and
        // the signed curvature is positive.
        let corners = [
            (Vec2::new(r, 0.0), Vec2::new(0.0, k)),
            (Vec2::new(0.0, r), Vec2::new(-k, 0.0)),
            (Vec2::new(-r, 0.0), Vec2::new(0.0, -k)),
            (Vec2::new(0.0, -r), Vec2::new(k, 0.0)),
        ];
        let nodes = corners
            .iter()
            .map(|(position, out)| TrackNode {
                position: to_map(*position),
                in_handle: to_map(-*out),
                out_handle: to_map(*out),
                half_width: None,
                mirrored: true,
            })
            .collect();
        MapData {
            version: MAP_FORMAT_VERSION,
            name: "circle".into(),
            nodes,
            road: RoadShape {
                half_width: scalar_to_map(half_width),
                kerb_width: scalar_to_map(1.5),
                kerb_stripe: scalar_to_map(6.0),
            },
            start: StartLine {
                at: TrackAnchor::new(0, 0.0, 0),
                depth: scalar_to_map(3.0),
                grid: GridLayout {
                    columns: 3,
                    row_spacing: scalar_to_map(10.0),
                    column_spacing: scalar_to_map(7.0),
                    first_row_offset: scalar_to_map(9.0),
                },
            },
            item_boxes: vec![],
            bounds_padding: scalar_to_map(24.0),
            decor: DecorSettings {
                seed: 1,
                density: 1.0,
                clearance: scalar_to_map(3.0),
            },
        }
    }

    #[test]
    fn a_circle_comes_out_the_size_a_circle_should_be() {
        let radius = 80.0;
        let half_width = 11.0;
        let built = build(&circle(radius, half_width), BuildLevel::Full);

        let expected = 2.0 * core::f32::consts::PI * radius;
        assert!(
            (built.length - expected).abs() / expected < 1e-3,
            "lap length {} should be about {expected}",
            built.length
        );

        // Every centreline sample is on the circle, and every wall vertex is on
        // the circle offset by the road's half-width.
        for sample in &built.centre {
            assert!((sample.position.length() - radius).abs() < 0.2);
            assert!((sample.half_width - half_width).abs() < 1e-3);
        }
        // The fixture runs anticlockwise, so "left of travel" is the inside.
        for point in &built.left_wall {
            assert!((point.length() - (radius - half_width)).abs() < 0.3);
        }
        for point in &built.right_wall {
            assert!((point.length() - (radius + half_width)).abs() < 0.3);
        }
        assert!(built.warnings.is_empty(), "{:?}", built.warnings);
    }

    /// Arc length has to increase evenly, because `measure_progress` reports a
    /// fraction of it and the lap counter compares that against 0.95 and 0.05.
    /// Bunched samples would make progress race and stall around the lap.
    #[test]
    fn arc_length_increases_evenly_and_the_loop_closes() {
        let built = build(&circle(80.0, 11.0), BuildLevel::Full);
        let step = built.length / built.centre.len() as f32;
        for pair in built.centre.windows(2) {
            assert!((pair[1].s - pair[0].s - step).abs() < 1e-3);
            // And the samples really are that far apart in space, not just in
            // the number written on them.
            let gap = pair[1].position.distance(pair[0].position);
            assert!((gap - step).abs() < step * 0.05, "gap {gap} vs step {step}");
        }
        assert_eq!(built.centre[0].s, 0.0, "sample 0 is the start line");
        let last = built.centre.last().unwrap();
        assert!((last.s + step - built.length).abs() < 1e-3, "the loop closes");
    }

    /// The start line is progress zero, wherever the author put it. Without
    /// this the lap counter's wrap fires at whichever node was written first.
    #[test]
    fn the_centreline_begins_at_the_start_line_not_at_node_zero() {
        let mut map = circle(80.0, 11.0);
        map.start.at = TrackAnchor::new(2, 0.5, 0);
        let built = build(&map, BuildLevel::Full);
        // Segment 2 starts at (-r, 0) and runs to (0, -r), so halfway is the
        // bottom-left of the circle.
        let expected = Vec2::new(-80.0, 0.0).lerp(Vec2::new(0.0, -80.0), 0.5).normalize() * 80.0;
        assert!(
            built.centre[0].position.distance(expected) < 3.0,
            "start at {:?}, expected near {expected:?}",
            built.centre[0].position
        );
        assert_eq!(built.start_pose.position, built.centre[0].position);
    }

    /// A road wider than the corner it goes round: the inner edge would fold
    /// through itself. It has to be reported, and the wall has to stay a simple
    /// polyline -- a knotted one is a hole a kart gets stuck in.
    #[test]
    fn a_corner_tighter_than_the_road_is_wide_is_reported_and_survives() {
        // Half-width 30 against radius 20: 30 * (1/20) = 1.5, well past folding.
        let built = build(&circle(20.0, 30.0), BuildLevel::Full);
        assert!(
            built
                .warnings
                .iter()
                .any(|w| matches!(w, TrackWarning::CornerTooTight { .. })),
            "expected a CornerTooTight, got {:?}",
            built.warnings
        );
        // Anticlockwise, so the left wall is the inner one: it is cut away
        // entirely rather than turned inside out, and the outer one is untouched.
        assert!(!built.right_wall.is_empty(), "the outer wall survives");
        assert!(
            built.left_wall.is_empty(),
            "the folded inner wall is dropped, not inverted"
        );
        for pair in built.right_wall.windows(2) {
            assert!(pair[0].distance(pair[1]) > 0.0, "no zero-length wall segment");
        }
    }

    /// Every point along every barrier is within [`WALL_TOLERANCE`] of the road
    /// edge it stands on -- not just its two ends.
    ///
    /// This is the thing a fixed stride got wrong. A barrier is straight and the
    /// edge is not, so a stride long enough to be economical on a straight cuts
    /// the corner off a hairpin: at half-width 12 on radius 14 the miss was over
    /// two units, a fifth of the road, and the two walls of a hairpin ended up
    /// looking like four.
    ///
    /// Measured against the offset of the *centreline samples*, which is what
    /// the road mesh draws, so this is literally "the barrier is where the road
    /// stops". Folded corners are excluded: there the wall is deliberately cut
    /// and the chord across the cut is not standing on anything.
    #[test]
    fn a_barrier_never_leaves_the_road_edge_it_stands_on() {
        for map in [circle(80.0, 11.0), circle(24.0, 10.0), circle(300.0, 6.0)] {
            let built = build(&map, BuildLevel::Full);
            for (wall, side) in [(&built.left_wall, 1.0f32), (&built.right_wall, -1.0)] {
                let edge: Vec<Vec2> = built
                    .centre
                    .iter()
                    .map(|s| s.position + s.normal * (s.half_width * side))
                    .collect();
                for i in 0..wall.len() {
                    let (a, b) = (wall[i], wall[(i + 1) % wall.len()]);
                    for step in 0..=8 {
                        let point = a.lerp(b, step as f32 / 8.0);
                        let miss = edge
                            .iter()
                            .enumerate()
                            .map(|(j, p)| distance_to_segment(point, *p, edge[(j + 1) % edge.len()]))
                            .fold(f32::MAX, f32::min);
                        assert!(
                            miss <= WALL_TOLERANCE + 1e-3,
                            "barrier {i} is {miss} from the road edge"
                        );
                    }
                }
            }
        }
    }

    fn distance_to_segment(point: Vec2, a: Vec2, b: Vec2) -> f32 {
        let along = b - a;
        let t = if along.length_squared() > 1e-9 {
            ((point - a).dot(along) / along.length_squared()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        point.distance(a + along * t)
    }

    /// A corner takes as many vertices as it needs and a straight takes as few
    /// as it can, which is the whole point of choosing them by the miss rather
    /// than by a stride.
    #[test]
    fn a_corner_buys_wall_vertices_and_a_straight_does_not() {
        let stride = |built: &BuiltTrack| built.length / built.right_wall.len() as f32;
        // Six samples of two units, the cap, because a chord that long across a
        // circle this size misses it by a hundredth of a unit.
        let gentle = stride(&build(&circle(300.0, 6.0), BuildLevel::Preview));
        assert!(gentle > 10.0, "a road this straight took a vertex every {gentle}");
        // And a corner a kart has to be pointed at takes them twice as often.
        let tight = stride(&build(&circle(24.0, 6.0), BuildLevel::Preview));
        assert!(tight < gentle, "the tight circle took a vertex every {tight}");
        let hairpin = stride(&build(&circle(12.0, 8.0), BuildLevel::Preview));
        assert!(
            hairpin < gentle * 0.5,
            "the hairpin took a vertex every {hairpin}, the straight every {gentle}"
        );
    }

    #[test]
    fn no_wall_segment_has_zero_length() {
        let built = build(&circle(80.0, 11.0), BuildLevel::Full);
        for wall in [&built.left_wall, &built.right_wall] {
            assert!(wall.len() > 3);
            for i in 0..wall.len() {
                let a = wall[i];
                let b = wall[(i + 1) % wall.len()];
                assert!(a.distance(b) > 1e-4, "degenerate wall segment at {i}");
            }
        }
    }

    /// The grid has to sit behind the line, on the road, facing the way the
    /// track goes -- on a curve, not just on a straight.
    #[test]
    fn the_starting_grid_sits_behind_the_line_and_faces_forward() {
        let map = circle(80.0, 11.0);
        let built = build(&map, BuildLevel::Full);
        assert_eq!(built.grid.len(), MAX_GRID);
        for (slot, pose) in built.grid.iter().enumerate() {
            // On the road: within half-width of the circle, allowing for the
            // lateral column offset.
            let from_centre = (pose.position.length() - 80.0).abs();
            assert!(from_centre < 11.0, "slot {slot} is off the road: {from_centre}");
            // Facing along the track: on an anticlockwise circle the heading is
            // perpendicular to the radius.
            let facing = Vec2::new(pose.cos, pose.sin);
            let radial = pose.position.normalize();
            assert!(
                facing.dot(radial).abs() < 0.2,
                "slot {slot} faces {facing:?} against radius {radial:?}"
            );
        }
        // Behind the line, not in front of it: the first slot is further round
        // the circle backwards than the last.
        let first = built.grid[0].position.distance(built.start_pose.position);
        let last = built.grid[MAX_GRID - 1].position.distance(built.start_pose.position);
        assert!(first < last, "the grid runs backwards from the line");
    }

    #[test]
    fn item_boxes_land_where_their_anchor_says() {
        let mut map = circle(80.0, 11.0);
        map.item_boxes = vec![
            TrackAnchor::new(0, 0.0, 0),
            TrackAnchor::new(0, 0.0, scalar_to_map(6.0)),
            TrackAnchor::new(2, 0.5, scalar_to_map(-6.0)),
        ];
        let built = build(&map, BuildLevel::Full);
        assert_eq!(built.item_boxes.len(), 3);
        // Centred at the start of segment 0, which is (r, 0).
        assert!(built.item_boxes[0].distance(Vec2::new(80.0, 0.0)) < 1.0);
        // Six units left of it: travel is anticlockwise there, so left is inward.
        assert!((built.item_boxes[1].length() - 74.0).abs() < 1.0);
        assert!(
            built
                .warnings
                .iter()
                .all(|w| !matches!(w, TrackWarning::AnchorOffRoad { .. })),
            "all three are on the road"
        );
    }

    /// Narrowing a node under an item box strands it. Widening cannot, because
    /// `lateral` is in world units -- which is exactly why it is.
    #[test]
    fn narrowing_the_road_under_an_item_box_is_reported() {
        let mut map = circle(80.0, 11.0);
        map.item_boxes = vec![TrackAnchor::new(0, 0.0, scalar_to_map(9.0))];
        assert!(
            build(&map, BuildLevel::Full)
                .warnings
                .iter()
                .all(|w| !matches!(w, TrackWarning::AnchorOffRoad { .. })),
            "9 units across a 11-unit half-width is on the road"
        );
        map.nodes[0].half_width = Some(scalar_to_map(4.0));
        assert!(
            build(&map, BuildLevel::Full)
                .warnings
                .iter()
                .any(|w| matches!(w, TrackWarning::AnchorOffRoad { .. })),
            "after narrowing to 4 it is not"
        );
    }

    // -- variable width ------------------------------------------------------

    /// Per-node width, and the smoothstep between. The C1 property is the whole
    /// reason it is a smoothstep and not a lerp, so it is asserted directly.
    #[test]
    fn width_interpolates_smoothly_between_nodes() {
        let mut map = circle(80.0, 10.0);
        map.nodes[0].half_width = Some(scalar_to_map(6.0));
        map.nodes[1].half_width = Some(scalar_to_map(14.0));

        // Exact at the nodes.
        assert!((map.half_width_at(0, 0.0) - 6.0).abs() < 1e-3);
        assert!((map.half_width_at(0, 1.0) - 14.0).abs() < 1e-3);
        // Inheriting nodes still get the map default.
        assert!((map.half_width_at(2, 0.0) - 10.0).abs() < 1e-3);

        // Matches the smoothstep analytically at the quarter points.
        for u in [0.25f32, 0.5, 0.75] {
            let e = u * u * (3.0 - 2.0 * u);
            let expected = 6.0 + (14.0 - 6.0) * e;
            assert!(
                (map.half_width_at(0, u) - expected).abs() < 1e-3,
                "u={u}: {} vs {expected}",
                map.half_width_at(0, u)
            );
        }

        // Zero slope at both ends -- this is what keeps the road edge from
        // creasing at a node where the width changes, and a lerp would fail it.
        let h = 1e-3;
        let slope_at_start = (map.half_width_at(0, h) - map.half_width_at(0, 0.0)) / h;
        let slope_at_end = (map.half_width_at(0, 1.0) - map.half_width_at(0, 1.0 - h)) / h;
        assert!(slope_at_start.abs() < 0.05, "slope {slope_at_start} at u=0");
        assert!(slope_at_end.abs() < 0.05, "slope {slope_at_end} at u=1");
    }

    /// The width the author asked for has to reach the geometry, not just the
    /// lookup function.
    #[test]
    fn a_varying_width_reaches_the_walls() {
        let mut map = circle(80.0, 10.0);
        map.nodes[0].half_width = Some(scalar_to_map(5.0));
        map.nodes[2].half_width = Some(scalar_to_map(16.0));
        let built = build(&map, BuildLevel::Full);
        let narrowest = built.centre.iter().map(|s| s.half_width).fold(f32::MAX, f32::min);
        let widest = built.centre.iter().map(|s| s.half_width).fold(f32::MIN, f32::max);
        assert!((narrowest - 5.0).abs() < 0.3, "narrowest {narrowest}");
        assert!((widest - 16.0).abs() < 0.3, "widest {widest}");
    }

    /// Pinching the road is how an author answers a `CornerTooTight`, so that
    /// has to actually work.
    #[test]
    fn pinching_a_tight_corner_clears_the_warning() {
        let tight = |half_width: f32| {
            build(&circle(20.0, half_width), BuildLevel::Full)
                .warnings
                .iter()
                .any(|w| matches!(w, TrackWarning::CornerTooTight { .. }))
        };
        assert!(tight(30.0), "a 30-unit half-width on a radius of 20 folds");
        assert!(!tight(8.0), "pinching to 8 clears it");
    }

    #[test]
    fn a_node_dragged_to_nothing_is_clamped_rather_than_inverted() {
        let mut map = circle(80.0, 10.0);
        map.nodes[0].half_width = Some(0);
        let built = build(&map, BuildLevel::Full);
        let floor = scalar_to_world(MIN_HALF_WIDTH);
        for sample in &built.centre {
            assert!(sample.half_width >= floor - 1e-4, "{}", sample.half_width);
        }
        assert!(
            built
                .warnings
                .iter()
                .any(|w| matches!(w, TrackWarning::WidthClamped { .. })),
            "and it says so"
        );
    }

    // -- determinism ---------------------------------------------------------

    /// Compare bit patterns, not values: `==` cannot tell `0.0` from `-0.0` and
    /// lies outright about NaN, and either would be a real divergence.
    fn fingerprint(built: &BuiltTrack) -> Vec<u32> {
        let mut out = Vec::new();
        let mut push = |v: f32| out.push(v.to_bits());
        for sample in &built.centre {
            push(sample.position.x);
            push(sample.position.y);
            push(sample.tangent.x);
            push(sample.tangent.y);
            push(sample.half_width);
            push(sample.s);
            push(sample.curvature);
        }
        for wall in [&built.left_wall, &built.right_wall] {
            for p in wall {
                push(p.x);
                push(p.y);
            }
        }
        for pose in &built.grid {
            push(pose.position.x);
            push(pose.position.y);
            push(pose.sin);
            push(pose.cos);
        }
        push(built.length);
        out
    }

    /// The builder is a function of its input, to the bit. This is the property
    /// the whole no-transcendentals rule exists to protect: every peer derives
    /// its own wall colliders, and a wall one ulp out of place is a divergence
    /// rollback re-creates every tick.
    #[test]
    fn the_builder_is_a_function_of_its_input() {
        let map = circle(80.0, 11.0);
        let a = build(&map, BuildLevel::Full);
        let b = build(&map.clone(), BuildLevel::Full);
        assert_eq!(fingerprint(&a), fingerprint(&b));
    }

    /// No NaN or infinity anywhere in the output. A single one reaches avian as
    /// a collider vertex and takes the whole simulation with it.
    #[test]
    fn the_geometry_is_finite() {
        for map in [circle(80.0, 11.0), circle(20.0, 30.0), circle(5.0, 4.0)] {
            let built = build(&map, BuildLevel::Full);
            for bits in fingerprint(&built) {
                let v = f32::from_bits(bits);
                assert!(v.is_finite(), "non-finite value in {}", built.map.name);
            }
        }
    }

    // -- degenerate input ----------------------------------------------------

    /// The editor will hand the builder garbage while the author is mid-drag,
    /// and a panic inside `OnEnter(Screen::Race)` takes the session down with
    /// it. Everything here has to produce *something*.
    #[test]
    fn degenerate_maps_build_without_panicking() {
        let mut coincident = circle(80.0, 11.0);
        coincident.nodes[1].position = coincident.nodes[0].position;
        coincident.nodes[1].in_handle = IVec2::ZERO;
        coincident.nodes[1].out_handle = IVec2::ZERO;

        let mut three = circle(80.0, 11.0);
        three.nodes.truncate(3);

        let mut figure_eight = circle(80.0, 11.0);
        figure_eight.nodes.swap(1, 3);

        let mut no_handles = circle(80.0, 11.0);
        for node in &mut no_handles.nodes {
            node.in_handle = IVec2::ZERO;
            node.out_handle = IVec2::ZERO;
        }

        for (name, map) in [
            ("two coincident nodes", coincident),
            ("the minimum three nodes", three),
            ("a self-intersecting loop", figure_eight),
            ("no handles at all", no_handles),
            ("a very small circle", circle(3.0, 2.0)),
        ] {
            let built = build(&map, BuildLevel::Full);
            assert!(!built.centre.is_empty(), "{name} produced no centreline");
            assert_eq!(built.grid.len(), MAX_GRID, "{name} produced no grid");
            for bits in fingerprint(&built) {
                assert!(f32::from_bits(bits).is_finite(), "{name} produced a NaN");
            }
        }
    }

    /// A track that doubles back close to itself is where nearest-segment
    /// progress goes wrong, so the author gets told.
    #[test]
    fn a_lap_that_passes_close_to_itself_is_reported() {
        let wide = circle(80.0, 11.0);
        assert!(
            !build(&wide, BuildLevel::Full)
                .warnings
                .iter()
                .any(|w| matches!(w, TrackWarning::LapPassesItself { .. })),
            "a plain circle never comes near itself"
        );

        // A flattened loop: the two long sides run within a few units of each
        // other while being half a lap apart.
        let mut squashed = circle(80.0, 6.0);
        for node in &mut squashed.nodes {
            node.position.y /= 40;
            node.in_handle.y /= 40;
            node.out_handle.y /= 40;
        }
        assert!(
            build(&squashed, BuildLevel::Full)
                .warnings
                .iter()
                .any(|w| matches!(w, TrackWarning::LapPassesItself { .. })),
            "a squashed loop does"
        );
    }

    // -- validation ----------------------------------------------------------

    #[test]
    fn validate_refuses_what_cannot_be_built() {
        let good = circle(80.0, 11.0);
        assert_eq!(good.validate(), Ok(()));

        let mut future = good.clone();
        future.version = MAP_FORMAT_VERSION + 1;
        assert_eq!(
            future.validate(),
            Err(MapError::UnknownVersion(MAP_FORMAT_VERSION + 1))
        );

        let mut two = good.clone();
        two.nodes.truncate(2);
        assert_eq!(two.validate(), Err(MapError::TooFewNodes(2)));

        let mut stray = good.clone();
        stray.item_boxes = vec![TrackAnchor::new(9, 0.0, 0)];
        assert_eq!(
            stray.validate(),
            Err(MapError::AnchorOutOfRange {
                segment: 9,
                segments: 4
            })
        );

        let mut nameless = good.clone();
        nameless.name = "  ".into();
        assert_eq!(nameless.validate(), Err(MapError::NoName));
    }

    /// Deleting a node must not teleport the item boxes past it.
    #[test]
    fn removing_a_node_keeps_the_anchors_where_they_were() {
        let mut map = circle(80.0, 11.0);
        map.item_boxes = vec![
            TrackAnchor::new(0, 0.5, 0),
            TrackAnchor::new(2, 0.5, 0),
            TrackAnchor::new(3, 0.5, 0),
        ];
        let before: Vec<Vec2> = build(&map, BuildLevel::Full).item_boxes;

        map.remap_anchors_after_node_removal(1);
        map.nodes.remove(1);
        assert_eq!(map.validate(), Ok(()));
        let after = build(&map, BuildLevel::Full).item_boxes;

        // The two boxes away from the removed node barely move; the one on a
        // merged segment stays on the road rather than jumping a quarter-lap.
        for (i, (a, b)) in before.iter().zip(after.iter()).enumerate() {
            assert!(
                a.distance(*b) < 40.0,
                "box {i} jumped from {a:?} to {b:?}"
            );
        }
    }
}
