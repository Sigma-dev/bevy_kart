//! Where the pointer is, in the terms the editor needs.

use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::camera::MainCamera;

#[derive(Resource, Default)]
pub struct EditorCursor {
    /// The pointer in world space, if it is over the window at all.
    pub world: Option<Vec2>,
    /// World units per screen pixel, at the current zoom.
    ///
    /// Every hit radius in the editor is written in pixels and multiplied by
    /// this. A radius in world units would be a comfortable target at 1:1 and a
    /// single pixel zoomed out, which is the zoom you are at when you want to
    /// grab something on the far side of the map.
    pub world_per_px: f32,
    /// The pointer is over the panel.
    ///
    /// Everything on the canvas is gated on this, which is also what stops the
    /// scroll wheel zooming the world while it is scrolling a list.
    pub over_ui: bool,
}

pub fn track_cursor(
    mut cursor: ResMut<EditorCursor>,
    hover: Res<HoverMap>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
) {
    cursor.over_ui = hover
        .get(&PointerId::Mouse)
        .is_some_and(|hits| !hits.is_empty());

    let (Ok(window), Ok((camera, transform))) = (windows.single(), cameras.single()) else {
        cursor.world = None;
        return;
    };
    cursor.world = window
        .cursor_position()
        .and_then(|position| camera.viewport_to_world_2d(transform, position).ok());

    // Measured rather than derived from the projection: two adjacent pixels
    // through the same transform is right whatever the scaling mode does.
    let origin = camera.viewport_to_world_2d(transform, Vec2::ZERO);
    let one_across = camera.viewport_to_world_2d(transform, Vec2::X);
    cursor.world_per_px = match (origin, one_across) {
        (Ok(a), Ok(b)) => (b - a).length().max(f32::EPSILON),
        _ => 1.0,
    };
}
