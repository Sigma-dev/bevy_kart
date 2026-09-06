//! The editor's overlay, drawn with gizmos.
//!
//! Immediate mode, so there is nothing to spawn, nothing to keep in step with
//! the map and nothing to tear down. Gizmo line width is already in screen
//! pixels, so it stays legible at every zoom -- which is exactly what a handle
//! made of entities would not do.

use bevy::prelude::*;

use crate::track::map::build::TrackWarning;
use crate::track::map::data::to_world;
use crate::RESOLUTION;

use super::cursor::EditorCursor;
use super::tools::{Hovered, Selection, width_handles};
use super::{EditorMap, Tool};

const NODE_PX: f32 = 4.5;
const HANDLE_PX: f32 = 4.0;

pub fn draw_overlay(
    mut gizmos: Gizmos,
    editor: Res<EditorMap>,
    selection: Res<Selection>,
    tool: Res<Tool>,
    cursor: Res<EditorCursor>,
) {
    let built = &editor.built;
    let scale = cursor.world_per_px.max(f32::EPSILON);

    // Where the map ends, and where the camera will stop.
    gizmos.rect_2d(
        Isometry2d::from_translation(built.bounds.center()),
        built.bounds.size(),
        Color::srgba(1., 1., 1., 0.25),
    );

    // The road edges, so a corner that is too tight for its width is visible as
    // a shape rather than only as a line of text.
    let tight: Vec<f32> = built
        .warnings
        .iter()
        .filter_map(|warning| match warning {
            TrackWarning::CornerTooTight { at_s, .. } => Some(*at_s),
            _ => None,
        })
        .collect();
    for wall in [&built.left_wall, &built.right_wall] {
        if wall.len() > 1 {
            gizmos.linestrip_2d(
                wall.iter().copied().chain(wall.first().copied()),
                Color::srgba(1., 1., 1., 0.35),
            );
        }
    }
    // One ring per reported corner, not one per sample: a circle every two units
    // along the offending stretch is a red smear rather than a location.
    for at in &tight {
        if let Some(sample) = built
            .centre
            .iter()
            .min_by(|a, b| (a.s - at).abs().total_cmp(&(b.s - at).abs()))
        {
            gizmos.circle_2d(
                Isometry2d::from_translation(sample.position),
                sample.half_width * 1.6,
                Color::srgb(1., 0.35, 0.35),
            );
        }
    }

    // The racing line, which is what the lap counter measures against.
    gizmos.linestrip_2d(
        built.progress.iter().copied().chain(built.progress.first().copied()),
        Color::srgba(1., 1., 1., 0.22),
    );

    // The start line and the grid it feeds, so a grid clipping a wall is
    // something you can see before you race it.
    let start = built.start_pose;
    let forward = Vec2::new(start.cos, start.sin);
    let across = Vec2::new(-forward.y, forward.x);
    let half = built.centre.first().map(|s| s.half_width).unwrap_or(10.0);
    gizmos.line_2d(
        start.position + across * half,
        start.position - across * half,
        Color::srgb(1., 1., 0.2),
    );
    for pose in &built.grid {
        let facing = Vec2::new(pose.cos, pose.sin);
        gizmos.rect_2d(
            Isometry2d::new(pose.position, Rot2::from_sin_cos(facing.y, facing.x)),
            Vec2::new(8., 4.),
            Color::srgba(1., 1., 0.2, 0.35),
        );
    }

    let items_live = *tool == Tool::Items;
    for (index, position) in built.item_boxes.iter().enumerate() {
        let hovered = selection.hovered == Some(Hovered::ItemBox(index));
        let colour = match (items_live, hovered) {
            (_, true) => Color::srgb(1., 0.95, 0.5),
            (true, false) => Color::srgb(1., 0.7, 0.2),
            // Dimmed in Edit, so the mode is visible on the canvas rather than
            // only as a word in the panel.
            (false, false) => Color::srgba(1., 0.7, 0.2, 0.4),
        };
        let size = if hovered { 6.0 } else { 4.0 };
        gizmos.rect_2d(
            Isometry2d::from_translation(*position),
            Vec2::splat(size),
            colour,
        );
    }

    // Nodes and their handles. Only the selected node shows handles: three
    // grabbable dots per node across a whole track is unreadable.
    let edit_live = *tool == Tool::Edit;
    for (index, node) in editor.data.nodes.iter().enumerate() {
        if !edit_live {
            // In Items mode the track is not editable, so it stops advertising
            // grips that would do nothing.
            gizmos.circle_2d(
                Isometry2d::from_translation(to_world(node.position)),
                NODE_PX * scale * 0.6,
                Color::srgba(1., 1., 1., 0.25),
            );
            continue;
        }
        let position = to_world(node.position);
        let selected = selection.node == Some(index);
        // Handles are shown for whichever node the pointer is nearest -- the
        // same node `focus_node` lets you grab them on, so what is drawn and
        // what can be picked up cannot disagree. Requiring a node to be selected
        // first made them undiscoverable: nothing on screen said they existed.
        let focused = selection.focus == Some(index);
        let hovered = matches!(
            selection.hovered,
            Some(Hovered::Node(i) | Hovered::HandleIn(i) | Hovered::HandleOut(i)) if i == index
        );
        let show_handles = selected || focused;
        let colour = if selected {
            Color::srgb(0.4, 0.9, 1.0)
        } else if node.half_width.is_some() {
            // A node that sets its own width is drawn differently, so "where does
            // this road change width" is answerable at a glance.
            Color::srgb(1.0, 0.8, 0.3)
        } else {
            Color::WHITE
        };
        let radius = NODE_PX * scale * if hovered { 1.5 } else { 1.0 };
        if node.half_width.is_some() || selected {
            gizmos.circle_2d(Isometry2d::from_translation(position), radius, colour);
            gizmos.circle_2d(
                Isometry2d::from_translation(position),
                radius * 0.55,
                colour,
            );
        } else {
            gizmos.circle_2d(Isometry2d::from_translation(position), radius, colour);
        }

        if show_handles {
            // Road-edge grips. Square, so they read as a different kind of thing
            // from the round bezier handles -- one changes the shape of the
            // corner, the other how wide the road is through it.
            for (edge, side) in width_handles(editor.as_ref(), index) {
                let hovered_here = selection.hovered == Some(Hovered::Width(index, side));
                let colour = if hovered_here {
                    Color::srgb(1.0, 0.95, 0.4)
                } else {
                    Color::srgb(1.0, 0.75, 0.25)
                };
                let size = HANDLE_PX * scale * if hovered_here { 2.6 } else { 2.0 };
                gizmos.line_2d(position, edge, Color::srgba(1.0, 0.75, 0.25, 0.5));
                gizmos.rect_2d(
                    Isometry2d::from_translation(edge),
                    Vec2::splat(size),
                    colour,
                );
            }
            for (offset, mirrored) in [
                (to_world(node.in_handle), node.mirrored),
                (to_world(node.out_handle), node.mirrored),
            ] {
                let tip = position + offset;
                let handle_colour = if mirrored {
                    Color::srgb(0.35, 0.95, 1.0)
                } else {
                    // A broken pair is a different thing and says so.
                    Color::srgb(1.0, 0.45, 1.0)
                };
                gizmos.line_2d(position, tip, handle_colour);
                gizmos.circle_2d(
                    Isometry2d::from_translation(tip),
                    HANDLE_PX * scale,
                    handle_colour,
                );
                gizmos.circle_2d(
                    Isometry2d::from_translation(tip),
                    HANDLE_PX * scale * 0.5,
                    handle_colour,
                );
            }
        }
    }

    // What the pointer would do, so a mode is visible without reading the panel.
    if let (Some(at), false) = (cursor.world, cursor.over_ui) {
        let hint = match *tool {
            Tool::Edit => Color::srgba(1., 1., 1., 0.35),
            Tool::Items => Color::srgba(1., 0.7, 0.2, 0.7),
        };
        gizmos.circle_2d(Isometry2d::from_translation(at), 2.0 * scale, hint);
    }

    // A quiet reminder of how much of this a player will actually see at once.
    gizmos.rect_2d(
        Isometry2d::from_translation(built.start_pose.position),
        RESOLUTION,
        Color::srgba(0.2, 0.2, 0.3, 0.25),
    );
}
