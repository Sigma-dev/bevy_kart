use audio_manager::prelude::*;
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_bundled_observers::observers;
use bevy_easy_p2p::{
    EasyP2PUpdate, NetworkedEntity, NetworkedEventsExt, prelude::NetworkedTransform,
};
use bevy_timer::{Timer, TimerFinished};
use rand::seq::IndexedRandom;
use serde::{Deserialize, Serialize};

use crate::{
    AppInstantiations, AppState, AssetHandles, KartEasyP2P, SpriteLayers,
    car_controller_2d::{BoostEffect, CarController2d},
};

const ROCKET_EXPLOSION_RADIUS: f32 = 12.;
pub struct ItemsPlugin;

impl Plugin for ItemsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                spawn_items,
                on_item_picked_up,
                use_item,
                animate_rocket,
                move_rocket,
                handle_rocket_explosion,
                handle_explosion_car_spin,
            ),
        )
        .init_networked_event::<ItemPickedUp>()
        .init_networked_event::<RocketExploded>();
    }
}

#[derive(Component, Copy, Clone, Debug, Serialize, Deserialize)]
pub enum ItemType {
    Boost,
    Rocket,
}

impl ItemType {
    fn possible_items() -> Vec<ItemType> {
        vec![ItemType::Boost, ItemType::Rocket]
    }

    fn random_possible_item() -> ItemType {
        *Self::possible_items().choose(&mut rand::rng()).unwrap()
    }

    pub fn to_index(&self) -> usize {
        match self {
            ItemType::Boost => 0,
            ItemType::Rocket => 1,
        }
    }

    fn run_effect(&self, commands: &mut Commands, car: NetworkedEntity, car_transform: Transform) {
        match self {
            ItemType::Boost => {
                commands.run_system_cached_with(boost_effect, car);
            }
            ItemType::Rocket => {
                commands.run_system_cached_with(rocket_effect, car_transform);
            }
        }
    }
}

#[derive(Component)]
pub struct ItemSpawner {
    interval: f32,
    item_exists: bool,
    last_pickup: Option<f32>,
}

#[derive(Component, Debug)]
pub struct ItemPickup(pub ItemType);

#[derive(Message, Clone, Debug, Serialize, Deserialize)]
pub struct ItemPickedUp {
    pub item: ItemType,
    pub car: NetworkedEntity,
    pub crate_id: u64,
    pub crate_position: Vec2,
}

#[derive(Component)]
pub struct Rocket;

#[derive(Component)]
pub struct RocketExplosion {
    pub start_time: f32,
}

#[derive(Message, Clone, Debug, Serialize, Deserialize)]
pub struct RocketExploded {
    pub position: Vec2,
    pub hit_cars: Vec<u64>,
}

pub fn spawn_spawner(commands: &mut Commands, position: Vec2) {
    commands.spawn((
        Transform::from_xyz(position.x, position.y, SpriteLayers::Car.to_z()),
        ItemSpawner {
            interval: 1.,
            item_exists: false,
            last_pickup: None,
        },
    ));
}

fn spawn_items(
    time: Res<Time>,
    mut easy: KartEasyP2P,
    mut spawners: Query<(&Transform, &mut ItemSpawner), Without<ItemPickup>>,
) {
    for (transform, mut spawner) in spawners.iter_mut() {
        if !spawner.item_exists
            && spawner
                .last_pickup
                .is_none_or(|last_pickup| time.elapsed_secs() - last_pickup > spawner.interval)
        {
            spawner.item_exists = true;
            easy.instantiate(
                AppInstantiations::ItemPickup(ItemType::random_possible_item()),
                transform.clone(),
            );
        }
    }
}

pub(crate) fn spawn_item_pickup(
    In((item, transform, networked_entity)): In<(ItemType, Transform, NetworkedEntity)>,
    easy: KartEasyP2P,
    mut commands: Commands,
    asset_handles: Res<AssetHandles>,
) {
    let mut item_pickup = commands.spawn((
        DespawnOnExit(AppState::Game),
        transform,
        ItemPickup(item),
        Sprite::from_image(asset_handles.crate_texture.clone()),
        networked_entity,
        observers!(
            |_: On<Despawn, ItemPickup>, mut audio_manager: AudioManager| {
                audio_manager.play_sound(PlayAudio2D::new_once("sounds/pickup.wav"));
            }
        ),
    ));
    if easy.is_host() {
        item_pickup
            .insert((Collider::rectangle(4., 4.), Sensor, CollisionEventsEnabled))
            .observe(
                |trigger: On<CollisionStart>,
                 item_pickups: Query<
                    (&Transform, &NetworkedEntity, &ItemPickup),
                    With<ItemPickup>,
                >,
                 car: Query<&NetworkedEntity, (With<CarController2d>, Without<ItemType>)>,
                 mut w: MessageWriter<ItemPickedUp>| {
                    let Ok(car) = car.get(trigger.collider2) else {
                        return;
                    };
                    let (transform, e, item_pickup) =
                        item_pickups.get(trigger.event_target()).unwrap();
                    w.write(ItemPickedUp {
                        item: item_pickup.0,
                        car: car.clone(),
                        crate_id: e.uuid(),
                        crate_position: transform.translation.xy(),
                    });
                },
            );
    }
}

fn on_item_picked_up(
    time: Res<Time>,
    mut easy: KartEasyP2P,
    mut commands: Commands,
    mut r: MessageReader<ItemPickedUp>,
    cars: Query<(Entity, &NetworkedEntity), With<CarController2d>>,
    mut spawners: Query<(&Transform, &mut ItemSpawner)>,
) {
    for picked_up_data in r.read() {
        let Some((car_entity, _)) = cars
            .iter()
            .find(|(_, c)| c.uuid() == picked_up_data.car.uuid())
        else {
            return;
        };
        commands.entity(car_entity).insert(picked_up_data.item);
        if !easy.is_host() {
            return;
        }
        easy.despawn(picked_up_data.crate_id);
        for (transform, mut spawner) in spawners.iter_mut() {
            if transform
                .translation
                .xy()
                .distance(picked_up_data.crate_position)
                < 1.
            {
                spawner.item_exists = false;
                spawner.last_pickup = Some(time.elapsed_secs());
                break;
            }
        }
    }
}

fn use_item(
    mut commands: Commands,
    mut easy: KartEasyP2P,
    cars: Query<(Entity, &NetworkedEntity, &ItemType, &Transform), With<CarController2d>>,
) {
    if !easy.is_host() {
        return;
    }
    let updates = easy.read_updates();
    let inputs = updates
        .iter()
        .filter_map(|update| match update {
            EasyP2PUpdate::ClientInput { sender, input } => {
                if input.using_item {
                    Some(sender)
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    for sender in inputs {
        let Some((car_entity, car, item_type, transform)) =
            cars.iter().find(|(_, car, _, _)| car.owner_id() == *sender)
        else {
            return;
        };
        commands.entity(car_entity).remove::<ItemType>();
        item_type.run_effect(&mut commands, car.clone(), *transform);
    }
}

fn rocket_effect(In(transform): In<Transform>, mut easy: KartEasyP2P) {
    let forward = transform.up().xy();
    let new_position = transform.translation.xy() + forward * 8.;
    easy.instantiate(
        AppInstantiations::Rocket,
        transform.with_translation(new_position.extend(transform.translation.z)),
    );
}

pub(crate) fn spawn_rocket(
    In((transform, networked_entity)): In<(Transform, NetworkedEntity)>,
    mut audio_manager: AudioManager,
    easy: KartEasyP2P,
    mut commands: Commands,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    asset_handles: Res<AssetHandles>,
) {
    let layout = TextureAtlasLayout::from_grid(UVec2::new(3, 8), 2, 1, None, None);
    let texture_atlas_layout = texture_atlas_layouts.add(layout);
    let mut rocket = commands.spawn((
        transform,
        Sprite::from_atlas_image(
            asset_handles.rocket_texture.clone(),
            TextureAtlas {
                layout: texture_atlas_layout,
                index: 0,
            },
        ),
        Rocket,
        networked_entity,
        NetworkedTransform,
    ));
    audio_manager.play_sound(
        PlayAudio2D::new_once("sounds/rocket.wav")
            .with_spatial(SpatialSettings2D::Entity(rocket.id())),
    );
    if easy.is_host() {
        rocket
            .insert((Collider::rectangle(2., 2.), Sensor, CollisionEventsEnabled))
            .observe(
                |trigger: On<CollisionStart>,
                 mut easy: KartEasyP2P,
                 rockets: Query<(&Transform, &NetworkedEntity), With<Rocket>>,
                 others: Query<Entity, Without<Sensor>>,
                 cars: Query<(&Transform, &NetworkedEntity), With<CarController2d>>,
                 mut w: MessageWriter<RocketExploded>| {
                    let Ok((transform, rocket)) = rockets.get(trigger.event_target()) else {
                        return;
                    };
                    if others.get(trigger.collider2).is_err() {
                        return;
                    }
                    if !easy.is_host() {
                        return;
                    }
                    easy.despawn(rocket.uuid());
                    let hit_cars = cars
                        .iter()
                        .filter(|(car_transform, _)| {
                            let dist = transform
                                .translation
                                .xy()
                                .distance(car_transform.translation.xy());
                            dist < ROCKET_EXPLOSION_RADIUS
                        })
                        .map(|(_, c)| c.uuid())
                        .collect::<Vec<_>>();
                    w.write(RocketExploded {
                        position: transform.translation.xy(),
                        hit_cars,
                    });
                },
            );
    }
}

fn boost_effect(
    In(networked_entity): In<NetworkedEntity>,
    mut commands: Commands,
    time: Res<Time>,
    cars: Query<(Entity, &NetworkedEntity), With<CarController2d>>,
) {
    let Some((car_entity, _)) = cars
        .iter()
        .find(|(_, c)| c.uuid() == networked_entity.uuid())
    else {
        return;
    };
    commands.entity(car_entity).insert(BoostEffect {
        multiplier: 3.,
        duration: 1.,
        start_time: time.elapsed_secs(),
    });
}

fn animate_rocket(time: Res<Time>, mut rockets: Query<&mut Sprite, With<Rocket>>) {
    for mut sprite in rockets.iter_mut() {
        sprite.texture_atlas.as_mut().unwrap().index = (time.elapsed_secs() * 10.) as usize % 2;
    }
}

fn move_rocket(
    time: Res<Time>,
    easy: KartEasyP2P,
    mut rockets: Query<&mut Transform, With<Rocket>>,
) {
    if !easy.is_host() {
        return;
    }
    for mut transform in rockets.iter_mut() {
        let up = transform.up();
        transform.translation += up * 100. * time.delta_secs();
    }
}

fn handle_rocket_explosion(
    mut commands: Commands,
    mut exploded_r: MessageReader<RocketExploded>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut audio_manager: AudioManager,
) {
    for exploded_data in exploded_r.read() {
        audio_manager.play_sound(PlayAudio2D::new_once("sounds/explosion.wav"));
        commands
            .spawn((
                Transform::from_xyz(
                    exploded_data.position.x,
                    exploded_data.position.y,
                    SpriteLayers::AboveCar.to_z(),
                ),
                Mesh2d(meshes.add(Circle::new(ROCKET_EXPLOSION_RADIUS))),
                MeshMaterial2d(materials.add(Color::WHITE)),
                Timer::new_running().with_target_duration(0.1),
            ))
            .observe(|timer: On<TimerFinished>, mut commands: Commands| {
                commands.entity(timer.event_target()).despawn();
            });
    }
}

fn handle_explosion_car_spin(
    easy: KartEasyP2P,
    mut exploded_r: MessageReader<RocketExploded>,
    mut cars: Query<(&NetworkedEntity, &Transform, Forces), With<CarController2d>>,
) {
    if !easy.is_host() {
        return;
    }
    for exploded_data in exploded_r.read() {
        for hit_car in &exploded_data.hit_cars {
            let Some((_, transform, mut force)) =
                cars.iter_mut().find(|(c, _, _)| c.uuid() == *hit_car)
            else {
                return;
            };
            // apply torque to the car depedning on the direction of the explosion
            let on_right = transform
                .right()
                .xy()
                .dot(exploded_data.position - transform.translation.xy())
                > 0.;

            force.apply_torque(if on_right { 1. } else { -1. } * 10000.);
        }
    }
}
