use crate::car_controller_2d::{CarController2d, CarControllerDisabled};
use crate::{AppP2PUpdate, KartEasyP2P, NetworkedEntity, car_controller_2d::CarController2dWheel};
use crate::{AppState, AssetHandles, SpriteLayers};
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_easy_p2p::prelude::*;
use serde::{Deserialize, Serialize};
pub struct KartPlugin;

impl Plugin for KartPlugin {
    fn build(&self, app: &mut App) {
        app.init_networked_event::<WheelPositionUpdate>()
            .add_systems(
                Update,
                (
                    sync_wheel_rotation,
                    receive_wheel_rotation,
                    follow_transform,
                    sync_wheel_rotation.after(EasyP2PSystemSet::Emit),
                    receive_wheel_rotation.after(EasyP2PSystemSet::Emit),
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WheelRotation {
    Left,
    Right,
    Straight,
}

#[derive(Message, Clone, Debug, Serialize, Deserialize)]
struct WheelPositionUpdate(NetworkedId, WheelRotation);

pub(crate) fn spawn_kart(
    In((networked_entity, transform)): In<(NetworkedEntity, Transform)>,
    mut commands: Commands,
    easy: KartEasyP2P,
    asset_handles: Res<AssetHandles>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let player = easy.get_player_data(networked_entity.owner_id().clone());
    let layout = TextureAtlasLayout::from_grid(KART_SIZE, KART_COLORS_COUNT, 1, None, None);
    let texture_atlas_layout = texture_atlas_layouts.add(layout);
    let half_car_width = 2.5;
    let half_car_length = 3.;
    let id = commands
        .spawn((
            DespawnOnExit(AppState::Game),
            Mass(1.),
            RigidBody::Dynamic,
            Collider::rectangle(4., 8.),
            transform,
            NetworkedTransform,
            networked_entity,
            CarController2d::new(1.),
            CarControllerDisabled,
            LapsCounter(0),
            Sprite::from_atlas_image(
                asset_handles.karts_texture.clone(),
                TextureAtlas {
                    layout: texture_atlas_layout,
                    index: player.kart_color.to_u32() as usize,
                },
            ),
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
    commands.spawn((
        DespawnOnExit(AppState::Game),
        FollowTransform(id),
        children![(
            Text2d::new(player.name),
            Transform::from_xyz(0., 5., SpriteLayers::AboveCar.to_z()).with_scale(Vec3::splat(0.1)),
        )],
    ));
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
