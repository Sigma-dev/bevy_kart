//! The camera, and the rules about where it is allowed to be.
//!
//! Until maps could be larger than one screen there were no rules: a single
//! orthographic camera sat at the origin from `Startup` and never moved, and
//! every screen outside the race was authored around that. This module owns the
//! camera so that assumption becomes something stated once rather than relied on
//! everywhere.

use bevy::prelude::*;

use crate::{RESOLUTION, Screen, kart::LocalKart};

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_camera)
            // The race is the only screen that moves the camera, and every other
            // screen is drawn around the origin, so both ways back out of a race
            // put it there again. Two transitions rather than one because
            // `AppState` and `LobbyState` are independent: a race can end into
            // the lobby, and a session can end from inside a race.
            .add_systems(
                OnEnter(Screen::StartMenu),
                reset_camera,
            )
            .add_systems(OnEnter(Screen::Lobby), reset_camera)
            .add_systems(
                PostUpdate,
                follow_local_kart
                    // The local kart's `Transform` is written by the rollback
                    // smoothing inside this set. Reading it any earlier gets a
                    // frame-old position, which does not make the camera lag --
                    // it makes the whole world jitter against a kart that is
                    // supposed to be nailed to the middle of the screen.
                    .after(crate::ApplyCorrectionSet)
                    .before(TransformSystems::Propagate)
                    .run_if(in_state(Screen::Race)),
            );
    }
}

/// The camera the player looks through, as opposed to the minimap's.
#[derive(Component)]
pub struct MainCamera;

/// The world rectangle the camera may show, for as long as a race is running.
#[derive(Resource, Clone, Copy, Debug)]
pub struct CameraBounds(pub Rect);

/// How much of the distance to the target is closed each second.
///
/// The kart is already smoothed by `rollback_smoothing`, so this only has to
/// take the edge off a correction the smoothing has already absorbed most of.
const FOLLOW_RESPONSE: f32 = 12.0;

fn setup_camera(mut commands: Commands) {
    let mut projection = OrthographicProjection::default_2d();
    projection.scaling_mode = bevy::camera::ScalingMode::Fixed {
        width: RESOLUTION.x,
        height: RESOLUTION.y,
    };
    commands.spawn((
        Camera2d,
        MainCamera,
        // Bevy's fallback for "which camera does the UI draw to" is the *highest*
        // `Camera::order` (see `DefaultUiCamera::get`). The minimap is a second
        // camera at a higher order, so without this marker the entire HUD, menu
        // and lobby would silently render inside the minimap's little viewport.
        IsDefaultUiCamera,
        // Without a listener anywhere in the world, bevy_audio puts the ears at
        // the origin. That was invisible while the camera never moved; the moment
        // it follows a kart, an explosion beside you is panned as though it were
        // wherever the kart happens to be relative to (0, 0).
        SpatialListener::default(),
        Projection::Orthographic(projection),
    ));
}

/// Put the camera back where every screen outside the race expects it.
fn reset_camera(mut camera: Query<&mut Transform, With<MainCamera>>) {
    for mut transform in camera.iter_mut() {
        transform.translation.x = 0.0;
        transform.translation.y = 0.0;
    }
}

/// Where the camera wants to be: on the kart, but never showing outside the map.
///
/// Split out and taken by value because it is the part worth testing, and
/// because the reversed-range case below is a panic waiting to happen inside a
/// system where it would be much harder to see.
pub fn camera_target(kart: Vec2, bounds: Rect, viewport: Vec2) -> Vec2 {
    let half = viewport / 2.0;
    let lo = bounds.min + half;
    let hi = bounds.max - half;
    let centre = bounds.center();
    // On an axis where the map is smaller than the screen there is no clamping to
    // do -- and `lo > hi` there, which `f32::clamp` panics on rather than
    // silently picking a side. Centre that axis instead, which is also what makes
    // a map that fits on one screen behave exactly as the game did before the
    // camera could move at all.
    Vec2::new(
        if lo.x <= hi.x { kart.x.clamp(lo.x, hi.x) } else { centre.x },
        if lo.y <= hi.y { kart.y.clamp(lo.y, hi.y) } else { centre.y },
    )
}

fn follow_local_kart(
    time: Res<Time>,
    bounds: Option<Res<CameraBounds>>,
    // Read the kart's `Transform`, never its `Position`: `Position` is the tick
    // pose and steps at 64 Hz, so following it would stutter on every frame that
    // is not a tick and would throw away everything `rollback_smoothing` does.
    kart: Query<&Transform, (With<LocalKart>, Without<MainCamera>)>,
    mut camera: Query<&mut Transform, (With<MainCamera>, Without<LocalKart>)>,
) {
    let Some(bounds) = bounds else { return };
    let Ok(mut camera) = camera.single_mut() else {
        return;
    };
    // No local kart means a spectator -- a peer that joined mid-race, which the
    // host does not spawn a kart for. Show it the middle of the map rather than
    // leaving the camera wherever it last was.
    let focus = kart
        .single()
        .map(|t| t.translation.xy())
        .unwrap_or_else(|_| bounds.0.center());
    let target = camera_target(focus, bounds.0, RESOLUTION);
    // Frame-rate independent exponential approach, so the follow feels the same
    // at 60 and 144 Hz.
    let alpha = 1.0 - (-FOLLOW_RESPONSE * time.delta_secs()).exp();
    let current = camera.translation.xy();
    let next = current + (target - current) * alpha;
    camera.translation.x = next.x;
    camera.translation.y = next.y;
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: Vec2 = Vec2::new(256.0, 144.0);

    fn rect(min: Vec2, max: Vec2) -> Rect {
        Rect { min, max }
    }

    #[test]
    fn a_kart_well_inside_a_big_map_is_centred_on() {
        let bounds = rect(Vec2::new(-500.0, -400.0), Vec2::new(500.0, 400.0));
        let kart = Vec2::new(30.0, -20.0);
        assert_eq!(camera_target(kart, bounds, VIEW), kart);
    }

    #[test]
    fn the_camera_stops_at_the_corner_instead_of_showing_past_it() {
        let bounds = rect(Vec2::new(-500.0, -400.0), Vec2::new(500.0, 400.0));
        // Far past the bottom-left corner.
        let target = camera_target(Vec2::new(-900.0, -900.0), bounds, VIEW);
        assert_eq!(target, Vec2::new(-500.0 + 128.0, -400.0 + 72.0));
        // The viewport it implies is inside the map on both axes.
        assert!(target.x - 128.0 >= bounds.min.x);
        assert!(target.y - 72.0 >= bounds.min.y);
    }

    /// The converted classic track is 244 x 125, smaller than the 256 x 144
    /// viewport on both axes. Without the reversed-range guard this is a panic,
    /// and it is the very first map the game will load.
    #[test]
    fn a_map_smaller_than_the_screen_is_centred_and_does_not_panic() {
        let bounds = rect(Vec2::new(-124.0, -62.0), Vec2::new(120.0, 63.0));
        let centre = bounds.center();
        for kart in [
            Vec2::ZERO,
            Vec2::new(-124.0, -62.0),
            Vec2::new(120.0, 63.0),
            Vec2::new(9999.0, -9999.0),
        ] {
            assert_eq!(camera_target(kart, bounds, VIEW), centre);
        }
    }

    /// A map wider than the screen but shorter than it: one axis clamps, the
    /// other centres. Mixing the two is the case a single `if` would get wrong.
    #[test]
    fn each_axis_decides_for_itself() {
        let bounds = rect(Vec2::new(-500.0, -50.0), Vec2::new(500.0, 50.0));
        let target = camera_target(Vec2::new(-900.0, 40.0), bounds, VIEW);
        assert_eq!(target.x, -500.0 + 128.0, "x is wide enough to clamp");
        assert_eq!(target.y, 0.0, "y is shorter than the screen, so it centres");
    }
}
