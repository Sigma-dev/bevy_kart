//! The track a new track starts as.
//!
//! Not a built-in: nothing races it and it is not in the picker. It is what the
//! editor's **New** button hands you, and its whole job is to be understood at a
//! glance -- eight nodes, one loop, no corner that needs explaining. What it
//! replaced was the classic circuit renamed, which is forty-eight hand-converted
//! nodes and a shape somebody else already decided: everything an example should
//! not be, and a first edit that begins by deleting.
//!
//! Built here rather than shipped as JSON because a reader should be able to see
//! that it is an oval this big, not eight pairs of coordinates that happen to
//! lie on one.

use bevy::prelude::Vec2;

use super::data::{
    DecorSettings, GridLayout, MAP_FORMAT_VERSION, MapData, RoadShape, StartLine, TrackAnchor,
    TrackNode, scalar_to_map, to_map,
};

/// Half the oval, along and across. Its outer edge comes to 200 by 116, which
/// leaves the whole thing inside one 256x144 screen: a new track is something
/// you can see all of before you have learnt what the camera does.
const ALONG: f32 = 88.0;
const ACROSS: f32 = 46.0;

/// A road the width of the one the game ships, so a new track feels like the
/// game rather than like a diagram.
const HALF_WIDTH: f32 = 12.0;

/// Eight nodes, at forty-five degrees to each other.
///
/// Written as a table rather than taken from `sin` and `cos` because every value
/// at forty-five degrees is exact: zero, one, or one over root two. A map is
/// content-hashed for the network and its coordinates are integers, so nothing
/// here *needs* to be bit-identical across platforms -- but a table that cannot
/// round differently is less to think about than one that can.
const AROUND: [Vec2; 8] = {
    const D: f32 = core::f32::consts::FRAC_1_SQRT_2;
    [
        // Anticlockwise from the bottom, so node zero is the start line and the
        // bottom straight runs left to right, the way the grid faces.
        Vec2::new(0.0, -1.0),
        Vec2::new(D, -D),
        Vec2::new(1.0, 0.0),
        Vec2::new(D, D),
        Vec2::new(0.0, 1.0),
        Vec2::new(-D, D),
        Vec2::new(-1.0, 0.0),
        Vec2::new(-D, -D),
    ]
};

/// The map a new track begins as.
pub fn starter_map() -> MapData {
    // A cubic bezier matching a parametric curve over a step of `dt` takes
    // handles of `dt / 3` times the derivative there. An eighth of a turn is
    // `PI / 4`, so the handles are `PI / 12` of the tangent -- which for a
    // circle comes out within two parts in a hundred of the exact answer, and
    // for this oval well inside the quarter-unit the coordinates are stored in.
    const REACH: f32 = core::f32::consts::PI / 12.0;
    let nodes = AROUND
        .iter()
        .map(|unit| {
            let position = Vec2::new(ALONG * unit.x, ACROSS * unit.y);
            // The derivative of `(ALONG cos t, ACROSS sin t)`, which is the
            // table turned a quarter turn and re-scaled.
            let out = Vec2::new(-ALONG * unit.y, ACROSS * unit.x) * REACH;
            TrackNode {
                position: to_map(position),
                in_handle: to_map(-out),
                out_handle: to_map(out),
                half_width: None,
                mirrored: true,
            }
        })
        .collect();

    MapData {
        version: MAP_FORMAT_VERSION,
        name: "New Track".to_string(),
        nodes,
        road: RoadShape {
            half_width: scalar_to_map(HALF_WIDTH),
            kerb_width: scalar_to_map(2.0),
            kerb_stripe: scalar_to_map(9.0),
        },
        start: StartLine {
            at: TrackAnchor::new(0, 0.0, 0),
            depth: scalar_to_map(4.0),
            grid: GridLayout {
                columns: 3,
                row_spacing: scalar_to_map(10.0),
                column_spacing: scalar_to_map(7.0),
                first_row_offset: scalar_to_map(9.0),
            },
        },
        // Four, on the two straights. Not for the racing -- for the editor,
        // which tells you a map with none is unfinished, and which a new map has
        // no business saying on the first frame.
        item_boxes: vec![
            TrackAnchor::new(0, 0.35, scalar_to_map(-5.0)),
            TrackAnchor::new(0, 0.35, scalar_to_map(5.0)),
            TrackAnchor::new(4, 0.35, scalar_to_map(-5.0)),
            TrackAnchor::new(4, 0.35, scalar_to_map(5.0)),
        ],
        bounds_padding: scalar_to_map(12.0),
        decor: DecorSettings {
            seed: 7,
            density: 1.0,
            clearance: scalar_to_map(3.0),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::map::build::{BuildLevel, build};

    /// It races, and it does it without a word of complaint. A starting point
    /// that opens with a warning teaches the warning, not the editor.
    #[test]
    fn the_starter_is_raceable_and_says_nothing() {
        let map = starter_map();
        assert_eq!(map.validate(), Ok(()));
        let built = build(&map, BuildLevel::Full);
        assert!(built.warnings.is_empty(), "{:?}", built.warnings);
        assert_eq!(built.item_boxes.len(), 4);
    }

    /// Small: a lap you can drive while still deciding what to change, and a
    /// shape that fits one screen.
    #[test]
    fn the_starter_is_smaller_than_the_track_it_replaced() {
        let starter = build(&starter_map(), BuildLevel::Full);
        let classic = build(&crate::track::map::by_slug("classic").unwrap(), BuildLevel::Full);
        assert!(
            starter.length < classic.length / 1.5,
            "a {} lap against the classic track's {}",
            starter.length,
            classic.length
        );
        assert!(
            starter_map().nodes.len() * 4 < classic.map.nodes.len(),
            "{} nodes is not an example",
            starter_map().nodes.len()
        );
        let size = starter.bounds.size();
        assert!(
            size.x <= crate::RESOLUTION.x && size.y <= crate::RESOLUTION.y,
            "the starter does not fit one screen: {size:?}"
        );
    }

    /// And it really is the oval it is written as, rather than eight nodes that
    /// nearly lie on one -- which is the only thing the handle length above can
    /// get wrong.
    #[test]
    fn the_starter_is_the_oval_it_says_it_is() {
        let built = build(&starter_map(), BuildLevel::Preview);
        for sample in &built.centre {
            // On the ellipse `(x/a)^2 + (y/b)^2 = 1`. Compared as a radius so
            // the tolerance is in units rather than in whatever that sum is.
            let normalised = Vec2::new(sample.position.x / ALONG, sample.position.y / ACROSS);
            let off = (normalised.length() - 1.0).abs() * ACROSS;
            assert!(off < 0.5, "sample at {:?} is {off} off the oval", sample.position);
        }
    }
}
