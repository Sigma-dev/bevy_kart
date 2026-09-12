//! What a tracked entity looks like, on every peer.
//!
//! The simulation spawns bare bodies: an id, a kind, an owner, a pose. This
//! module dresses them when they appear -- on the host as the tick spawns them,
//! on a client as a snapshot introduces them -- and says what it looks and
//! sounds like when one goes.

use audio_manager::prelude::*;
use avian2d::prelude::*;
use bevy::ecs::entity_disabling::Disabled;
use bevy::ecs::query::Allow;
use bevy::prelude::*;
use bevy_ensemble::prelude::*;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;
use bevy_timer::{Timer as GameTimer, TimerFinished};

use crate::{
    AppPlayerData, AppState, AssetHandles, EntityKind, SpriteLayers, car_controller_2d, items,
    kart::{self, FollowTransform, LapsCounter, LocalKart},
    track,
};

pub struct EntitySpawnPlugin;

impl Plugin for EntitySpawnPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_tracked_entity_spawned)
            .add_observer(on_replication_mode_changed)
            .add_observer(on_tracked_entity_gone);
    }
}

/// What kind of body a kart is on this peer.
///
/// The host simulates every kart. A client simulates its own, which the stack
/// marks `Predicted` from its `Owner`, and is handed every other kart's state
/// by the host a couple of ticks behind: a dynamic body there would fight that
/// restore every tick -- damping, colliding, integrating from a velocity the
/// host has since changed -- so it is kinematic, moved by the record and still
/// solid to drive into.
fn body_kind(
    local_client: Option<&LocalClientPlayer>,
    owner: Option<&Owner>,
    mode: Option<&ReplicationMode>,
) -> RigidBody {
    let Some(local) = local_client else {
        return RigidBody::Dynamic;
    };
    let mine = owner.is_some_and(|owner| owner.0 == local.0);
    if mine || matches!(mode, Some(ReplicationMode::Predicted)) {
        RigidBody::Dynamic
    } else {
        RigidBody::Kinematic
    }
}

/// Observer: when a TickTrackedEntity is added (host spawn or client snapshot),
/// add visual and physics components based on EntityKind.
///
/// A snapshot inserts `TickTrackedEntity` last, after every networked
/// component, so this sees the owner and the pose.
fn on_tracked_entity_spawned(
    trigger: On<Add, TickTrackedEntity>,
    mut commands: Commands,
    query: Query<(
        &EntityKind,
        Option<&Owner>,
        Option<&Position>,
        Option<&Rotation>,
        Option<&ReplicationMode>,
    )>,
    asset_handles: Res<AssetHandles>,
    participants_with_data: Query<(&LobbyParticipant, Option<&PlayerData<AppPlayerData>>)>,
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    local_client: Option<Res<LocalClientPlayer>>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    mut audio_manager: AudioManager,
) {
    let entity = trigger.entity;
    let Ok((kind, maybe_owner, maybe_pos, maybe_rot, maybe_mode)) = query.get(entity) else {
        return;
    };
    let pos = maybe_pos.map(|p| p.0).unwrap_or_default();
    let rot = maybe_rot.map(|r| r.as_radians()).unwrap_or(0.0);

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
            let is_local = local_player.as_ref().is_some_and(|p| p.0 == owner_uuid);
            debug!(
                "kart {entity} appeared: owner {owner_uuid:#x}, local player {:?}, mine: {is_local}",
                local_player.as_ref().map(|p| format!("{:#x}", p.0))
            );

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
                // Drawn between its last two tick states, and a snapshot
                // correction slides in over a few frames rather than blinking,
                // up to the size that would read as clipping through a wall.
                // The local kart is exempt by its `Owner`: a correction to
                // what you are steering should be felt, not hidden.
                TickedInterpolation::default(),
                CorrectionSmoothing {
                    decay_rate: 20.0,
                    max_offset: 5.0,
                    max_angle: 1.0,
                    apply_to: SmoothingTarget::Self_,
                },
                Mass(1.),
                body_kind(local_client.as_deref(), maybe_owner, maybe_mode),
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
            let flame_tex = asset_handles.boost_flame_texture.clone();
            let flame_atlas = asset_handles.boost_flame_atlas.clone();
            commands.entity(entity).with_children(|parent| {
                kart::spawn_kart_wheels(parent, wheel_tex);
                kart::spawn_boost_flame(parent, flame_tex, flame_atlas);
            });

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
            commands.entity(entity).insert((
                DespawnOnExit(AppState::Game),
                Transform::from_xyz(pos.x, pos.y, SpriteLayers::Car.to_z()),
                Sprite::from_image(asset_handles.crate_texture.clone()),
                TickedInterpolation::default(),
            ));
        }
        EntityKind::Rocket => {
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
                // The marker `move_rocket` and `animate_rocket` key on. The host
                // spawns it with the rocket; a client only ever sees `EntityKind`,
                // so without this its rockets neither flew between snapshots nor
                // animated.
                items::Rocket,
                TickedInterpolation::default(),
            ));
            audio_manager.play_sound(
                PlayAudio2D::new_once("sounds/rocket.wav")
                    .with_spatial(SpatialSettings2D::Entity(entity)),
            );
        }
        EntityKind::Mine => {
            commands.entity(entity).insert((
                DespawnOnExit(AppState::Game),
                // Above the track image, which sits at z = 0, and below the
                // karts, so one drives over the mine rather than under it.
                Transform::from_xyz(pos.x, pos.y, SpriteLayers::OnTrack.to_z()),
                Sprite::from_image(asset_handles.mine_texture.clone()),
                // The marker `trigger_mines` keys on. The host spawns it with
                // the mine; a client only ever sees `EntityKind`.
                items::Mine::default(),
                TickedInterpolation::default(),
            ));
        }
    }
}

/// A kart whose mode changes after it appeared -- the role arriving after the
/// body, or the game marking one predicted -- changes body kind with it.
fn on_replication_mode_changed(
    trigger: On<Insert, ReplicationMode>,
    mut commands: Commands,
    karts: Query<(&EntityKind, Option<&Owner>, &ReplicationMode), With<RigidBody>>,
    local_client: Option<Res<LocalClientPlayer>>,
) {
    let Ok((kind, owner, mode)) = karts.get(trigger.entity) else {
        return;
    };
    if matches!(kind, EntityKind::Kart) {
        commands.entity(trigger.entity).insert(body_kind(
            local_client.as_deref(),
            owner,
            Some(mode),
        ));
    }
}

/// What a tracked entity leaving the world looks and sounds like, on every peer.
///
/// `despawn_ticked` tombstones rather than destroys -- on the host when it
/// decides, on a client when the snapshot says so -- and the tombstone is the
/// one moment both see. A rocket or a mine that ceased to exist exploded; an
/// item crate that did was picked up. The explosion is an entity of its own,
/// untracked, so nothing about it is replicated or rolled back; the game used
/// to spawn a *tracked* explosion for every peer to render and despawn it on a
/// frame timer, which is a despawn no rollback could undo.
///
/// `Allow<Disabled>`: the tombstone disables the entity in the same breath, and
/// a query that skipped disabled entities would find nothing to ask.
fn on_tracked_entity_gone(
    trigger: On<Add, Tombstone>,
    mut commands: Commands,
    gone: Query<(&EntityKind, &Position), Allow<Disabled>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut audio_manager: AudioManager,
) {
    let Ok((kind, pos)) = gone.get(trigger.entity) else {
        return;
    };
    match kind {
        EntityKind::Rocket | EntityKind::Mine => {
            audio_manager
                .play_sound(PlayAudio2D::new_once("sounds/explosion.wav").with_volume(0.3));
            commands
                .spawn((
                    DespawnOnExit(AppState::Game),
                    Transform::from_xyz(pos.x, pos.y, SpriteLayers::AboveCar.to_z()),
                    Mesh2d(meshes.add(Circle::new(items::EXPLOSION_RADIUS))),
                    MeshMaterial2d(materials.add(Color::WHITE)),
                    GameTimer::new_running().with_target_duration(0.1),
                ))
                .observe(|timer: On<TimerFinished>, mut commands: Commands| {
                    commands.entity(timer.event_target()).try_despawn();
                });
        }
        EntityKind::ItemPickup(_) => {
            audio_manager.play_sound(PlayAudio2D::new_once("sounds/pickup.wav"));
        }
        EntityKind::Kart => {}
    }
}
