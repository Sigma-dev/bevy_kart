//! A second view of the same world, in the corner.
//!
//! Once a map is larger than the screen you cannot see where you are on it, and
//! neither can you see anybody else. The minimap is a second `Camera2d` pointed
//! at the *real* world on its own render layer, rather than a scaled copy of the
//! track drawn into UI space.
//!
//! That is the whole reason for the choice. A drawn copy needs a world-to-minimap
//! projection applied to every kart every frame; a second camera gets the karts
//! for free, at the right positions, moving smoothly, because they are the same
//! karts. The road is the same entity too -- it simply belongs to both layers --
//! so the minimap cannot drift from the track it is a map of.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, ScalingMode, Viewport};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::kart::{FollowTransform, LocalKart};
use crate::track::map::build::BuiltTrack;
use crate::track::position::TrackPosition;
use crate::{AppColors, AppState, RESOLUTION, SpriteLayers};

/// The layer the minimap camera can see. Layer 0 is the world the player is in.
pub const MINIMAP_LAYER: usize = 1;

/// Fraction of the window's width the minimap occupies.
const WIDTH_FRACTION: f32 = 0.22;

/// Blip radius, as a fraction of the map's width. Keeps a dot the same size on
/// screen whatever the map's scale, which a fixed world radius would not.
const BLIP_RADIUS_FRACTION: f32 = 0.012;

#[derive(Component)]
pub struct MinimapCamera;

#[derive(Component)]
struct MinimapBlip;

/// The kart a blip belongs to already has one.
#[derive(Component)]
pub(crate) struct HasBlip;

pub(crate) fn spawn_minimap(
    mut commands: Commands,
    built: Res<BuiltTrack>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    // Nothing to navigate if the whole map is already on the screen -- which is
    // exactly the case for `Classic`, so it plays as it always did.
    if built.bounds.size().cmple(RESOLUTION).all() {
        return;
    }
    let Ok(window) = windows.single() else { return };

    let mut projection = OrthographicProjection::default_2d();
    // AutoMin, so the whole map fits whatever aspect the corner ends up being.
    projection.scaling_mode = ScalingMode::AutoMin {
        min_width: built.bounds.width(),
        min_height: built.bounds.height(),
    };

    commands.spawn((
        DespawnOnExit(AppState::Game),
        Camera2d,
        MinimapCamera,
        Camera {
            // Drawn after the main view, over the top of it. The main camera
            // carries `IsDefaultUiCamera` precisely because this order would
            // otherwise make *this* camera the one the whole HUD renders into.
            order: 1,
            viewport: Some(viewport_for(window, &built)),
            // The same grass, a shade down: enough that the corner reads as a
            // panel rather than as part of the world behind it, without
            // introducing a colour the game does not otherwise use.
            clear_color: ClearColorConfig::Custom(AppColors::Grass.color().darker(0.06)),
            ..default()
        },
        Projection::Orthographic(projection),
        // Depth zero, like the main camera. A 2D orthographic projection sees
        // `near..far` *relative to the camera*, which defaults to -1000..1000 --
        // so parking this at z = 1000 would put the road, which sits at z = -100,
        // a hundred units behind the near plane. The karts at z = 100 would still
        // show, which looks exactly like a working minimap of an empty world.
        Transform::from_translation(built.bounds.center().extend(0.)),
        RenderLayers::layer(MINIMAP_LAYER),
    ));
}

fn viewport_for(window: &Window, built: &BuiltTrack) -> Viewport {
    let window_size = Vec2::new(window.physical_width() as f32, window.physical_height() as f32);
    let aspect = built.bounds.height() / built.bounds.width().max(1.0);
    let width = (window_size.x * WIDTH_FRACTION).max(1.0);
    let height = (width * aspect).clamp(1.0, window_size.y * 0.5);
    let margin = window_size.x * 0.01;
    Viewport {
        // Top right: the position and item readouts are bottom right, and the
        // FPS text is top left.
        physical_position: UVec2::new(
            (window_size.x - width - margin).max(0.0) as u32,
            margin as u32,
        ),
        physical_size: UVec2::new(width as u32, height as u32),
        ..default()
    }
}

/// Keep the viewport in the corner when the window is resized.
///
/// Written every frame rather than on a resize event: a `Viewport` write that
/// changes nothing costs nothing, and the alternative is being wrong for a frame
/// after every resize.
pub(crate) fn track_minimap_viewport(
    built: Option<Res<BuiltTrack>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Camera, With<MinimapCamera>>,
) {
    let (Some(built), Ok(window)) = (built, windows.single()) else {
        return;
    };
    for mut camera in cameras.iter_mut() {
        camera.viewport = Some(viewport_for(window, &built));
    }
}

/// Give every racing kart a dot on the minimap.
///
/// Reuses `FollowTransform`, which copies translation and nothing else -- which
/// is exactly what a blip wants. Keyed on `Added<TrackPosition>` so nothing in
/// `entity_spawn` has to know the minimap exists.
pub(crate) fn spawn_minimap_blips(
    mut commands: Commands,
    built: Option<Res<BuiltTrack>>,
    minimap: Query<(), With<MinimapCamera>>,
    karts: Query<(Entity, Has<LocalKart>), (With<TrackPosition>, Without<HasBlip>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let Some(built) = built else { return };
    if minimap.is_empty() {
        return;
    }
    let radius = built.bounds.width() * BLIP_RADIUS_FRACTION;
    for (kart, is_local) in karts.iter() {
        let colour = if is_local {
            Color::WHITE
        } else {
            AppColors::Dark.color()
        };
        commands.entity(kart).insert(HasBlip);
        commands.spawn((
            DespawnOnExit(AppState::Game),
            MinimapBlip,
            FollowTransform(kart),
            Mesh2d(meshes.add(Circle::new(radius))),
            MeshMaterial2d(materials.add(ColorMaterial::from(colour))),
            // Only the minimap sees these: at this size they would be enormous
            // blobs over the track in the main view.
            RenderLayers::layer(MINIMAP_LAYER),
            Transform::from_xyz(0., 0., SpriteLayers::AboveCar.to_z()),
        ));
    }
}
