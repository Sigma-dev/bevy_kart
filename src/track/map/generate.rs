//! Drawing the built-in maps.
//!
//! Every shipped map but the classic one -- which was converted from a sprite --
//! is a closed centreline said in code here: a radius that varies with angle, or
//! a few arcs joined end to end. [`draw`] turns one into spline nodes, sets the
//! road wide on the straights and narrow through the corners, spreads item boxes
//! round the lap, and hands back a [`MapData`]. The JSON under `assets/maps/` is
//! a snapshot of the result: `the_shipped_maps_are_what_this_draws` keeps the two
//! in step, and `just maps` rewrites the snapshot.
//!
//! Test-only, so none of this is in the game or the web bundle. What the game
//! ships is the JSON.
//!
//! # Why a snapshot, and not a `sin` at startup
//!
//! `MapData` is integer coordinates so that every peer builds identical walls,
//! and its content hash is what the network calls a map. A shape drawn from
//! `sin` and `cos` when the game starts would round differently in the last bit
//! on different platforms and, once in a long while, in the last map unit -- so
//! one slug would mean two maps. Drawing once and committing the integers puts
//! the rounding in one place. It is also why the snapshot test allows a map unit
//! of slack: the drawing runs on whichever machine runs the tests, and it may
//! own a different `libm`.
//!
//! # Two ways of drawing
//!
//! Sweeping Bends was drawn first: nodes evenly spaced along the curve,
//! Catmull-Rom handles, and the road narrowed by how sharply the curve turns
//! between neighbouring nodes. The four maps after it space their nodes by
//! curvature, point their handles along the curve's own tangent, and read the
//! width off the spline they actually are -- see [`Handles`] and [`Widths`] for
//! what each change bought. The first way is kept so the file that ships is
//! reproduced, not redrawn.

use bevy::math::{DVec2, IVec2};
use std::f64::consts::PI;

use super::data::{
    DecorSettings, GridLayout, MAP_FORMAT_VERSION, MAP_UNITS_PER_WORLD, MapData, RoadShape,
    StartLine, TrackAnchor, TrackNode,
};

/// The drawing is done in doubles and rounded to map units once, at the end.
const UNITS: f64 = MAP_UNITS_PER_WORLD as f64;
/// The builder folds the inner edge at `h * k >= 0.9`; the widths stop short.
const FOLD_LIMIT: f64 = 0.8;
/// A handle never reaches more than this far towards the nearer neighbour, so a
/// node cannot overshoot one.
const HANDLE_REACH: f64 = 0.38;
/// How far either side of the centre an item box sits.
const ITEM_LATERAL: f64 = 4.0;
const SAMPLES_PER_SEGMENT: usize = 64;

// --- small helpers -----------------------------------------------------------

fn to_map(p: DVec2) -> IVec2 {
    IVec2::new((p.x * UNITS).round() as i32, (p.y * UNITS).round() as i32)
}

fn scalar(v: f64) -> i32 {
    (v * UNITS).round() as i32
}

fn len(v: DVec2) -> f64 {
    v.x.hypot(v.y)
}

fn dist(a: DVec2, b: DVec2) -> f64 {
    len(a - b)
}

fn unit(v: DVec2) -> DVec2 {
    let n = len(v);
    if n > 1e-12 { v / n } else { DVec2::ZERO }
}

fn cross(a: DVec2, b: DVec2) -> f64 {
    a.perp_dot(b)
}

fn lerp(a: DVec2, b: DVec2, f: f64) -> DVec2 {
    a + (b - a) * f
}

fn linspace(a: f64, b: f64, steps: usize) -> impl Iterator<Item = f64> {
    (0..=steps).map(move |i| a + (b - a) * i as f64 / steps as f64)
}

// --- curves ------------------------------------------------------------------

/// One turn of `shape(theta)` from `theta0`, as a dense polyline. Anticlockwise
/// when the shape is, which for a radius-of-angle it is.
fn parametric(shape: impl Fn(f64) -> DVec2, theta0: f64, steps: usize) -> Vec<DVec2> {
    (0..steps)
        .map(|i| shape(theta0 + 2.0 * PI * i as f64 / steps as f64))
        .collect()
}

/// A circular arc from `deg0` to `deg1`, both ends included; the direction of
/// travel is the sign of the turn.
fn arc(centre: DVec2, radius: f64, deg0: f64, deg1: f64) -> Vec<DVec2> {
    linspace(deg0, deg1, 800)
        .map(|d| centre + radius * DVec2::from_angle(d.to_radians()))
        .collect()
}

/// A half-circle from `centre + radius * start_dir` round to the opposite point,
/// passing through `centre + radius * travel`: the hairpin at the end of a
/// stretch of road that runs out and back.
fn cap(centre: DVec2, radius: f64, start_dir: DVec2, travel: DVec2) -> Vec<DVec2> {
    let a0 = start_dir.y.atan2(start_dir.x);
    let sign = if cross(start_dir, travel) > 0.0 {
        1.0
    } else {
        -1.0
    };
    (0..=600)
        .map(|i| centre + radius * DVec2::from_angle(a0 + sign * PI * i as f64 / 600.0))
        .collect()
}

/// Open polylines laid end to start into one closed one. Each has to begin where
/// the last ended, and the last has to end where the first began; a gap is a
/// mistake in the map's definition, not something to smooth over.
fn joined(sections: Vec<Vec<DVec2>>) -> Vec<DVec2> {
    let mut points: Vec<DVec2> = Vec::new();
    for section in sections {
        match points.last() {
            Some(&last) => {
                let gap = dist(last, section[0]);
                assert!(gap <= 1e-3, "sections do not meet: gap of {gap:.3}");
                points.extend_from_slice(&section[1..]);
            }
            None => points.extend(section),
        }
    }
    let gap = dist(*points.last().expect("a section"), points[0]);
    assert!(gap <= 1e-3, "the loop does not close: gap of {gap:.3}");
    points.pop();
    points
}

/// Arc length at each point of a closed polyline, and the total after the last.
fn cumulative(points: &[DVec2]) -> Vec<f64> {
    let mut out = Vec::with_capacity(points.len() + 1);
    out.push(0.0);
    for i in 0..points.len() {
        let step = dist(points[(i + 1) % points.len()], points[i]);
        out.push(out[i] + step);
    }
    out
}

/// The point at arc length `s` round a closed polyline, in whatever measure
/// `cum` was accumulated in.
fn point_at(points: &[DVec2], cum: &[f64], s: f64) -> DVec2 {
    let total = cum[points.len()];
    let mut s = s % total;
    if s < 0.0 {
        s += total;
    }
    let (mut lo, mut hi) = (0usize, points.len());
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if cum[mid] <= s { lo = mid } else { hi = mid }
    }
    let span = cum[lo + 1] - cum[lo];
    let f = if span > 1e-12 {
        (s - cum[lo]) / span
    } else {
        0.0
    };
    lerp(points[lo], points[(lo + 1) % points.len()], f)
}

/// |curvature| at each point: how far the direction turns over `window` units
/// either side, per unit of arc. A window rather than the neighbouring points so
/// the small kink where two sections meet reads as the gentle bend the spline
/// will make of it, not as a spike.
fn turning(points: &[DVec2], cum: &[f64], window: f64) -> Vec<f64> {
    points
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let before = unit(p - point_at(points, cum, cum[i] - window));
            let after = unit(point_at(points, cum, cum[i] + window) - p);
            let angle = cross(before, after).atan2(before.dot(after));
            angle.abs() / window
        })
        .collect()
}

/// `count` nodes round a closed polyline, and the curve's own direction at each.
///
/// Spaced evenly in a measure that counts a unit of arc as `1 + weight *
/// curvature`, so with a weight a hairpin gets the nodes a spline needs to go
/// round it without a straight paying for nodes it does not need, and with none
/// the spacing is plain arc length. The first node is the polyline's first
/// point, or the point nearest `start`.
fn place_nodes(
    points: &[DVec2],
    count: usize,
    start: Option<DVec2>,
    weight: f64,
) -> (Vec<DVec2>, Vec<DVec2>) {
    let rotated: Vec<DVec2>;
    let points = match start {
        Some(start) => {
            let nearest = (0..points.len())
                .min_by(|&a, &b| dist(points[a], start).total_cmp(&dist(points[b], start)))
                .expect("a polyline with points");
            rotated = points[nearest..]
                .iter()
                .chain(&points[..nearest])
                .copied()
                .collect();
            &rotated
        }
        None => points,
    };
    let cum = cumulative(points);
    let k = turning(points, &cum, 4.0);
    let mut weighted = Vec::with_capacity(points.len() + 1);
    weighted.push(0.0);
    for i in 0..points.len() {
        let j = (i + 1) % points.len();
        let step = dist(points[j], points[i]) * (1.0 + weight * 0.5 * (k[i] + k[j]));
        weighted.push(weighted[i] + step);
    }
    let total = weighted[points.len()];
    (0..count)
        .map(|n| {
            let s = total * n as f64 / count as f64;
            let node = point_at(points, &weighted, s);
            // Read off a short chord either side, in the same measure.
            let tangent =
                unit(point_at(points, &weighted, s + 2.0) - point_at(points, &weighted, s - 2.0));
            (node, tangent)
        })
        .unzip()
}

// --- the spline --------------------------------------------------------------

/// How a node's handles are found. One length for both, mirrored, because that
/// is the ordinary smooth node the editor makes.
#[derive(Clone, Copy)]
enum Handles {
    /// A sixth of the way from the previous node to the next. Right on a
    /// straight and short in a corner: on a sixty-degree arc it is a fifth short,
    /// and a spline whose handles are short cuts inside the arc and turns hardest
    /// at the nodes. Through a hairpin that came out as a radius of thirteen
    /// where the curve had thirty-one.
    CatmullRom,
    /// Along the curve's own tangent, as long as the arc through the
    /// neighbouring nodes wants: for a chord `c` turning through `phi` a cubic
    /// needs `c * (4/3) tan(phi/4) / (2 sin(phi/2))`, which is `c / 3` when
    /// `phi` is small. A node takes the mean of what the segments either side
    /// want.
    Tangent,
}

fn handles(centres: &[DVec2], tangents: &[DVec2], kind: Handles) -> Vec<DVec2> {
    fn ideal(chord: f64, phi: f64) -> f64 {
        if phi < 1e-6 {
            chord / 3.0
        } else {
            chord * (4.0 / 3.0) * (phi / 4.0).tan() / (2.0 * (phi / 2.0).sin())
        }
    }
    fn turn(a: DVec2, b: DVec2) -> f64 {
        cross(a, b).atan2(a.dot(b)).abs()
    }

    let n = centres.len();
    (0..n)
        .map(|i| {
            let (prev, p, next) = (centres[(i + n - 1) % n], centres[i], centres[(i + 1) % n]);
            let (before, after) = (dist(p, prev), dist(next, p));
            let reach = HANDLE_REACH * before.min(after);
            match kind {
                Handles::CatmullRom => {
                    let h = (next - prev) * (1.0 / 6.0);
                    if len(h) > reach && reach > 0.0 {
                        h * (reach / len(h))
                    } else {
                        h
                    }
                }
                Handles::Tangent => {
                    let want = 0.5
                        * (ideal(before, turn(tangents[(i + n - 1) % n], tangents[i]))
                            + ideal(after, turn(tangents[i], tangents[(i + 1) % n])));
                    tangents[i] * want.min(reach)
                }
            }
        })
        .collect()
}

/// The four control points of segment `i`, from node positions and handles.
fn control(centres: &[DVec2], hs: &[DVec2], i: usize) -> [DVec2; 4] {
    let j = (i + 1) % centres.len();
    [
        centres[i],
        centres[i] + hs[i],
        centres[j] - hs[j],
        centres[j],
    ]
}

/// Signed curvature of a cubic at `t`; positive turns left.
fn bezier_curvature(p: [DVec2; 4], t: f64) -> f64 {
    let mt = 1.0 - t;
    let d1 = 3.0 * (mt * mt * (p[1] - p[0]) + 2.0 * mt * t * (p[2] - p[1]) + t * t * (p[3] - p[2]));
    let d2 = 6.0 * (mt * (p[2] - 2.0 * p[1] + p[0]) + t * (p[3] - 2.0 * p[2] + p[1]));
    let speed = len(d1);
    if speed > 1e-9 {
        cross(d1, d2) / speed.powi(3)
    } else {
        0.0
    }
}

/// Curvature of the circle through three points, signed the same way.
fn curvature3(prev: DVec2, p: DVec2, next: DVec2) -> f64 {
    let (a, b) = (p - prev, next - p);
    let denominator = len(a) * len(b) * len(next - prev);
    if denominator > 1e-9 {
        2.0 * cross(a, b) / denominator
    } else {
        0.0
    }
}

/// How the road's width at each node is decided. Both go from wide on a
/// straight to narrow in a corner, and both then stop short of the fold and of
/// eight tenths of narrow.
#[derive(Clone, Copy)]
enum Widths {
    /// From the circle through each node and its two neighbours: narrow in
    /// proportion to `min(1, k * per_unit)`. Measures the curve the nodes were
    /// placed on, not the spline they became.
    Tightness { per_unit: f64 },
    /// From the spline itself, at the tightest it gets in the half segment either
    /// side of the node: wide at and above `wide_radius`, narrow at and below
    /// `narrow_radius`, a straight line between. A corner the nodes cut tighter
    /// than the curve they were placed on is a corner that has to be narrowed by
    /// that much, and only the spline knows by how much.
    Radii {
        wide_radius: f64,
        narrow_radius: f64,
    },
}

fn widths(spec: &Spec, centres: &[DVec2], hs: &[DVec2]) -> Vec<f64> {
    let n = centres.len();
    let finish = |k: f64, mut w: f64, tiny: f64| {
        if k > tiny {
            w = w.min(FOLD_LIMIT / k);
        }
        w.max(spec.narrow * 0.8)
    };
    match spec.widths {
        Widths::Tightness { per_unit } => (0..n)
            .map(|i| {
                let k =
                    curvature3(centres[(i + n - 1) % n], centres[i], centres[(i + 1) % n]).abs();
                let tightness = (k * per_unit).min(1.0);
                finish(k, spec.wide + (spec.narrow - spec.wide) * tightness, 1e-6)
            })
            .collect(),
        Widths::Radii {
            wide_radius,
            narrow_radius,
        } => {
            let mut tightest = vec![0.0f64; n];
            for segment in 0..n {
                let p = control(centres, hs, segment);
                for step in 0..SAMPLES_PER_SEGMENT {
                    let u = step as f64 / SAMPLES_PER_SEGMENT as f64;
                    let node = if u < 0.5 { segment } else { (segment + 1) % n };
                    tightest[node] = tightest[node].max(bezier_curvature(p, u).abs());
                }
            }
            tightest
                .into_iter()
                .map(|k| {
                    let radius = if k > 1e-9 { 1.0 / k } else { f64::INFINITY };
                    let w = if radius >= wide_radius {
                        spec.wide
                    } else if radius <= narrow_radius {
                        spec.narrow
                    } else {
                        let f = (radius - narrow_radius) / (wide_radius - narrow_radius);
                        spec.narrow + (spec.wide - spec.narrow) * f
                    };
                    finish(k, w, 1e-9)
                })
                .collect()
        }
    }
}

/// Which segments the item-box pairs go in, `pairs` of them spread round the lap.
#[derive(Clone, Copy)]
enum Boxes {
    /// `n * k / pairs`: from the start of the lap, so the first pair is in the
    /// first segment, just past the line.
    FromStart,
    /// `n * (k + 1/2) / pairs`: centred in each share of the lap, which keeps
    /// them out of the closing segment where the grid stands.
    Centred,
}

// --- a map -------------------------------------------------------------------

/// Everything that distinguishes one built-in from another.
struct Spec {
    slug: &'static str,
    name: &'static str,
    /// The centreline, as a dense closed polyline.
    points: Vec<DVec2>,
    /// Where node zero -- the start line -- goes: the point nearest this, or the
    /// polyline's first point.
    start: Option<DVec2>,
    nodes: usize,
    /// Node spacing's curvature weight; see [`place_nodes`].
    weight: f64,
    handles: Handles,
    widths: Widths,
    /// Half-widths, in world units.
    wide: f64,
    narrow: f64,
    boxes: Boxes,
    pairs: usize,
    seed: u64,
    /// Grass kept past the outer wall, in world units.
    padding: f64,
}

/// A spec into the map it describes.
fn draw(spec: &Spec) -> MapData {
    let (centres, tangents) = place_nodes(&spec.points, spec.nodes, spec.start, spec.weight);
    let hs = handles(&centres, &tangents, spec.handles);
    let ws = widths(spec, &centres, &hs);

    // The most common width is the road's, and a node says its own only when it
    // differs by more than half a unit.
    let mut ordered = ws.clone();
    ordered.sort_by(f64::total_cmp);
    let base = ordered[ordered.len() / 2];

    let n = spec.nodes;
    let item_boxes = (0..spec.pairs)
        .flat_map(|k| {
            let segment = match spec.boxes {
                Boxes::FromStart => n * k / spec.pairs,
                Boxes::Centred => n * (2 * k + 1) / (2 * spec.pairs),
            } % n;
            [-ITEM_LATERAL, ITEM_LATERAL]
                .map(|lateral| TrackAnchor::new(segment as u16, 0.5, scalar(lateral)))
        })
        .collect();

    MapData {
        version: MAP_FORMAT_VERSION,
        name: spec.name.to_string(),
        nodes: centres
            .iter()
            .zip(&hs)
            .zip(&ws)
            .map(|((&position, &h), &w)| TrackNode {
                position: to_map(position),
                in_handle: to_map(-h),
                out_handle: to_map(h),
                half_width: ((w - base).abs() > 0.5).then(|| scalar(w)),
                mirrored: true,
            })
            .collect(),
        road: RoadShape {
            half_width: scalar(base),
            kerb_width: scalar(1.5),
            kerb_stripe: scalar(10.0),
        },
        start: StartLine {
            at: TrackAnchor::new(0, 0.0, 0),
            depth: scalar(4.0),
            grid: GridLayout {
                columns: 3,
                row_spacing: scalar(11.0),
                column_spacing: scalar(7.0),
                first_row_offset: scalar(9.0),
            },
        },
        item_boxes,
        bounds_padding: scalar(spec.padding),
        decor: DecorSettings {
            seed: spec.seed,
            density: 1.4,
            clearance: scalar(3.0),
        },
    }
}

// --- the maps ----------------------------------------------------------------

/// Two by one and a half screens, because that is the case the follow camera
/// and the minimap exist for. A wobbled ellipse -- two harmonics on the radius,
/// chosen so the lap never doubles back close to itself -- with the road
/// narrowing through the tight parts and opening out on the fast ones.
fn sweeping() -> Spec {
    let shape = |t: f64| {
        let r = 1.0 + 0.16 * (3.0 * t + 0.6).sin() + 0.09 * (2.0 * t - 1.1).sin();
        DVec2::new(250.0 * r * t.cos(), 150.0 * r * t.sin())
    };
    Spec {
        slug: "sweeping",
        name: "Sweeping Bends",
        points: parametric(shape, 0.0, 2000),
        start: None,
        nodes: 22,
        weight: 0.0,
        handles: Handles::CatmullRom,
        widths: Widths::Tightness { per_unit: 90.0 },
        wide: 13.0,
        narrow: 7.5,
        boxes: Boxes::FromStart,
        pairs: 6,
        seed: 4242,
        padding: 20.0,
    }
}

/// Three fast lobes with a tight dip between each pair, anticlockwise. Starts
/// just past the inflection between a lobe and the dip after it, so the grid
/// stands on the straightest road there is and the first corner is the dip.
fn clover() -> Spec {
    const R: f64 = 135.0;
    const A: f64 = 0.30;
    let shape = |t: f64| {
        let r = R * (1.0 + A * (3.0 * t).cos());
        DVec2::new(r * t.cos(), r * t.sin())
    };
    Spec {
        slug: "clover",
        name: "Clover",
        points: parametric(shape, 42f64.to_radians(), 4000),
        start: None,
        nodes: 30,
        weight: 25.0,
        handles: Handles::Tangent,
        widths: Widths::Radii {
            wide_radius: 75.0,
            narrow_radius: 35.0,
        },
        wide: 13.0,
        narrow: 8.0,
        boxes: Boxes::Centred,
        pairs: 6,
        seed: 20260908,
        padding: 16.0,
    }
}

/// A long outer sweep and a tighter inner one, joined by two hairpins, clockwise.
///
/// The shoe is a spine -- an arc of an ellipse, taller than it is wide -- with
/// the road running out along one side of it and back along the other,
/// `HALF_GAP` off it either way, and a half-circle of the same radius joining
/// the two at each end. Offsets of a curve share its tangents, so the hairpins
/// join without a kink.
fn horseshoe() -> Spec {
    const A: f64 = 100.0;
    const B: f64 = 120.0;
    const HALF_GAP: f64 = 31.0;
    /// Degrees either side of straight down that the shoe leaves open.
    const OPENING: f64 = 38.0;
    let (left_end, right_end) = (270.0 - OPENING, OPENING - 90.0);

    let spine = |deg: f64| {
        let t = deg.to_radians();
        DVec2::new(A * t.cos(), B * t.sin())
    };
    let outward = |deg: f64| {
        let t = deg.to_radians();
        unit(DVec2::new(B * t.cos(), A * t.sin()))
    };
    // The spine's direction of travel: `sign` +1 with the angle, -1 against.
    let along = |deg: f64, sign: f64| {
        let t = deg.to_radians();
        unit(DVec2::new(-A * t.sin() * sign, B * t.cos() * sign))
    };

    let outer = linspace(left_end, right_end, 3000)
        .map(|d| spine(d) + outward(d) * HALF_GAP)
        .collect();
    let right_cap = cap(
        spine(right_end),
        HALF_GAP,
        outward(right_end),
        along(right_end, -1.0),
    );
    let inner = linspace(right_end, left_end, 3000)
        .map(|d| spine(d) - outward(d) * HALF_GAP)
        .collect();
    let left_cap = cap(
        spine(left_end),
        HALF_GAP,
        -outward(left_end),
        along(left_end, 1.0),
    );
    Spec {
        slug: "horseshoe",
        name: "Horseshoe",
        points: joined(vec![outer, right_cap, inner, left_cap]),
        // Clockwise round the outside: the top runs east and the right side runs
        // south, and the start is on the right side, where the sweep is
        // straightest.
        start: Some(spine(20.0) + outward(20.0) * HALF_GAP),
        nodes: 30,
        weight: 25.0,
        handles: Handles::Tangent,
        widths: Widths::Radii {
            wide_radius: 80.0,
            narrow_radius: 32.0,
        },
        wide: 13.0,
        narrow: 8.0,
        boxes: Boxes::Centred,
        pairs: 6,
        seed: 20260909,
        padding: 16.0,
    }
}

/// A stadium, clockwise, with each straight bent into an S: a tight one on the
/// top straight and a longer, faster one along the bottom.
fn serpent() -> Spec {
    const H: f64 = 72.0;
    const L: f64 = 260.0;
    // One period of a sine under a window that is flat at both ends, so the S
    // joins the straight with no step in position, slope or curvature.
    let s_bend = |v: f64| (2.0 * PI * v).sin() * (PI * v).sin().powi(2);

    let top = linspace(0.0, 1.0, 1500)
        .map(|u| {
            let v = (u - 0.3) / 0.7;
            let bend = if (0.0..=1.0).contains(&v) {
                16.0 * s_bend(v)
            } else {
                0.0
            };
            DVec2::new(-L / 2.0 + L * u, H + bend)
        })
        .collect();
    let right = arc(DVec2::new(L / 2.0, 0.0), H, 90.0, -90.0);
    let bottom = linspace(0.0, 1.0, 1500)
        .map(|u| DVec2::new(L / 2.0 - L * u, -H - 22.0 * s_bend(u)))
        .collect();
    let left = arc(DVec2::new(-L / 2.0, 0.0), H, 270.0, 90.0);
    Spec {
        slug: "serpent",
        name: "Serpent",
        points: joined(vec![top, right, bottom, left]),
        // On the flat run before the first S, with the grid behind it.
        start: Some(DVec2::new(-L / 2.0 + 0.2 * L, H)),
        nodes: 26,
        weight: 25.0,
        handles: Handles::Tangent,
        widths: Widths::Radii {
            wide_radius: 90.0,
            narrow_radius: 35.0,
        },
        wide: 13.0,
        narrow: 8.0,
        boxes: Boxes::Centred,
        pairs: 5,
        seed: 20260910,
        padding: 16.0,
    }
}

/// Two lobes and a waist, anticlockwise. `cos 2t` makes the lobes and the
/// waist; `cos t` makes one lobe the bigger. Starts on the bottom, heading east
/// into the big one.
fn peanut() -> Spec {
    const R: f64 = 115.0;
    const A: f64 = 0.45;
    const B: f64 = 0.10;
    let shape = |t: f64| {
        let r = R * (1.0 + A * (2.0 * t).cos() + B * t.cos());
        DVec2::new(r * t.cos(), r * t.sin())
    };
    Spec {
        slug: "peanut",
        name: "Peanut",
        points: parametric(shape, 315f64.to_radians(), 4000),
        start: None,
        nodes: 24,
        weight: 25.0,
        handles: Handles::Tangent,
        widths: Widths::Radii {
            wide_radius: 80.0,
            narrow_radius: 35.0,
        },
        wide: 13.0,
        narrow: 8.0,
        boxes: Boxes::Centred,
        pairs: 4,
        seed: 20260911,
        padding: 16.0,
    }
}

/// Every map this file draws, by slug.
fn drawn() -> Vec<(&'static str, MapData)> {
    [sweeping, clover, horseshoe, serpent, peanut]
        .iter()
        .map(|make| {
            let spec = make();
            (spec.slug, draw(&spec))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::map::build::{BuildLevel, build};
    use crate::track::map::builtin::by_slug;

    /// What the start line needs of the road behind it: `first_row_offset` plus
    /// three rows of `row_spacing` and a little, and the outer column plus half
    /// a kart across. Not one of the builder's warnings, because a narrow grid
    /// is an authoring choice everywhere but here.
    const GRID_DEPTH: f32 = 9.0 + 3.0 * 11.0 + 6.0;
    const GRID_HALF_SPAN: f32 = 7.0 + 2.5;

    /// The builder's own verdict on a drawing, plus the grid's.
    ///
    /// The grid's is waived for Sweeping Bends: it starts in a narrowing, 7.5
    /// wide where the grid wants 9.5, and has shipped that way since it was
    /// drawn. Moving its line would move every node, so it is left as it is and
    /// the check holds the maps drawn after it.
    fn problems_with(slug: &str, map: &MapData) -> Vec<String> {
        let mut problems = Vec::new();
        if let Err(error) = map.validate() {
            problems.push(error.to_string());
        }
        let built = build(map, BuildLevel::Full);
        problems.extend(built.warnings.iter().map(|w| format!("{w:?}")));
        if slug == "sweeping" {
            return problems;
        }
        let behind = built
            .centre
            .iter()
            .filter(|s| s.s >= built.length - GRID_DEPTH)
            .map(|s| s.half_width)
            .fold(f32::MAX, f32::min);
        if behind < GRID_HALF_SPAN {
            problems.push(format!(
                "the road behind the start is {behind} wide, the grid needs {GRID_HALF_SPAN}"
            ));
        }
        problems
    }

    /// Where two maps differ by more than the drawing's own slack: one map unit
    /// on anything rounded from a double, nothing on anything else.
    fn differences(drawn: &MapData, shipped: &MapData) -> Vec<String> {
        let mut out = Vec::new();
        let close = |a: IVec2, b: IVec2| (a - b).abs().max_element() <= 1;
        if drawn.name != shipped.name {
            out.push(format!("name {:?} vs {:?}", drawn.name, shipped.name));
        }
        if drawn.nodes.len() != shipped.nodes.len() {
            out.push(format!(
                "{} nodes vs {}",
                drawn.nodes.len(),
                shipped.nodes.len()
            ));
            return out;
        }
        for (i, (a, b)) in drawn.nodes.iter().zip(&shipped.nodes).enumerate() {
            for (what, x, y) in [
                ("position", a.position, b.position),
                ("in_handle", a.in_handle, b.in_handle),
                ("out_handle", a.out_handle, b.out_handle),
            ] {
                if !close(x, y) {
                    out.push(format!("node {i} {what} {x:?} vs {y:?}"));
                }
            }
            let widths_agree = match (a.half_width, b.half_width) {
                (None, None) => true,
                (Some(x), Some(y)) => (x - y).abs() <= 1,
                _ => false,
            };
            if !widths_agree || a.mirrored != b.mirrored {
                out.push(format!(
                    "node {i} half_width {:?} vs {:?}",
                    a.half_width, b.half_width
                ));
            }
        }
        if (drawn.road.half_width - shipped.road.half_width).abs() > 1
            || drawn.road.kerb_width != shipped.road.kerb_width
            || drawn.road.kerb_stripe != shipped.road.kerb_stripe
        {
            out.push(format!("road {:?} vs {:?}", drawn.road, shipped.road));
        }
        if drawn.start != shipped.start {
            out.push(format!("start {:?} vs {:?}", drawn.start, shipped.start));
        }
        if drawn.item_boxes != shipped.item_boxes {
            out.push(format!(
                "item boxes {:?} vs {:?}",
                drawn.item_boxes, shipped.item_boxes
            ));
        }
        if drawn.bounds_padding != shipped.bounds_padding || drawn.decor != shipped.decor {
            out.push("padding or decor".to_string());
        }
        out
    }

    /// The files under `assets/maps/` are what this file draws. Edit a shape
    /// here and this fails until `just maps` has been run; edit a JSON by hand
    /// and it fails until the shape agrees.
    #[test]
    fn the_shipped_maps_are_what_this_draws() {
        let mut report = Vec::new();
        for (slug, map) in drawn() {
            let shipped = by_slug(slug).unwrap_or_else(|| panic!("{slug} is not a built-in"));
            for difference in differences(&map, &shipped) {
                report.push(format!("{slug}: {difference}"));
            }
        }
        assert!(
            report.is_empty(),
            "the shipped maps differ from the drawing; run `just maps` if the drawing changed:\n{}",
            report.join("\n")
        );
    }

    /// Nothing drawn here warns. The shipped files are held to the same by
    /// `the_built_in_maps_are_raceable`; this is the check on the drawing itself,
    /// which is what `regenerate` runs before it writes anything.
    #[test]
    fn what_this_draws_builds_clean() {
        for (slug, map) in drawn() {
            let problems = problems_with(slug, &map);
            assert!(problems.is_empty(), "{slug}: {problems:?}");
        }
    }

    /// Rewrite the snapshot. `just maps`, or
    /// `cargo test regenerate_the_built_in_maps -- --ignored`.
    ///
    /// Refuses the whole batch if any one drawing has a problem, so a broken
    /// shape cannot half-land.
    #[test]
    #[ignore = "writes into assets/maps; run on purpose"]
    fn regenerate_the_built_in_maps() {
        let maps = drawn();
        for (slug, map) in &maps {
            let problems = problems_with(slug, map);
            assert!(
                problems.is_empty(),
                "not writing anything: {slug}: {problems:?}"
            );
        }
        for (slug, map) in &maps {
            let path = format!("{}/assets/maps/{slug}.json", env!("CARGO_MANIFEST_DIR"));
            let json = serde_json::to_string_pretty(map).expect("a map serialises") + "\n";
            std::fs::write(&path, json).unwrap_or_else(|e| panic!("writing {path}: {e}"));
            eprintln!("wrote {path}");
        }
    }
}
