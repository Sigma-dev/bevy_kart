use crate::car_controller_2d::{CarController2d, CarControllerDisabled, CarControllerInputs, SteeringState};
use crate::menu::lobby::{LobbyCar, LobbyCarName};
use crate::track::LAPS_TO_WIN;
use crate::track::position::TrackPosition;
use crate::{
    AppPlayerData, AppState, ApplyCorrectionSet, AssetHandles, FinishTimes, LobbyState,
    LocalPlayerData, OwnerPlayer, SpriteLayers,
    car_controller_2d::CarController2dWheel,
};
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy_bundled_observers::observers;
use bevy_ensemble::prelude::*;
use bevy_ensemble::LobbyClientPlayerUuid;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};
pub struct KartPlugin;

impl Plugin for KartPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            follow_transform
                .after(ApplyCorrectionSet)
                .before(TransformSystems::Propagate),
        );
    }
}

pub const KART_SIZE: UVec2 = UVec2::new(4, 8);
pub const KART_COLORS_COUNT: u32 = 10;

#[derive(Component, Debug)]
pub struct LapsCounter {
    pub count: i32,
    pub last_frame_progress: f32,
}

impl LapsCounter {
    pub fn new() -> Self {
        Self {
            count: 0,
            last_frame_progress: 0.,
        }
    }

    pub fn update(&mut self, progress: f32) {
        if self.last_frame_progress > 0.95 && progress < 0.05 {
            self.count += 1;
        }
        if self.last_frame_progress < 0.05 && progress > 0.95 {
            self.count -= 1;
        }
        self.last_frame_progress = progress;
    }
}

#[derive(EntityEvent)]
pub struct LapUpdate {
    pub count: i32,
    pub entity: Entity,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct KartColor(pub u32);

impl KartColor {
    pub fn new() -> Self {
        Self(0)
    }

    pub fn right(&self) -> KartColor {
        Self((self.0 + 1) % KART_COLORS_COUNT)
    }

    pub fn left(&self) -> KartColor {
        if self.0 == 0 {
            return Self(KART_COLORS_COUNT - 1);
        }
        Self(self.0 - 1)
    }

    pub fn to_u32(&self) -> u32 {
        self.0
    }

    pub fn new_random() -> Self {
        Self(rand::rng().random_range(0..KART_COLORS_COUNT))
    }
}

/// Spawn the 4 wheel child entities for a kart.
pub fn spawn_kart_wheels(parent: &mut ChildSpawnerCommands, wheel_texture: Handle<Image>) {
    let half_car_width = 2.5;
    let half_car_length = 3.0;
    let positions = [
        (half_car_width, half_car_length - 1., true, true),
        (-half_car_width, half_car_length - 1., true, true),
        (half_car_width, -half_car_length, false, false),
        (-half_car_width, -half_car_length, false, false),
    ];
    for (x, y, powered, steerable) in positions {
        parent.spawn((
            Transform::from_xyz(x, y, SpriteLayers::Wheels.to_z()),
            CarController2dWheel::new(powered, steerable),
            Sprite::from_image(wheel_texture.clone()),
        ));
    }
}

/// Observer for LapUpdate: disable car and record finish time when lap target reached.
pub fn on_lap_update(
    trigger: On<LapUpdate>,
    tick: Res<CurrentTick>,
    karts: Query<&CarControllerDisabled>,
    mut commands: Commands,
    owners: Query<&OwnerPlayer>,
    mut finish_times: ResMut<FinishTimes>,
) {
    if trigger.event().count == LAPS_TO_WIN as i32 {
        if karts.get(trigger.event_target()).is_ok() {
            return;
        }
        if let Ok(owner) = owners.get(trigger.event_target()) {
            commands
                .entity(trigger.event_target())
                .insert(CarControllerDisabled);
            finish_times.times.insert(owner.0, tick.0);
        }
    }
}

#[derive(Component)]
pub struct AutoCar;

pub enum KartControlType {
    Player(OwnerPlayer),
    AutoCar,
    LobbyCar(u128, Option<usize>),
}

#[derive(Component)]
pub struct LocalKart;

pub(crate) fn spawn_kart(
    In((control_type, transform)): In<(KartControlType, Transform)>,
    mut commands: Commands,
    participants_with_data: Query<(&LobbyParticipant, Option<&PlayerData<AppPlayerData>>)>,
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    server_player: Option<Res<LocalServerPlayer>>,
    asset_handles: Res<AssetHandles>,
) {
    let wheel_tex = asset_handles.wheel_texture.clone();
    let id = commands
        .spawn((
            DespawnOnExit(AppState::Game),
            Mass(1.),
            RigidBody::Dynamic,
            Collider::rectangle(4., 8.),
            transform,
            CarController2d::new(1.),
            SteeringState::default(),
            Visibility::Inherited,
        ))
        .with_children(|parent| spawn_kart_wheels(parent, wheel_tex))
        .id();
    match control_type {
        KartControlType::Player(owner) => {
            let Some(player) = participants_with_data
                .iter()
                .find(|(p, _)| p.player_uuid == owner.0)
                .and_then(|(_, data)| data.map(|d| &d.0))
            else {
                commands.entity(id).despawn();
                return;
            };
            let local_uuid = local_player
                .as_ref()
                .map(|p| p.0)
                .or_else(|| server_player.as_ref().map(|p| p.0));
            let is_local = local_uuid.is_some_and(|uuid| uuid == owner.0);
            commands.entity(id).insert((
                owner,
                CarControllerDisabled,
                LapsCounter::new(),
                Sprite::from_atlas_image(
                    asset_handles.karts_texture.clone(),
                    TextureAtlas {
                        layout: asset_handles.karts_atlas.clone(),
                        index: player.kart_color.to_u32() as usize,
                    },
                ),
                TrackPosition,
            ));
            commands.entity(id).observe(on_lap_update);

            if is_local {
                commands.entity(id).insert(LocalKart);
            }

            commands.spawn((
                DespawnOnExit(AppState::Game),
                FollowTransform(id),
                children![(
                    Text2d::new(player.name.clone()),
                    Transform::from_xyz(0., 5., SpriteLayers::AboveCar.to_z())
                        .with_scale(Vec3::splat(0.1)),
                )],
            ));
        }
        KartControlType::AutoCar => {
            commands.entity(id).insert((
                Sprite::from_atlas_image(
                    asset_handles.karts_texture.clone(),
                    TextureAtlas {
                        layout: asset_handles.karts_atlas.clone(),
                        index: rand::rng().random_range(0..KART_COLORS_COUNT) as usize,
                    },
                ),
                AutoCar,
                CarControllerInputs {
                    forward: true,
                    ..default()
                },
                DespawnOnExit(LobbyState::OutOfLobby),
            ));
        }
        KartControlType::LobbyCar(player_uuid, rank) => {
            let local_uuid = local_player
                .as_ref()
                .map(|p| p.0)
                .or_else(|| server_player.as_ref().map(|p| p.0));
            let is_local = local_uuid.is_some_and(|uuid| uuid == player_uuid);
            let is_host = server_player.is_some();
            let player_data = participants_with_data
                .iter()
                .find(|(p, _)| p.player_uuid == player_uuid)
                .and_then(|(_, data)| data.map(|d| &d.0));
            let player_name = player_data
                .map(|d| d.name.as_str())
                .unwrap_or("...");
            let player_color = player_data
                .map(|d| d.kart_color.to_u32() as usize)
                .unwrap_or(0);
            let name = if is_local { "(YOU)\n" } else { "" }.to_string() + player_name;
            let name = rank
                .map(|r| format!("({})\n", r))
                .unwrap_or_default()
                .to_string()
                + &name;

            commands
                .entity(id)
                .insert((
                    Sprite::from_atlas_image(
                        asset_handles.karts_texture.clone(),
                        TextureAtlas {
                            layout: asset_handles.karts_atlas.clone(),
                            index: player_color,
                        },
                    ),
                    LobbyCar(player_uuid),
                    DespawnOnExit(LobbyState::InLobby),
                    DespawnOnExit(AppState::OutOfGame),
                ))
                .remove::<Collider>();

            let mut ui = commands.spawn((
                Visibility::Inherited,
                FollowTransform(id),
                children![(
                    Text2d::new(name),
                    LobbyCarName(player_uuid),
                    TextLayout::new_with_justify(Justify::Center),
                    Transform::from_xyz(0., 5., SpriteLayers::AboveCar.to_z())
                        .with_scale(Vec3::splat(0.1)),
                ),],
            ));
            if is_local {
                ui.with_children(|parent| {
                    parent.spawn((
                        Transform::from_xyz(-6., 0., SpriteLayers::Car.to_z()),
                        Button,
                        Sprite {
                            image: asset_handles.arrow_texture.clone(),
                            flip_x: true,
                            ..default()
                        },
                        Pickable::default(),
                        observers!(|_: On<Pointer<Press>>,
                                    mut local_data: ResMut<LocalPlayerData>,
                                    mut commands: Commands,
                                    lobbies: Query<Entity, With<Lobby>>| {
                            local_data.0.kart_color = local_data.0.kart_color.left();
                            if let Some(lobby) = lobbies.iter().next() {
                                let data = local_data.0.clone();
                                commands.entity(lobby).trigger(move |entity| SetPlayerData::new(entity, data));
                            }
                        }),
                    ));
                    parent.spawn((
                        Transform::from_xyz(6., 0., SpriteLayers::Car.to_z()),
                        Button,
                        Sprite::from_image(asset_handles.arrow_texture.clone()),
                        Pickable::default(),
                        observers![|_: On<Pointer<Press>>,
                                    mut local_data: ResMut<LocalPlayerData>,
                                    mut commands: Commands,
                                    lobbies: Query<Entity, With<Lobby>>| {
                            local_data.0.kart_color = local_data.0.kart_color.right();
                            if let Some(lobby) = lobbies.iter().next() {
                                let data = local_data.0.clone();
                                commands.entity(lobby).trigger(move |entity| SetPlayerData::new(entity, data));
                            }
                        },],
                    ));
                });
            } else if is_host {
                let kick_uuid = player_uuid;
                ui.with_child((
                    Transform::from_xyz(0., -6., SpriteLayers::Car.to_z()),
                    Button,
                    Sprite::from_image(asset_handles.kick_texture.clone()),
                    Pickable::default(),
                    observers![move |_: On<Pointer<Press>>,
                                mut commands: Commands,
                                lobby_clients: Query<(Entity, &LobbyClientPlayerUuid)>| {
                        if let Some((entity, _)) = lobby_clients.iter().find(|(_, uuid)| uuid.0 == kick_uuid) {
                            commands.entity(entity).try_despawn();
                        }
                    }],
                ));
            }
        }
    }
}

#[derive(Component)]
#[require(Transform)]
pub struct FollowTransform(pub Entity);

fn follow_transform(
    mut commands: Commands,
    transforms: Query<&Transform, Without<FollowTransform>>,
    mut follow_transforms: Query<(Entity, &mut Transform, &FollowTransform)>,
) {
    for (entity, mut transform, follow_transform) in follow_transforms.iter_mut() {
        if let Ok(target_transform) = transforms.get(follow_transform.0) {
            transform.translation = target_transform.translation;
        } else {
            commands.entity(entity).try_despawn();
        }
    }
}
