use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;
use rand::seq::IndexedRandom;
use serde::{Deserialize, Serialize};

use crate::{
    AppState, EntityKind, OwnerPlayer, PlayerInput, SpriteLayers,
    car_controller_2d::{BoostEffect, CarController2d},
};

pub const ROCKET_EXPLOSION_RADIUS: f32 = 12.;
const BOOST_DURATION_TICKS: u64 = TICKS_PER_SECOND as u64;
const ITEM_RESPAWN_TICKS: u64 = TICKS_PER_SECOND as u64; // 1 second

pub struct ItemsPlugin;

impl Plugin for ItemsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
                TickedSimulation,
                (
                    spawn_items,
                    detect_item_pickup,
                    use_item,
                    move_rocket,
                    detect_rocket_collision,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                animate_rocket,
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
    interval_ticks: u64,
    item_exists: bool,
    last_pickup_tick: Option<u64>,
}

#[derive(Component, Debug)]
pub struct ItemPickup(pub ItemType);

/// Links a spawned item back to its spawner entity.
#[derive(Component)]
struct ItemSpawnerId(Entity);

/// Networked component: which item a car is holding.
#[derive(Component, Clone, Debug, Serialize, Deserialize, Default)]
pub struct HeldItem(pub Option<ItemType>);

#[derive(Component)]
pub struct Rocket;

pub fn spawn_spawner(commands: &mut Commands, position: Vec2) {
    commands.spawn((
        DespawnOnExit(AppState::Game),
        Transform::from_xyz(position.x, position.y, SpriteLayers::Car.to_z()),
        ItemSpawner {
            interval_ticks: ITEM_RESPAWN_TICKS,
            item_exists: false,
            last_pickup_tick: None,
        },
    ));
}

/// Host-only: spawn item crate entities at spawner positions on a tick-based interval.
fn spawn_items(
    tick: Res<CurrentTick>,
    mut commands: Commands,
    server_player: Option<Res<LocalServerPlayer>>,
    mut counter: ResMut<TickTrackedEntityCounter>,
    mut spawners: Query<(Entity, &Transform, &mut ItemSpawner)>,
) {
    if server_player.is_none() {
        return;
    }
    for (spawner_entity, transform, mut spawner) in spawners.iter_mut() {
        if spawner.item_exists {
            continue;
        }
        let ready = spawner
            .last_pickup_tick
            .is_none_or(|t| tick.0.saturating_sub(t) >= spawner.interval_ticks);
        if !ready {
            continue;
        }
        spawner.item_exists = true;
        let item = ItemType::random_possible_item();
        let tracked_id = counter.next();
        commands.spawn((
            DespawnOnExit(AppState::Game),
            *transform,
            crate::NetworkedPosition(transform.translation.xy()),
            ItemPickup(item),
            ItemSpawnerId(spawner_entity),
            EntityKind::ItemPickup(item),
            tracked_id,
            Collider::rectangle(4., 4.),
            Sensor,
            CollidingEntities::default(),
        ));
    }
}

/// Host-only: detect when a car overlaps an item crate and assign the held item.
fn detect_item_pickup(
    tick: Res<CurrentTick>,
    mut commands: Commands,
    server_player: Option<Res<LocalServerPlayer>>,
    items: Query<(Entity, &ItemPickup, &ItemSpawnerId, &CollidingEntities)>,
    cars: Query<(Entity, Option<&HeldItem>), (With<CarController2d>, With<OwnerPlayer>)>,
    mut spawners: Query<&mut ItemSpawner>,
) {
    if server_player.is_none() {
        return;
    }
    for (item_entity, item_pickup, spawner_id, colliding) in items.iter() {
        for &other in colliding.iter() {
            let Ok((car_entity, maybe_held)) = cars.get(other) else {
                continue;
            };
            // Skip if car already holds an item
            if maybe_held.is_some_and(|h| h.0.is_some()) {
                continue;
            }
            commands.entity(car_entity).insert(HeldItem(Some(item_pickup.0)));
            commands.entity(item_entity).despawn();
            if let Ok(mut spawner) = spawners.get_mut(spawner_id.0) {
                spawner.item_exists = false;
                spawner.last_pickup_tick = Some(tick.0);
            }
            break; // Only one car picks up this item
        }
    }
}

/// Host-only: when a player presses the item button, consume their held item.
fn use_item(
    mut commands: Commands,
    server_player: Option<Res<LocalServerPlayer>>,
    tick: Res<CurrentTick>,
    input_queue: Res<InputQueue<PlayerInput>>,
    mut counter: ResMut<TickTrackedEntityCounter>,
    cars: Query<(Entity, &OwnerPlayer, &HeldItem, &Transform), With<CarController2d>>,
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
        let Some((car_entity, _, held_item, transform)) =
            cars.iter().find(|(_, owner, _, _)| owner.0 == *uuid)
        else {
            continue;
        };
        let Some(item) = held_item.0 else {
            continue;
        };
        commands.entity(car_entity).insert(HeldItem(None));
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
                let z_angle = transform.rotation.to_euler(EulerRot::ZYX).0;
                let tracked_id = counter.next();
                commands.spawn((
                    DespawnOnExit(AppState::Game),
                    crate::NetworkedPosition(new_position),
                    crate::NetworkedRotation(z_angle),
                    Rocket,
                    EntityKind::Rocket,
                    tracked_id,
                    Collider::rectangle(2., 2.),
                    Sensor,
                    CollisionEventsEnabled,
                    CollidingEntities::default(),
                ));
            }
        }
    }
}

fn move_rocket(
    server_player: Option<Res<LocalServerPlayer>>,
    mut rockets: Query<(&mut crate::NetworkedPosition, &crate::NetworkedRotation), With<Rocket>>,
) {
    if server_player.is_none() {
        return;
    }
    for (mut pos, rot) in rockets.iter_mut() {
        let dir = Vec2::new(-rot.0.sin(), rot.0.cos());
        pos.0 += dir * (100. * SECONDS_PER_TICK);
    }
}

/// Host-only: detect rocket collisions with non-sensor entities, apply torque, spawn explosion entity.
fn detect_rocket_collision(
    mut commands: Commands,
    server_player: Option<Res<LocalServerPlayer>>,
    mut counter: ResMut<TickTrackedEntityCounter>,
    rockets: Query<(Entity, &crate::NetworkedPosition, &CollidingEntities), With<Rocket>>,
    non_sensors: Query<Entity, Without<Sensor>>,
    mut cars: Query<(&Transform, Forces), With<CarController2d>>,
) {
    if server_player.is_none() {
        return;
    }
    for (rocket_entity, rocket_pos, colliding) in rockets.iter() {
        let hit = colliding.iter().any(|e| non_sensors.get(*e).is_ok());
        if !hit {
            continue;
        }
        let rocket_xy = rocket_pos.0;
        // Apply torque to nearby cars
        for (car_transform, mut force) in cars.iter_mut() {
            if car_transform.translation.xy().distance(rocket_xy) >= ROCKET_EXPLOSION_RADIUS {
                continue;
            }
            let on_right = car_transform
                .right()
                .xy()
                .dot(rocket_xy - car_transform.translation.xy())
                > 0.;
            force.apply_torque(if on_right { 1. } else { -1. } * 10000.);
        }
        // Spawn tracked explosion entity so all peers render VFX
        let tracked_id = counter.next();
        commands.spawn((
            DespawnOnExit(AppState::Game),
            crate::NetworkedPosition(rocket_xy),
            EntityKind::Explosion,
            tracked_id,
        ));
        commands.entity(rocket_entity).despawn();
    }
}

/// Visual-only: animate rocket sprite.
fn animate_rocket(time: Res<Time>, mut rockets: Query<&mut Sprite, With<Rocket>>) {
    for mut sprite in rockets.iter_mut() {
        if let Some(atlas) = sprite.texture_atlas.as_mut() {
            atlas.index = (time.elapsed_secs() * 10.) as usize % 2;
        }
    }
}

