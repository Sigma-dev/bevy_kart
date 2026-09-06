//! What a track *is*, before any geometry is derived from it.
//!
//! This is the only thing that is persisted, edited and sent over the network,
//! so it is deliberately dull: plain scalars, `Vec`s and `Option`s, no
//! `#[serde(flatten)]`, no untagged enums, and **no maps**. It round-trips
//! through `serde_json` (storage, and hand-editable built-ins) and `postcard`
//! (the wire) unchanged.
//!
//! # Why the coordinates are integers
//!
//! Every peer builds its own wall colliders from this data -- they are not
//! networked entities -- so two peers that disagree about where a wall is do not
//! wobble, they diverge permanently, and rollback re-corrects the same
//! disagreement every tick forever. The geometry has to be *bit*-identical
//! across x86, ARM and wasm.
//!
//! Storing positions as [`IVec2`] in 1/256 world units removes a whole class of
//! ways that can go wrong at once: `i32 as f32` is exactly rounded on every
//! target (and exact outright below 2^24, which is 65536 world units -- far past
//! any map), and dividing by a power of two is exact, so [`to_world`] returns
//! the identical bit pattern everywhere. It also means a hand-edited file cannot
//! smuggle a NaN or an infinity into the physics engine, a JSON round-trip
//! cannot lose a digit, and an editor drag cannot accumulate error in the low
//! bits of a float. And it gives the format a *stable content hash*, which is
//! what the network layer identifies a map by.
//!
//! `i32` rather than the more obvious `i16`: at 1/256, an `i16` would cap a map
//! at ±128 world units, which is smaller than the track this game already has.
//! After varint encoding an `i32` costs three bytes for anything realistic.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Bumped when the *meaning* of a field changes. A file claiming a version this
/// build does not know is refused at load rather than half-read into a track
/// with a wall in the wrong place.
pub const MAP_FORMAT_VERSION: u32 = 1;

/// Sub-units per world unit. A power of two, so the conversion is exact.
pub const MAP_UNITS_PER_WORLD: f32 = 256.0;

/// Map units to world units, exactly, on every target.
#[inline]
pub fn to_world(p: IVec2) -> Vec2 {
    Vec2::new(p.x as f32, p.y as f32) / MAP_UNITS_PER_WORLD
}

/// One map-unit scalar to world units.
#[inline]
pub fn scalar_to_world(v: i32) -> f32 {
    v as f32 / MAP_UNITS_PER_WORLD
}

/// World units back to map units, rounding to the nearest representable value.
///
/// The editor's only way in. Rounding rather than truncating so dragging a point
/// left and back again lands where it started.
#[inline]
pub fn to_map(p: Vec2) -> IVec2 {
    IVec2::new(
        (p.x * MAP_UNITS_PER_WORLD).round() as i32,
        (p.y * MAP_UNITS_PER_WORLD).round() as i32,
    )
}

/// One world scalar to map units.
#[inline]
pub fn scalar_to_map(v: f32) -> i32 {
    (v * MAP_UNITS_PER_WORLD).round() as i32
}

/// One node of the closed track spline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackNode {
    pub position: IVec2,
    /// Handle offsets **relative to `position`**, so dragging a node carries its
    /// handles for free and translating a whole map is `position += delta`
    /// alone. `out_handle` shapes the segment leaving this node, `in_handle` the
    /// one arriving at it.
    pub in_handle: IVec2,
    pub out_handle: IVec2,
    /// Road half-width **at this node**. `None` inherits [`RoadShape::half_width`],
    /// so a map is authored at one width and then pinched or opened out node by
    /// node without touching the rest. Interpolated along each segment; see
    /// [`MapData::half_width_at`].
    pub half_width: Option<i32>,
    /// While true the two handles mirror each other's direction, which is the
    /// ordinary smooth node. The editor breaks the pair to make a cusp.
    pub mirrored: bool,
}

/// A place on the track, said in the track's terms rather than the world's.
///
/// Anchored to a *segment* rather than to a distance around the whole lap, on
/// purpose. Normalised lap distance would look tidier, but every node drag
/// changes the total length, so moving one corner would shift every item box on
/// the map. An anchor on segment 3 stays put when node 12 moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackAnchor {
    pub segment: u16,
    /// Curve parameter within that segment, as a fraction of `u16::MAX + 1`.
    /// Quantised for the same reasons the positions are, and because `t = 1` of
    /// one segment is `t = 0` of the next, so the open interval loses nothing.
    pub t: u16,
    /// World offset across the centreline, in map units. Positive is left of the
    /// direction of travel.
    ///
    /// World units and not a fraction of the width: an item box a kart-width
    /// left of centre should stay a kart-width from centre when the author
    /// widens the road, and a fraction would slide it outward instead.
    pub lateral: i32,
}

impl TrackAnchor {
    pub fn new(segment: u16, t: f32, lateral: i32) -> Self {
        Self {
            segment,
            t: quantise_t(t),
            lateral,
        }
    }

    /// This anchor's curve parameter, in `0.0..1.0`.
    #[inline]
    pub fn t_fraction(&self) -> f32 {
        self.t as f32 / 65536.0
    }

    /// Where this sits in the curve's own global parameter, which runs
    /// `0..segment_count`.
    #[inline]
    pub fn global_t(&self) -> f32 {
        self.segment as f32 + self.t_fraction()
    }
}

/// `0.0..1.0` to the `u16` fraction [`TrackAnchor`] stores.
#[inline]
pub fn quantise_t(t: f32) -> u16 {
    (t.clamp(0.0, 1.0) * 65536.0).round().min(65535.0) as u16
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoadShape {
    /// The width every node falls back to. Only a default; see
    /// [`TrackNode::half_width`].
    pub half_width: i32,
    /// Red/white band inside each edge. Constant, so it keeps its width as the
    /// road body narrows and widens around it. Zero disables kerbs.
    pub kerb_width: i32,
    /// World length of one kerb stripe.
    pub kerb_stripe: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartLine {
    /// Progress zero. The polyline handed to `ProgressLine` begins here, so the
    /// lap counter's wrap is the finish line rather than whichever node happened
    /// to be written first.
    pub at: TrackAnchor,
    /// Depth of the painted band, along the direction of travel.
    pub depth: i32,
    pub grid: GridLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridLayout {
    pub columns: u32,
    /// Back along the track, between rows.
    pub row_spacing: i32,
    /// Across the track, between columns.
    pub column_spacing: i32,
    /// How far back the first row sits from the line.
    pub first_row_offset: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecorSettings {
    pub seed: u64,
    /// Elements per 1000 square world units of ground outside the road.
    pub density: f32,
    /// How close to the road edge an element may be placed.
    pub clearance: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapData {
    pub version: u32,
    pub name: String,
    /// The closed loop. Segment `i` runs `nodes[i] -> nodes[(i + 1) % len]`; the
    /// closing segment is implicit, so there is no duplicated last node to keep
    /// in sync. At least [`MIN_NODES`].
    pub nodes: Vec<TrackNode>,
    pub road: RoadShape,
    pub start: StartLine,
    pub item_boxes: Vec<TrackAnchor>,
    /// Grass kept visible past the outer wall. The map's bounds are *derived*
    /// from the geometry plus this and never stored: an authored rectangle that
    /// does not contain its own track is a bug class that cannot exist if the
    /// rectangle is computed.
    pub bounds_padding: i32,
    pub decor: DecorSettings,
}

/// Below this there is no closed loop to speak of.
pub const MIN_NODES: usize = 3;

/// A road narrower than this is not drivable, so the builder refuses to make one
/// however hard a node is dragged.
pub const MIN_HALF_WIDTH: i32 = (2.0 * MAP_UNITS_PER_WORLD) as i32;

/// Why a map could not be loaded. Distinct from `TrackWarning`, which is about a
/// map that loads but has something wrong with its shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapError {
    /// Written by a build that knew something this one does not.
    UnknownVersion(u32),
    TooFewNodes(usize),
    /// A `segment` index that is not a segment.
    AnchorOutOfRange { segment: u16, segments: usize },
    NoName,
}

impl core::fmt::Display for MapError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MapError::UnknownVersion(v) => write!(
                f,
                "map format version {v} is newer than this build understands ({MAP_FORMAT_VERSION})"
            ),
            MapError::TooFewNodes(n) => {
                write!(f, "a track needs at least {MIN_NODES} nodes, this has {n}")
            }
            MapError::AnchorOutOfRange { segment, segments } => write!(
                f,
                "an anchor refers to segment {segment}, but the track has {segments}"
            ),
            MapError::NoName => write!(f, "the map has no name"),
        }
    }
}

impl MapData {
    pub fn segment_count(&self) -> usize {
        self.nodes.len()
    }

    /// The road half-width at a node, in world units, after the fallback.
    ///
    /// Reports what the author asked for, including a value too small to build a
    /// road from. The floor is applied by the builder, which also says that it
    /// had to -- clamping silently here would make that warning impossible.
    #[inline]
    pub fn node_half_width(&self, index: usize) -> f32 {
        scalar_to_world(self.nodes[index].half_width.unwrap_or(self.road.half_width))
    }

    /// The road half-width part-way along a segment, in world units.
    ///
    /// Smoothstep rather than a lerp, and this is the whole reason the road can
    /// change width without looking wrong: smoothstep's derivative is zero at
    /// both ends, so the width profile stays C1 *across* a node even though the
    /// segments either side of it have different arc lengths. A lerp would put a
    /// visible crease in the road edge at every node where the width changes.
    ///
    /// Only multiplies and subtracts, so it is inside the set of operations the
    /// builder is allowed (see `build.rs`) and adds no determinism risk.
    #[inline]
    pub fn half_width_at(&self, segment: usize, u: f32) -> f32 {
        let a = self.node_half_width(segment);
        let b = self.node_half_width((segment + 1) % self.nodes.len());
        let e = u * u * (3.0 - 2.0 * u);
        a + (b - a) * e
    }

    /// The four control points of one segment, in world units.
    pub fn segment_control_points(&self, segment: usize) -> [Vec2; 4] {
        let a = &self.nodes[segment];
        let b = &self.nodes[(segment + 1) % self.nodes.len()];
        let p0 = to_world(a.position);
        let p3 = to_world(b.position);
        [
            p0,
            p0 + to_world(a.out_handle),
            p3 + to_world(b.in_handle),
            p3,
        ]
    }

    /// Refuse a map that cannot be built at all. Shape problems that still
    /// produce a playable track are warnings from the builder, not errors here.
    pub fn validate(&self) -> Result<(), MapError> {
        if self.version > MAP_FORMAT_VERSION {
            return Err(MapError::UnknownVersion(self.version));
        }
        if self.nodes.len() < MIN_NODES {
            return Err(MapError::TooFewNodes(self.nodes.len()));
        }
        if self.name.trim().is_empty() {
            return Err(MapError::NoName);
        }
        let segments = self.nodes.len();
        for anchor in core::iter::once(&self.start.at).chain(self.item_boxes.iter()) {
            if anchor.segment as usize >= segments {
                return Err(MapError::AnchorOutOfRange {
                    segment: anchor.segment,
                    segments,
                });
            }
        }
        Ok(())
    }

    /// Keep the anchors pointing at the same *places* after a node is removed.
    ///
    /// Deleting node `i` merges segments `i - 1` and `i` into one, so anchors on
    /// either of them have to be rescaled into the survivor, and everything
    /// after shuffles down. Without this, deleting a node in the editor teleports
    /// every item box past it.
    ///
    /// Call **before** removing the node, so the old indices still mean something.
    pub fn remap_anchors_after_node_removal(&mut self, removed: usize) {
        let segments = self.nodes.len();
        if segments <= MIN_NODES {
            return;
        }
        let prev = (removed + segments - 1) % segments;
        let remap = |anchor: &mut TrackAnchor| {
            let seg = anchor.segment as usize;
            let t = anchor.t_fraction();
            // The two merged segments each become half of the survivor. Which
            // half depends on which side of the removed node the anchor was.
            let (new_seg, new_t) = if seg == prev {
                (prev, t * 0.5)
            } else if seg == removed {
                (prev, 0.5 + t * 0.5)
            } else {
                (seg, t)
            };
            // Indices after the removed node shuffle down, and the survivor is
            // itself renumbered when it sat after the removal point.
            let shifted = if new_seg > removed { new_seg - 1 } else { new_seg };
            anchor.segment = shifted as u16;
            anchor.t = quantise_t(new_t);
        };
        remap(&mut self.start.at);
        for anchor in self.item_boxes.iter_mut() {
            remap(anchor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property the whole integer-coordinate decision rests on.
    #[test]
    fn map_units_survive_a_round_trip_through_world_units() {
        for raw in [
            IVec2::ZERO,
            IVec2::new(1, -1),
            IVec2::new(256, 512),
            IVec2::new(-31_744, 16_128),
            IVec2::new(i16::MAX as i32 * 256, i16::MIN as i32 * 256),
        ] {
            assert_eq!(to_map(to_world(raw)), raw, "{raw:?}");
        }
    }

    #[test]
    fn a_quantised_curve_parameter_stays_inside_its_segment() {
        for t in [0.0, 0.25, 0.5, 0.75, 0.999_99, 1.0, 1.5, -3.0] {
            let f = TrackAnchor::new(0, t, 0).t_fraction();
            assert!((0.0..1.0).contains(&f), "t={t} became {f}");
        }
        assert_eq!(TrackAnchor::new(0, 0.0, 0).t_fraction(), 0.0);
        assert!((TrackAnchor::new(0, 0.5, 0).t_fraction() - 0.5).abs() < 1e-4);
    }
}
