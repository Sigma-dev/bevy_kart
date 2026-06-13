use audio_manager::prelude::*;
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_ensemble::prelude::*;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;
use bevy_timer::{Timer as GameTimer, TimerFinished};

use crate::{
    AssetHandles, AppPlayerData, AppState, CorrectionSmoothing, EntityKind, NetworkedPosition,
    NetworkedRotation, OwnerPlayer, PlayerInput, SpriteLayers,
    car_controller_2d,
    items,
    kart::{self, FollowTransform, LapsCounter, LocalKart},
    track,
};

pub struct EntitySpawnPlugin;

impl Plugin for EntitySpawnPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_tracked_entity_spawned)
            .add_systems(
                Update,
                (capture_local_input, sync_visuals),
            );
    }
}

/// Capture keyboard input and write to InputQueue each tick.
fn capture_local_input(
    keys: Res<ButtonInput<KeyCode>>,
    tick: Res<CurrentTick>,
    local_client: Option<Res<LocalClientPlayer>>,
    local_server: Option<Res<LocalServerPlayer>>,
    mut input_queue: ResMut<InputQueue<PlayerInput>>,
) {
    let uuid = local_client
        .as_ref()
        .map(|p| p.0)
        .or_else(|| local_server.as_ref().map(|p| p.0));
    let Some(uuid) = uuid else { return };
    let input = PlayerInput {
        forward: keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp),
        backward: keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown),
        left: keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft),
        right: keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight),
        using_item: keys.pressed(KeyCode::Space),
    };
    input_queue.insert(tick.0 + 1, uuid, input);
}

/// Sync networked Position/Rotation to Transform every frame for non-physics entities.
fn sync_visuals(
    mut non_physics: Query<
        (
            &NetworkedPosition,
            Option<&NetworkedRotation>,
            &mut Transform,
        ),
        (With<TickTrackedEntity>, Without<RigidBody>),
    >,
) {
    for (pos, rot, mut transform) in non_physics.iter_mut() {
        transform.translation.x = pos.0.x;
        transform.translation.y = pos.0.y;
        if let Some(rot) = rot {
            transform.rotation = Quat::from_rotation_z(rot.0);
        }
    }
}

/// Observer: when a TickTrackedEntity is added (host spawn or client snapshot),
/// add visual and physics components based on EntityKind.
fn on_tracked_entity_spawned(
    trigger: On<Add, TickTrackedEntity>,
    mut commands: Commands,
    query: Query<(
        &EntityKind,
        Option<&OwnerPlayer>,
        Option<&Position>,
        Option<&Rotation>,
        Option<&NetworkedPosition>,
        Option<&NetworkedRotation>,
    )>,
    asset_handles: Res<AssetHandles>,
    participants_with_data: Query<(&LobbyParticipant, Option<&PlayerData<AppPlayerData>>)>,
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    local_server: Option<Res<LocalServerPlayer>>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut audio_manager: AudioManager,
) {
    let entity = trigger.entity;
    let Ok((kind, maybe_owner, maybe_pos, maybe_rot, maybe_net_pos, maybe_net_rot)) =
        query.get(entity)
    else {
        return;
    };

    match kind {
        EntityKind::Kart => {
            if let Some(pos) = maybe_pos {
                let z = SpriteLayers::Car.to_z();
                let mut t = Transform::from_xyz(pos.x, pos.y, z);
                if let Some(rot) = maybe_rot {
                    t.rotation = Quat::from_rotation_z(rot.as_radians());
                }
                commands.entity(entity).insert(t);
            }
            let owner_uuid = maybe_owner.map(|o| o.0).unwrap_or(0);
            let local_uuid = local_player
                .as_ref()
                .map(|p| p.0)
                .or_else(|| local_server.as_ref().map(|p| p.0));
            let is_local = local_uuid.is_some_and(|uuid| uuid == owner_uuid);

            let player = participants_with_data
                .iter()
                .find(|(p, _)| p.player_uuid == owner_uuid)
                .and_then(|(_, data)| data.map(|d| &d.0));
            let kart_color_index = player.map(|p| p.kart_color.to_u32() as usize).unwrap_or(0);
            let player_name = player
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            commands.entity(entity).insert((
                DespawnOnExit(AppState::Game),
                car_controller_2d::CarController2d::new(1.),
                car_controller_2d::SteeringState::default(),
                car_controller_2d::CarControllerDisabled,
                CorrectionSmoothing::default(),
                NoTransformEasing,
                Mass(1.),
                RigidBody::Dynamic,
                Collider::rectangle(4., 8.),
                Visibility::Inherited,
                LapsCounter::new(),
                track::position::TrackPosition,
                Sprite::from_atlas_image(
                    asset_handles.karts_texture.clone(),
                    TextureAtlas {
                        layout: asset_handles.karts_atlas.clone(),
                        index: kart_color_index,
                    },
                ),
            ));
            commands.entity(entity).observe(kart::on_lap_update);

            let wheel_tex = asset_handles.wheel_texture.clone();
            commands
                .entity(entity)
                .with_children(|parent| kart::spawn_kart_wheels(parent, wheel_tex));

            if is_local {
                commands.entity(entity).insert(LocalKart);
            }

            commands.spawn((
                DespawnOnExit(AppState::Game),
                FollowTransform(entity),
                children![(
                    Text2d::new(player_name),
                    Transform::from_xyz(0., 5., SpriteLayers::AboveCar.to_z())
                        .with_scale(Vec3::splat(0.1)),
                )],
            ));
        }
        EntityKind::ItemPickup(_) => {
            let pos = maybe_net_pos.map(|p| p.0).unwrap_or_default();
            commands.entity(entity).insert((
                DespawnOnExit(AppState::Game),
                Transform::from_xyz(pos.x, pos.y, SpriteLayers::Car.to_z()),
                Sprite::from_image(asset_handles.crate_texture.clone()),
            ));
            commands.entity(entity).observe(
                |_trigger: On<Despawn>, mut audio_manager: AudioManager| {
                    audio_manager.play_sound(PlayAudio2D::new_once("sounds/pickup.wav"));
                },
            );
        }
        EntityKind::Rocket => {
            let pos = maybe_net_pos.map(|p| p.0).unwrap_or_default();
            let rot = maybe_net_rot.map(|r| r.0).unwrap_or(0.0);
            let layout = TextureAtlasLayout::from_grid(UVec2::new(3, 8), 2, 1, None, None);
            let atlas_layout = texture_atlas_layouts.add(layout);
            commands.entity(entity).insert((
                DespawnOnExit(AppState::Game),
                Transform::from_xyz(pos.x, pos.y, SpriteLayers::Car.to_z())
                    .with_rotation(Quat::from_rotation_z(rot)),
                Sprite::from_atlas_image(
                    asset_handles.rocket_texture.clone(),
                    TextureAtlas {
                        layout: atlas_layout,
                        index: 0,
                    },
                ),
            ));
            audio_manager.play_sound(
                PlayAudio2D::new_once("sounds/rocket.wav")
                    .with_spatial(SpatialSettings2D::Entity(entity)),
            );
        }
        EntityKind::Explosion => {
            let pos = maybe_net_pos.map(|p| p.0).unwrap_or_default();
            audio_manager
                .play_sound(PlayAudio2D::new_once("sounds/explosion.wav").with_volume(0.3));
            commands
                .entity(entity)
                .insert((
                    Transform::from_xyz(pos.x, pos.y, SpriteLayers::AboveCar.to_z()),
                    Mesh2d(meshes.add(Circle::new(items::ROCKET_EXPLOSION_RADIUS))),
                    MeshMaterial2d(materials.add(Color::WHITE)),
                    GameTimer::new_running().with_target_duration(0.1),
                ))
                .observe(|timer: On<TimerFinished>, mut commands: Commands| {
                    commands.entity(timer.event_target()).try_despawn();
                });
        }
    }
}
