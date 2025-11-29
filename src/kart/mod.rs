use crate::car_controller_2d::{CarController2d, CarControllerDisabled, CarControllerInputs};
use crate::menu::lobby::LobbyCar;
use crate::{AppP2PUpdate, KartEasyP2P, NetworkedEntity, car_controller_2d::CarController2dWheel};
use crate::{AppPlayerData, AppState, AssetHandles, KartP2PData, SpriteLayers};
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_bundled_observers::observers;
use bevy_easy_p2p::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};
pub struct KartPlugin;

impl Plugin for KartPlugin {
    fn build(&self, app: &mut App) {
        app.init_networked_event::<WheelPositionUpdate, KartP2PData>()
            .add_systems(
                Update,
                (
                    sync_wheel_rotation,
                    receive_wheel_rotation,
                    follow_transform,
                    sync_wheel_rotation,
                    receive_wheel_rotation,
                ),
            );
    }
}

pub const KART_SIZE: UVec2 = UVec2::new(4, 8);
pub const KART_COLORS_COUNT: u32 = 10;

#[derive(Component)]
pub struct LapsCounter(pub u32);

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WheelRotation {
    Left,
    Right,
    Straight,
}

#[derive(Message, Clone, Debug, Serialize, Deserialize)]
struct WheelPositionUpdate(NetworkedId, WheelRotation);

#[derive(Component)]
pub struct AutoCar;

pub enum KartControlType {
    Player(NetworkedEntity),
    AutoCar,
    LobbyCar(NetworkedId, Option<usize>),
}

pub(crate) fn spawn_kart(
    In((control_type, transform)): In<(KartControlType, Transform)>,
    mut commands: Commands,
    easy: KartEasyP2P,
    asset_handles: Res<AssetHandles>,
) {
    let half_car_width = 2.5;
    let half_car_length = 3.;
    let id = commands
        .spawn((
            DespawnOnExit(AppState::Game),
            Mass(1.),
            RigidBody::Dynamic,
            Collider::rectangle(4., 8.),
            transform,
            CarController2d::new(1.),
            Visibility::Inherited,
            children![
                (
                    Transform::from_xyz(
                        half_car_width,
                        half_car_length - 1.,
                        SpriteLayers::Wheels.to_z()
                    ),
                    CarController2dWheel::new(true, true),
                    Sprite::from_image(asset_handles.wheel_texture.clone()),
                ),
                (
                    Transform::from_xyz(
                        -half_car_width,
                        half_car_length - 1.,
                        SpriteLayers::Wheels.to_z()
                    ),
                    CarController2dWheel::new(true, true),
                    Sprite::from_image(asset_handles.wheel_texture.clone()),
                ),
                (
                    Transform::from_xyz(
                        half_car_width,
                        -half_car_length,
                        SpriteLayers::Wheels.to_z()
                    ),
                    CarController2dWheel::new(false, false),
                    Sprite::from_image(asset_handles.wheel_texture.clone()),
                ),
                (
                    Transform::from_xyz(
                        -half_car_width,
                        -half_car_length,
                        SpriteLayers::Wheels.to_z()
                    ),
                    CarController2dWheel::new(false, false),
                    Sprite::from_image(asset_handles.wheel_texture.clone()),
                ),
            ],
        ))
        .id();
    match control_type {
        KartControlType::Player(networked_entity) => {
            let player = easy
                .get_player_data(networked_entity.owner_id().clone())
                .unwrap();
            commands.entity(id).insert((
                networked_entity,
                CarControllerDisabled,
                LapsCounter(0),
                Sprite::from_atlas_image(
                    asset_handles.karts_texture.clone(),
                    TextureAtlas {
                        layout: asset_handles.karts_atlas.clone(),
                        index: player.kart_color.to_u32() as usize,
                    },
                ),
                NetworkedTransform,
            ));

            commands.spawn((
                DespawnOnExit(AppState::Game),
                FollowTransform(id),
                children![(
                    Text2d::new(player.name),
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
                DespawnOnExit(P2PLobbyState::OutOfLobby),
            ));
        }
        KartControlType::LobbyCar(networked_id, rank) => {
            let is_local = easy.get_local_player_id().unwrap() == networked_id;
            let is_host = easy.is_host();
            let player = easy.get_player_data(networked_id).unwrap();
            let name = if is_local { "(YOU)\n" } else { "" }.to_string() + &player.name;
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
                            index: 0,
                        },
                    ),
                    LobbyCar(networked_id),
                    DespawnOnExit(P2PLobbyState::InLobby),
                    DespawnOnExit(AppState::OutOfGame),
                ))
                .remove::<Collider>();

            let mut ui = commands.spawn((
                Visibility::Inherited,
                FollowTransform(id),
                children![(
                    Text2d::new(name),
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
                        observers!(|_: On<Pointer<Press>>, mut easy: KartEasyP2P| {
                            let local_player_data = easy.get_local_player_data();
                            easy.set_local_player_data(AppPlayerData {
                                name: local_player_data.name,
                                kart_color: local_player_data.kart_color.left(),
                            });
                        }),
                    ));
                    parent.spawn((
                        Transform::from_xyz(6., 0., SpriteLayers::Car.to_z()),
                        Button,
                        Sprite::from_image(asset_handles.arrow_texture.clone()),
                        Pickable::default(),
                        observers![|_: On<Pointer<Press>>, mut easy: KartEasyP2P| {
                            let local_player_data = easy.get_local_player_data();
                            easy.set_local_player_data(AppPlayerData {
                                name: local_player_data.name,
                                kart_color: local_player_data.kart_color.right(),
                            });
                        },],
                    ));
                });
            } else if is_host {
                ui.with_child((
                    Transform::from_xyz(0., -6., SpriteLayers::Car.to_z()),
                    Button,
                    Sprite::from_image(asset_handles.kick_texture.clone()),
                    Pickable::default(),
                    networked_id,
                    observers![|trigger: On<Pointer<Press>>,
                                mut easy: KartEasyP2P,
                                ids: Query<&NetworkedId>| {
                        easy.kick(*ids.get(trigger.event_target()).unwrap());
                    },],
                ));
            }
        }
    }
}

fn sync_wheel_rotation(
    mut updates: MessageReader<AppP2PUpdate>,
    mut w: MessageWriter<WheelPositionUpdate>,
) {
    for AppP2PUpdate(update) in updates.read() {
        if let EasyP2PUpdate::ClientInput { sender, input } = update {
            let update = WheelPositionUpdate(
                sender.clone(),
                if input.right {
                    WheelRotation::Right
                } else if input.left {
                    WheelRotation::Left
                } else {
                    WheelRotation::Straight
                },
            );
            w.write(update);
        }
    }
}

fn receive_wheel_rotation(
    easy: KartEasyP2P,
    mut r: MessageReader<WheelPositionUpdate>,
    mut wheels: Query<(Entity, &mut Transform, &CarController2dWheel)>,
) {
    if easy.is_host() {
        return;
    }
    for WheelPositionUpdate(target, rotation) in r.read() {
        for (entity, mut transform, wheel) in wheels.iter_mut() {
            if easy.get_closest_networked_id(entity) != Some(target.clone()) {
                continue;
            }
            if wheel.steerable {
                match rotation {
                    WheelRotation::Left => {
                        transform.rotation = Quat::from_rotation_z(45_f32.to_radians());
                    }
                    WheelRotation::Right => {
                        transform.rotation = Quat::from_rotation_z(-45_f32.to_radians());
                    }
                    WheelRotation::Straight => {
                        transform.rotation = Quat::from_rotation_z(0_f32.to_radians());
                    }
                }
            }
        }
    }
}

#[derive(Component)]
#[require(Transform)]

struct FollowTransform(Entity);

fn follow_transform(
    mut commands: Commands,
    transforms: Query<&Transform, Without<FollowTransform>>,
    mut follow_transforms: Query<(Entity, &mut Transform, &FollowTransform)>,
) {
    for (entity, mut transform, follow_transform) in follow_transforms.iter_mut() {
        if let Ok(target_transform) = transforms.get(follow_transform.0) {
            transform.translation = target_transform.translation;
        } else {
            commands.entity(entity).despawn();
        }
    }
}
