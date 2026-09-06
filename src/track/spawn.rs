//! Turning a [`BuiltTrack`] into entities.
//!
//! Everything here runs on **every** peer, and the barriers it spawns are
//! ordinary static colliders rather than networked entities -- so two peers that
//! disagree about where a wall is do not resynchronise, they fight. That is why
//! the geometry arrives pre-computed from `map::build`, whose determinism rules
//! are the ones that matter; this file only has to spawn what it is given, in
//! the order it is given it.

use avian2d::prelude::*;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::decor::Decor;
use crate::items::spawn_spawner;
use crate::track::StartLight;
use crate::track::map::build::BuiltTrack;
use crate::track::map::data::scalar_to_world;
use crate::track::map::mesh::{road_mesh, start_line_mesh};
use crate::track::map::scatter::scatter;
use crate::track::position::progress_line::ProgressLine;
use crate::{AppState, AssetHandles, SpriteLayers};

/// How thick a barrier is when the map draws no wall band for it to match.
///
/// A barrier is invisible -- the road mesh paints the band, and this is the
/// collider under it -- so a map with `kerb_width` of zero would otherwise get a
/// wall of nothing. Two units is what the hand-drawn track's walls were traced
/// at.
const MIN_WALL_THICKNESS: f32 = 2.0;

/// How far beyond the road edge the start light stands.
const START_LIGHT_CLEARANCE: f32 = 5.0;

pub(crate) fn spawn_map(
    mut commands: Commands,
    built: Res<BuiltTrack>,
    asset_handles: Res<AssetHandles>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    for warning in &built.warnings {
        warn!("{}: {:?}", built.map.name, warning);
    }

    // `ColorMaterial`'s shader multiplies the material colour by the vertex
    // colour, so this has to be white: the road, the kerbs and the start line
    // all carry their own colours per vertex.
    let vertex_coloured = materials.add(ColorMaterial::from(Color::WHITE));

    commands.spawn((
        DespawnOnExit(AppState::Game),
        Mesh2d(meshes.add(road_mesh(&built))),
        MeshMaterial2d(vertex_coloured.clone()),
        Transform::from_xyz(0., 0., SpriteLayers::Background.to_z()),
        // The one entity the minimap shares with the main view, so the map in
        // the corner is the road being raced rather than a copy of it.
        RenderLayers::from_layers(&[0, crate::track::minimap::MINIMAP_LAYER]),
    ));
    commands.spawn((
        DespawnOnExit(AppState::Game),
        Mesh2d(meshes.add(start_line_mesh(&built))),
        MeshMaterial2d(vertex_coloured),
        Transform::from_xyz(0., 0., SpriteLayers::OnGround.to_z()),
    ));

    // The wall the player sees is the band in the road mesh above. These are the
    // colliders under it, and they are the same thickness so that where a kart
    // stops is where the paint is.
    let thickness = scalar_to_world(built.map.road.kerb_width).max(MIN_WALL_THICKNESS);
    for wall in [&built.left_wall, &built.right_wall] {
        spawn_barriers(&mut commands, wall, thickness);
    }

    commands.spawn((
        DespawnOnExit(AppState::Game),
        ProgressLine::new(built.progress.clone()),
    ));

    for point in &built.item_boxes {
        spawn_spawner(&mut commands, *point);
    }

    spawn_start_light(
        &mut commands,
        &built,
        &asset_handles,
        &mut texture_atlas_layouts,
    );

    // Cosmetic only, and deliberately spawned in its own pass with its own
    // marker: decoration must never grow a collider or a networked identity, and
    // a rule with a marker behind it is one a debug assertion can check.
    for placement in scatter(&built) {
        commands.spawn((
            DespawnOnExit(AppState::Game),
            Decor,
            placement.element,
            placement.element.as_sprite(&asset_handles),
            Transform::from_translation(
                placement.position.extend(SpriteLayers::OnGround.to_z()),
            ),
        ));
    }
}

/// One static box body per wall segment. Nothing is drawn: the road mesh paints
/// the band these sit under.
///
/// Takes the points as a closed loop -- consecutive pairs, wrapping -- which is
/// exactly what the offset polylines out of the builder are.
///
/// The pose is built with [`Rotation::from_sin_cos`] from the normalised segment
/// direction rather than `Rotation::radians(dy.atan2(dx))`. `Rotation` is a unit
/// complex number with public `sin` and `cos`, so the angle was only ever an
/// intermediate -- and `atan2` is one of the functions that does not agree to
/// the last bit between glibc, musl and wasm, which for a wall every peer builds
/// for itself is a divergence rollback would re-create every tick.
fn spawn_barriers(commands: &mut Commands, points: &[Vec2], thickness: f32) {
    if points.len() < 2 {
        return;
    }
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
        let delta = *a - *b;
        let length = delta.length();
        if length < f32::EPSILON {
            continue;
        }
        let direction = delta / length;
        let middle = (*a + *b) / 2.;
        commands.spawn((
            DespawnOnExit(AppState::Game),
            RigidBody::Static,
            Collider::rectangle(length, thickness),
            Position(middle),
            Rotation::from_sin_cos(direction.y, direction.x),
        ));
    }
}

/// The countdown light, standing just off the road beside the start line.
fn spawn_start_light(
    commands: &mut Commands,
    built: &BuiltTrack,
    asset_handles: &AssetHandles,
    texture_atlas_layouts: &mut Assets<TextureAtlasLayout>,
) {
    let layout = TextureAtlasLayout::from_grid(UVec2::new(15, 7), 5, 1, None, None);
    let texture_atlas_layout = texture_atlas_layouts.add(layout);
    let start = built.centre[0];
    let offset = start.half_width + START_LIGHT_CLEARANCE;
    let position = start.position + start.normal * offset;
    commands.spawn((
        DespawnOnExit(AppState::Game),
        Transform::from_translation(position.extend(SpriteLayers::Car.to_z())),
        Sprite::from_atlas_image(
            asset_handles.traffic_light_texture.clone(),
            TextureAtlas {
                layout: texture_atlas_layout,
                index: 1,
            },
        ),
        StartLight,
    ));
}

/// Where a kart on the starting grid sits and which way it points.
///
/// Returned as avian's own [`Rotation`] rather than an angle: the kart sprite
/// points along +y, so a kart facing direction `d` needs `sin = -d.x, cos = d.y`
/// -- which is exact, where forming the angle and taking it apart again is not.
pub(crate) fn grid_slot(built: &BuiltTrack, index: usize) -> (Vec2, Rotation) {
    let pose = built.grid[index % built.grid.len()];
    (pose.position, Rotation::from_sin_cos(-pose.cos, pose.sin))
}
