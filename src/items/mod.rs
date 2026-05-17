use audio_manager::prelude::*;
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_bundled_observers::observers;
use bevy_ensemble::prelude::*;
use bevy_ticked::prelude::*; // SECONDS_PER_TICK, TICKS_PER_SECOND, etc.
use bevy_ticked_networking::prelude::*;
use bevy_timer::{Timer, TimerFinished};
use rand::seq::IndexedRandom;
use serde::{Deserialize, Serialize};

use crate::{
    AppState, AssetHandles, EntityKind, OwnerPlayer, PlayerInput, SpriteLayers,
    car_controller_2d::{BoostEffect, CarController2d},
};

const ROCKET_EXPLOSION_RADIUS: f32 = 12.;
const BOOST_DURATION_TICKS: u64 = TICKS_PER_SECOND as u64; // 1 second

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
                handle_rocket_explosion,
                handle_explosion_car_spin,
            ),
        )
        .add_systems(
            TickedSimulation,
            move_rocket,
        );
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
}

#[derive(Component)]
pub struct ItemSpawner {
    interval: f32,
    item_exists: bool,
    last_pickup: Option<f32>,
}

#[derive(Component, Debug)]
pub struct ItemPickup(pub ItemType);

/// Ensemble message: an item was picked up.
#[derive(Clone, Debug, Serialize, Deserialize, Message)]
pub struct ItemPickedUp {
    pub item: ItemType,
    pub car_uuid: u128,
    pub crate_entity_id: u64,
    pub crate_position: Vec2,
}


#[derive(Component)]
pub struct Rocket;

#[derive(Component)]
pub struct RocketExplosion {
    pub start_time: f32,
}

/// Ensemble message: a rocket exploded.
#[derive(Clone, Debug, Serialize, Deserialize, Message)]
pub struct RocketExploded {
    pub position: Vec2,
    pub hit_car_uuids: Vec<u128>,
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
    mut commands: Commands,
    server_player: Option<Res<LocalServerPlayer>>,
    mut counter: ResMut<TickTrackedEntityCounter>,
    mut spawners: Query<(&Transform, &mut ItemSpawner), Without<ItemPickup>>,
    asset_handles: Res<AssetHandles>,
) {
    if server_player.is_none() {
        return;
    }
    for (transform, mut spawner) in spawners.iter_mut() {
        if !spawner.item_exists
            && spawner
                .last_pickup
                .is_none_or(|last_pickup| time.elapsed_secs() - last_pickup > spawner.interval)
        {
            spawner.item_exists = true;
            let item = ItemType::random_possible_item();
            let tracked_id = counter.next();
            commands.spawn((
                DespawnOnExit(AppState::Game),
                *transform,
                crate::NetworkedPosition(transform.translation.xy()),
                ItemPickup(item),
                EntityKind::ItemPickup(item),
                tracked_id,
                Sprite::from_image(asset_handles.crate_texture.clone()),
                Collider::rectangle(4., 4.),
                Sensor,
                CollisionEventsEnabled,
                observers!(
                    |_: On<Despawn, ItemPickup>, mut audio_manager: AudioManager| {
                        audio_manager.play_sound(PlayAudio2D::new_once("sounds/pickup.wav"));
                    }
                ),
            )).observe(
                |trigger: On<CollisionStart>,
                 item_pickups: Query<(&Transform, &TickTrackedEntity, &ItemPickup), With<ItemPickup>>,
                 car: Query<&OwnerPlayer, (With<CarController2d>, Without<ItemType>)>,
                 lobbies: Query<Entity, With<Lobby>>,
                 mut commands: Commands| {
                    let Ok(car_owner) = car.get(trigger.collider2) else {
                        return;
                    };
                    let Ok((transform, tracked, item_pickup)) =
                        item_pickups.get(trigger.event_target())
                    else {
                        return;
                    };
                    let Some(lobby) = lobbies.iter().next() else {
                        return;
                    };
                    let msg = ItemPickedUp {
                        item: item_pickup.0,
                        car_uuid: car_owner.0,
                        crate_entity_id: tracked.0,
                        crate_position: transform.translation.xy(),
                    };
                    commands
                        .entity(lobby)
                        .trigger(move |entity| BroadcastLobbyMessage::new(entity, msg));
                },
            );
        }
    }
}

fn on_item_picked_up(
    time: Res<Time>,
    server_player: Option<Res<LocalServerPlayer>>,
    mut commands: Commands,
    mut reader: MessageReader<ReceivedEnsembleMessage<ItemPickedUp>>,
    cars: Query<(Entity, &OwnerPlayer), With<CarController2d>>,
    tracked_entities: Query<(Entity, &TickTrackedEntity)>,
    mut spawners: Query<(&Transform, &mut ItemSpawner)>,
) {
    for msg in reader.read() {
        let picked_up = &msg.message;
        let Some((car_entity, _)) = cars
            .iter()
            .find(|(_, owner)| owner.0 == picked_up.car_uuid)
        else {
            continue;
        };
        commands.entity(car_entity).insert(picked_up.item);
        if server_player.is_none() {
            continue;
        }
        // Despawn the crate on host — snapshot propagates to clients
        if let Some((crate_entity, _)) = tracked_entities
            .iter()
            .find(|(_, t)| t.0 == picked_up.crate_entity_id)
        {
            commands.entity(crate_entity).despawn();
        }
        for (transform, mut spawner) in spawners.iter_mut() {
            if transform
                .translation
                .xy()
                .distance(picked_up.crate_position)
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
    server_player: Option<Res<LocalServerPlayer>>,
    tick: Res<CurrentTick>,
    input_queue: Res<InputQueue<PlayerInput>>,
    mut counter: ResMut<TickTrackedEntityCounter>,
    cars: Query<(Entity, &OwnerPlayer, &ItemType, &Transform), With<CarController2d>>,
    asset_handles: Res<AssetHandles>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    mut audio_manager: AudioManager,
) {
    if server_player.is_none() {
        return;
    }
    let Some(tick_inputs) = input_queue.at_tick(tick.0) else {
        return;
    };
    for (uuid, input) in tick_inputs.iter() {
        if !input.using_item {
            continue;
        }
        let Some((car_entity, _, item_type, transform)) =
            cars.iter().find(|(_, owner, _, _)| owner.0 == *uuid)
        else {
            continue;
        };
        let item = *item_type;
        commands.entity(car_entity).remove::<ItemType>();
        match item {
            ItemType::Boost => {
                commands.entity(car_entity).insert(BoostEffect {
                    multiplier: 3.,
                    remaining_ticks: BOOST_DURATION_TICKS,
                });
            }
            ItemType::Rocket => {
                let forward = transform.up().xy();
                let new_position = transform.translation.xy() + forward * 8.;
                let rocket_transform =
                    transform.with_translation(new_position.extend(transform.translation.z));
                let z_angle = transform.rotation.to_euler(EulerRot::ZYX).0;
                let tracked_id = counter.next();
                let layout = TextureAtlasLayout::from_grid(UVec2::new(3, 8), 2, 1, None, None);
                let texture_atlas_layout = texture_atlas_layouts.add(layout);
                let rocket_entity = commands
                    .spawn((
                        DespawnOnExit(AppState::Game),
                        rocket_transform,
                        crate::NetworkedPosition(new_position),
                        crate::NetworkedRotation(z_angle),
                        Sprite::from_atlas_image(
                            asset_handles.rocket_texture.clone(),
                            TextureAtlas {
                                layout: texture_atlas_layout,
                                index: 0,
                            },
                        ),
                        Rocket,
                        EntityKind::Rocket,
                        tracked_id,
                        Collider::rectangle(2., 2.),
                        Sensor,
                        CollisionEventsEnabled,
                    ))
                    .id();
                audio_manager.play_sound(
                    PlayAudio2D::new_once("sounds/rocket.wav")
                        .with_spatial(SpatialSettings2D::Entity(rocket_entity)),
                );
                commands.entity(rocket_entity).observe(
                    |trigger: On<CollisionStart>,
                     rockets: Query<&crate::NetworkedPosition, With<Rocket>>,
                     others: Query<Entity, Without<Sensor>>,
                     cars: Query<(&Transform, &OwnerPlayer), With<CarController2d>>,
                     lobbies: Query<Entity, With<Lobby>>,
                     mut commands: Commands| {
                        let Ok(rocket_pos) = rockets.get(trigger.event_target()) else {
                            return;
                        };
                        if others.get(trigger.collider2).is_err() {
                            return;
                        }
                        let Some(lobby) = lobbies.iter().next() else {
                            return;
                        };
                        let rocket_xy = rocket_pos.0;
                        // Despawn rocket on host
                        commands.entity(trigger.event_target()).despawn();
                        let hit_car_uuids = cars
                            .iter()
                            .filter(|(car_transform, _)| {
                                car_transform.translation.xy().distance(rocket_xy)
                                    < ROCKET_EXPLOSION_RADIUS
                            })
                            .map(|(_, owner)| owner.0)
                            .collect::<Vec<_>>();
                        let msg = RocketExploded {
                            position: rocket_xy,
                            hit_car_uuids,
                        };
                        commands
                            .entity(lobby)
                            .trigger(move |entity| BroadcastLobbyMessage::new(entity, msg));
                    },
                );
            }
        }
    }
}

fn animate_rocket(time: Res<Time>, mut rockets: Query<&mut Sprite, With<Rocket>>) {
    for mut sprite in rockets.iter_mut() {
        if let Some(atlas) = sprite.texture_atlas.as_mut() {
            atlas.index = (time.elapsed_secs() * 10.) as usize % 2;
        }
    }
}

fn move_rocket(
    mut rockets: Query<(&mut crate::NetworkedPosition, &crate::NetworkedRotation), With<Rocket>>,
) {
    for (mut pos, rot) in rockets.iter_mut() {
        // "up" direction: local Y rotated by the angle = (-sin θ, cos θ)
        let dir = Vec2::new(-rot.0.sin(), rot.0.cos());
        pos.0 += dir * (100. * SECONDS_PER_TICK);
    }
}

fn handle_rocket_explosion(
    mut commands: Commands,
    mut exploded_r: MessageReader<ReceivedEnsembleMessage<RocketExploded>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut audio_manager: AudioManager,
) {
    for msg in exploded_r.read() {
        let exploded_data = &msg.message;
        audio_manager.play_sound(PlayAudio2D::new_once("sounds/explosion.wav").with_volume(0.3));
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
    server_player: Option<Res<LocalServerPlayer>>,
    mut exploded_r: MessageReader<ReceivedEnsembleMessage<RocketExploded>>,
    mut cars: Query<(&OwnerPlayer, &Transform, Forces), With<CarController2d>>,
) {
    if server_player.is_none() {
        return;
    }
    for msg in exploded_r.read() {
        let exploded_data = &msg.message;
        for hit_uuid in &exploded_data.hit_car_uuids {
            let Some((_, transform, mut force)) =
                cars.iter_mut().find(|(owner, _, _)| owner.0 == *hit_uuid)
            else {
                continue;
            };
            let on_right = transform
                .right()
                .xy()
                .dot(exploded_data.position - transform.translation.xy())
                > 0.;
            force.apply_torque(if on_right { 1. } else { -1. } * 10000.);
        }
    }
}
