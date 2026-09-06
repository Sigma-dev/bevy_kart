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

pub const EXPLOSION_RADIUS: f32 = 12.;
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
                    detect_rocket_hits,
                    resolve_rocket_hits,
                    trigger_mines,
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
    Mine,
}

impl ItemType {
    /// Icons across `sprites/items.png`, in [`ItemType::to_index`] order. One
    /// per variant, so the HUD atlas is as wide as this enum.
    pub const ICON_COUNT: u32 = 3;

    fn possible_items() -> Vec<ItemType> {
        vec![ItemType::Boost, ItemType::Rocket, ItemType::Mine]
    }

    fn random_possible_item() -> ItemType {
        *Self::possible_items().choose(&mut rand::rng()).unwrap()
    }

    pub fn to_index(&self) -> usize {
        match self {
            ItemType::Boost => 0,
            ItemType::Rocket => 1,
            ItemType::Mine => 2,
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

/// A mine lying on the track, waiting for a kart to come close.
///
/// `armed` is the host's, and false until the mine has been clear of every kart
/// once. A mine is dropped *under* the kart that laid it, so on the tick it
/// appears there is already a kart inside its trigger radius; without arming it
/// would go off under its owner immediately. A clearance rather than a timer
/// because a timer is wrong exactly when it matters: a kart that has been spun
/// by an explosion, or is sitting still, would blow itself up waiting for the
/// clock. A client never runs the trigger, so its copy's `armed` is unused.
#[derive(Component, Default)]
pub struct Mine {
    armed: bool,
}

/// A rocket's hit, as this peer has seen it: where it stopped.
///
/// Registered for rollback but never sent, so every peer decides hits with the
/// same code and a client's wrong guess is undone by the ordinary rollback: at
/// the snapshot tick the marker is restored to absent, and the replay from
/// authoritative positions decides again. What a hit *does*, the explosion,
/// the torque and the despawn, stays with the host, because rollback cannot
/// undo a spawn or a despawn. So a client's rocket waits where it hit until the
/// host's explosion arrives, a prediction lead later, instead of flying on
/// through the wall.
#[derive(Component, Clone, Debug)]
pub struct RocketHit {
    pub at: Vec2,
}

const ROCKET_SPEED: f32 = 100.;
const ROCKET_HALF_SIZE: f32 = 1.;

/// How close a kart's centre has to get before an armed mine goes off, and how
/// far the kart that laid one has to drive for it to arm. See [`Mine`].
const MINE_TRIGGER_RADIUS: f32 = 5.;

fn rocket_direction(rot: &Rotation) -> Vec2 {
    let angle = rot.as_radians();
    Vec2::new(-angle.sin(), angle.cos())
}

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
            Position(transform.translation.xy()),
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
    cars: Query<(Entity, &OwnerPlayer, &HeldItem, &Position, &Rotation), With<CarController2d>>,
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
        let Some((car_entity, _, held_item, position, rotation)) =
            cars.iter().find(|(_, owner, _, _, _)| owner.0 == *uuid)
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
                // The tick pose, not the `Transform`: that one is the smoothed
                // view and can lag the simulation by up to a tick.
                let angle = rotation.as_radians();
                let forward = Vec2::from_angle(angle).rotate(Vec2::Y);
                let tracked_id = counter.next();
                // No collider: the hit is a shape cast in `detect_rocket_hits`,
                // which every peer runs, rather than an overlap only the host sees.
                commands.spawn((
                    DespawnOnExit(AppState::Game),
                    Position(position.0 + forward * 8.),
                    Rotation::radians(angle),
                    Rocket,
                    EntityKind::Rocket,
                    tracked_id,
                ));
            }
            ItemType::Mine => {
                // Under the kart, not behind it. It cannot go off until the kart
                // has driven out of its radius: see `Mine`.
                let tracked_id = counter.next();
                commands.spawn((
                    DespawnOnExit(AppState::Game),
                    Position(position.0),
                    Mine::default(),
                    EntityKind::Mine,
                    tracked_id,
                ));
            }
        }
    }
}

/// Runs on every peer. Straight-line flight from a replicated pose is
/// deterministic, so a client predicts it through rollback like a kart and the
/// rocket stays level with the karts around it instead of a prediction lead
/// behind them. A rocket that has hit stays where it hit.
fn move_rocket(
    mut rockets: Query<(&mut Position, &Rotation), (With<Rocket>, Without<RocketHit>)>,
) {
    for (mut pos, rot) in rockets.iter_mut() {
        pos.0 += rocket_direction(rot) * (ROCKET_SPEED * SECONDS_PER_TICK);
    }
}

/// Runs on every peer, with the same code: sweep the rocket's box over the step
/// it just flew and stop it at the first solid thing.
///
/// A swept box rather than an overlap test at the end of the step, so a rocket
/// cannot tunnel through anything thinner than one step, and so the answer
/// depends only on `Position` and `Rotation`, which rollback restores, and not
/// on collision state left over from before the rollback. Sensors, item crates
/// and other rockets, do not count.
fn detect_rocket_hits(
    mut commands: Commands,
    spatial: SpatialQuery,
    sensors: Query<(), With<Sensor>>,
    // Read-only, and the stop is written through commands: `SpatialQuery`
    // reads every `Position` itself, so a mutable query here conflicts with it.
    rockets: Query<(Entity, &Position, &Rotation), (With<Rocket>, Without<RocketHit>)>,
) {
    let shape = Collider::rectangle(ROCKET_HALF_SIZE * 2., ROCKET_HALF_SIZE * 2.);
    let step = ROCKET_SPEED * SECONDS_PER_TICK;
    for (rocket, pos, rot) in rockets.iter() {
        let dir = rocket_direction(rot);
        let Ok(direction) = Dir2::new(dir) else {
            continue;
        };
        let origin = pos.0 - dir * step;
        let hit = spatial.cast_shape_predicate(
            &shape,
            origin,
            rot.as_radians(),
            direction,
            &ShapeCastConfig {
                max_distance: step,
                ..default()
            },
            &SpatialQueryFilter::default(),
            &|entity| entity != rocket && !sensors.contains(entity),
        );
        if let Some(hit) = hit {
            let at = origin + dir * hit.distance;
            debug!("rocket {rocket} hit {} at {at:?}", hit.entity);
            commands
                .entity(rocket)
                .insert((Position(at), RocketHit { at }));
        }
    }
}

/// Host-only: what a hit does. Torque on the karts in the blast, an explosion
/// entity for every peer to render, and the rocket goes.
fn resolve_rocket_hits(
    mut commands: Commands,
    server_player: Option<Res<LocalServerPlayer>>,
    mut counter: ResMut<TickTrackedEntityCounter>,
    rockets: Query<(Entity, &RocketHit), With<Rocket>>,
    mut cars: Query<(&Position, &Rotation, Forces), With<CarController2d>>,
) {
    if server_player.is_none() {
        return;
    }
    for (rocket_entity, hit) in rockets.iter() {
        explode(&mut commands, &mut counter, &mut cars, hit.at);
        commands.entity(rocket_entity).despawn();
    }
}

/// Host-only: a mine arms on the first tick no kart is within
/// [`MINE_TRIGGER_RADIUS`] of it, and after that goes off, with the rocket's
/// explosion, on the first tick one is.
///
/// Host-only, unlike [`detect_rocket_hits`], and with no rollback marker of its
/// own. The marker exists for the rocket because a rocket that has hit has to
/// *stop*, and a client that waited for the host's word would fly it on through
/// the wall for a prediction lead. A mine does not move, so there is nothing for
/// a client to predict: everything a trigger does — the torque, the explosion,
/// the despawn — is the host's, and arrives with the next snapshot.
fn trigger_mines(
    mut commands: Commands,
    server_player: Option<Res<LocalServerPlayer>>,
    mut counter: ResMut<TickTrackedEntityCounter>,
    mut mines: Query<(Entity, &Position, &mut Mine)>,
    mut cars: Query<(&Position, &Rotation, Forces), With<CarController2d>>,
) {
    if server_player.is_none() {
        return;
    }
    for (mine_entity, mine_pos, mut mine) in mines.iter_mut() {
        let kart_in_range = cars
            .iter()
            .any(|(car_pos, _, _)| car_pos.0.distance(mine_pos.0) < MINE_TRIGGER_RADIUS);
        if !mine.armed {
            // The kart that laid it is standing on it. Arm as soon as it is not.
            mine.armed = !kart_in_range;
            continue;
        }
        if !kart_in_range {
            continue;
        }
        debug!("mine {mine_entity} triggered at {:?}", mine_pos.0);
        explode(&mut commands, &mut counter, &mut cars, mine_pos.0);
        commands.entity(mine_entity).despawn();
    }
}

/// Host-only: what an explosion does, shared by the rocket and the mine. Torque
/// on the karts in the blast, and a tracked explosion entity for every peer to
/// render.
fn explode(
    commands: &mut Commands,
    counter: &mut TickTrackedEntityCounter,
    cars: &mut Query<(&Position, &Rotation, Forces), With<CarController2d>>,
    at: Vec2,
) {
    for (car_pos, car_rot, mut force) in cars.iter_mut() {
        if car_pos.0.distance(at) >= EXPLOSION_RADIUS {
            continue;
        }
        let right = Vec2::from_angle(car_rot.as_radians()).rotate(Vec2::X);
        let on_right = right.dot(at - car_pos.0) > 0.;
        force.apply_torque(if on_right { 1. } else { -1. } * 10000.);
    }
    let tracked_id = counter.next();
    commands.spawn((
        DespawnOnExit(AppState::Game),
        Position(at),
        EntityKind::Explosion,
        tracked_id,
    ));
}

/// Visual-only: animate rocket sprite.
fn animate_rocket(time: Res<Time>, mut rockets: Query<&mut Sprite, With<Rocket>>) {
    for mut sprite in rockets.iter_mut() {
        if let Some(atlas) = sprite.texture_atlas.as_mut() {
            atlas.index = (time.elapsed_secs() * 10.) as usize % 2;
        }
    }
}


#[cfg(test)]
mod tests {
    //! The rocket's flight and hit, headless: ticks, physics and this module, no
    //! window, assets or network.
    use super::*;
    use crate::{PlayerInput, register_networked_components};
    use bevy::time::TimeUpdateStrategy;
    use bevy_ticked::tick::ResetToTick;
    use std::time::Duration;

    /// One tick per `app.update()`.
    const TICK: Duration = Duration::from_micros(15_625);
    /// A wall across the rocket's path at x = 20, two units thick: its near face
    /// is at 19, so a rocket a unit wide stops with its centre at 18.
    const STOP_X: f32 = 18.;

    /// Ticks, physics and this module: no window, assets or network, and
    /// nothing in the world yet.
    fn base_app(host: bool) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(TICK))
            .add_plugins(TickedPlugin {
                source: TickSource::Hz(64.0),
                ..default()
            })
            .add_plugins(PhysicsPlugins::new(TickedSimulation))
            .insert_resource(Gravity::ZERO)
            .insert_resource(avian2d::physics_transform::PhysicsTransformConfig {
                transform_to_position: false,
                ..default()
            })
            // Avian's per-step diagnostics counters, which its plugins only
            // create when `bevy_diagnostic` is on, but read unconditionally.
            .init_resource::<avian2d::collider_tree::ColliderTreeDiagnostics>()
            .init_resource::<avian2d::collision::CollisionDiagnostics>()
            .init_resource::<avian2d::dynamics::solver::SolverDiagnostics>()
            .init_resource::<avian2d::spatial_query::SpatialQueryDiagnostics>()
            .init_resource::<InputQueue<PlayerInput>>()
            .add_plugins(ItemsPlugin);
        register_networked_components(&mut app);
        if host {
            app.insert_resource(LocalServerPlayer(1));
        }
        app
    }

    /// A rocket at the origin and a wall for it to hit.
    fn app(host: bool) -> App {
        let mut app = base_app(host);
        app.world_mut().spawn((
            RigidBody::Static,
            Collider::rectangle(2., 40.),
            Position(Vec2::new(20., 0.)),
        ));
        // Facing +x: the flight direction is (-sin a, cos a).
        app.world_mut().spawn((
            Position(Vec2::ZERO),
            Rotation::degrees(-90.),
            Rocket,
            EntityKind::Rocket,
            TickTrackedEntity(1),
        ));
        app
    }

    /// A mine at the origin and a kart parked at `car_at`.
    fn mine_app(host: bool, car_at: Vec2) -> (App, Entity) {
        let mut app = base_app(host);
        app.world_mut().spawn((
            Position(Vec2::ZERO),
            Mine::default(),
            EntityKind::Mine,
            TickTrackedEntity(1),
        ));
        let car = app
            .world_mut()
            .spawn((
                Position(car_at),
                Rotation::default(),
                CarController2d::new(1.),
                Mass(1.),
                RigidBody::Dynamic,
                Collider::rectangle(4., 8.),
            ))
            .id();
        (app, car)
    }

    fn mine_count(app: &mut App) -> usize {
        let mut query = app.world_mut().query_filtered::<(), With<Mine>>();
        query.iter(app.world()).count()
    }

    fn rocket(app: &mut App) -> Option<(Vec2, Option<Vec2>)> {
        let mut query = app.world_mut().query_filtered::<(&Position, Option<&RocketHit>), With<Rocket>>();
        query
            .iter(app.world())
            .next()
            .map(|(pos, hit)| (pos.0, hit.map(|h| h.at)))
    }

    fn explosion(app: &mut App) -> Option<Vec2> {
        let mut query = app.world_mut().query::<(&EntityKind, &Position)>();
        query
            .iter(app.world())
            .find(|(kind, _)| matches!(kind, EntityKind::Explosion))
            .map(|(_, pos)| pos.0)
    }

    /// Step until the rocket has hit, returning the tick it happened on.
    fn fly_until_hit(app: &mut App) -> u64 {
        for _ in 0..40 {
            app.update();
            if let Some((_, Some(_))) = rocket(app) {
                return app.world().resource::<CurrentTick>().0;
            }
            if explosion(app).is_some() {
                return app.world().resource::<CurrentTick>().0;
            }
        }
        panic!("the rocket never reached the wall");
    }

    #[test]
    fn the_host_explodes_the_rocket_at_the_wall() {
        let mut app = app(true);
        fly_until_hit(&mut app);
        app.update();
        assert!(rocket(&mut app).is_none(), "the host despawns a rocket that hit");
        let at = explosion(&mut app).expect("the host spawns an explosion");
        assert!((at.x - STOP_X).abs() < 0.2, "explosion at {at:?}, expected x = {STOP_X}");
    }

    #[test]
    fn a_client_stops_the_rocket_at_the_wall_and_waits() {
        let mut app = app(false);
        fly_until_hit(&mut app);
        let (pos, hit) = rocket(&mut app).expect("a client keeps the rocket");
        assert!((pos.x - STOP_X).abs() < 0.2, "stopped at {pos:?}, expected x = {STOP_X}");
        assert_eq!(hit, Some(pos), "the marker records where it stopped");
        for _ in 0..10 {
            app.update();
        }
        let (later, _) = rocket(&mut app).expect("still there, still waiting");
        assert_eq!(later, pos, "a rocket that hit does not move on");
        assert!(explosion(&mut app).is_none(), "the explosion is the host's to make");
    }

    /// The reason the marker is a rollback component: rewinding to before the
    /// hit takes it away, and the replay decides the hit again from scratch.
    #[test]
    fn rolling_back_before_the_hit_forgets_it_and_the_replay_decides_again() {
        let mut app = app(false);
        let hit_tick = fly_until_hit(&mut app);
        let (_, first) = rocket(&mut app).unwrap();

        app.world_mut().write_message(ResetToTick(hit_tick - 3));
        app.update();
        let (pos, hit) = rocket(&mut app).unwrap();
        assert!(hit.is_none(), "the rewind restored the tick before the hit");
        assert!(pos.x < STOP_X - 1.0, "and the rocket is back in flight at {pos:?}");

        for _ in 0..5 {
            app.update();
        }
        let (_, again) = rocket(&mut app).unwrap();
        assert_eq!(again, first, "the replay reaches the same wall at the same spot");
    }

    #[test]
    fn a_mine_lies_there_until_a_kart_comes_close() {
        let (mut app, car) = mine_app(true, Vec2::new(MINE_TRIGGER_RADIUS + 1., 0.));
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(mine_count(&mut app), 1, "just out of reach, so it waits");
        assert!(explosion(&mut app).is_none());

        app.world_mut().entity_mut(car).insert(Position(Vec2::new(MINE_TRIGGER_RADIUS - 1., 0.)));
        app.update();
        app.update();
        assert_eq!(mine_count(&mut app), 0, "the kart came close, so the mine went");
        let at = explosion(&mut app).expect("and it explodes like a rocket");
        assert_eq!(at, Vec2::ZERO, "where the mine lay");
    }

    /// The trigger is the host's, like the rocket's explosion: a client's mine
    /// waits for the snapshot rather than guessing. The kart drives clear and
    /// comes back, which on a host is the whole arm-and-explode sequence.
    #[test]
    fn a_client_does_not_trigger_its_own_mines() {
        let (mut app, car) = mine_app(false, Vec2::new(MINE_TRIGGER_RADIUS + 1., 0.));
        app.update();
        app.world_mut().entity_mut(car).insert(Position(Vec2::ZERO));
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(mine_count(&mut app), 1, "a client leaves the mine alone");
        assert!(explosion(&mut app).is_none());
    }

    /// The mine is dropped under its own kart, so this is the case that would
    /// blow the layer up on the tick they used the item.
    #[test]
    fn a_mine_under_its_own_kart_waits_for_it_to_drive_clear() {
        let (mut app, car) = mine_app(true, Vec2::ZERO);
        for _ in 0..20 {
            app.update();
        }
        assert_eq!(mine_count(&mut app), 1, "sitting on it does not set it off");
        assert!(explosion(&mut app).is_none(), "and nobody blows themselves up");

        // Drive clear: the mine arms.
        app.world_mut()
            .entity_mut(car)
            .insert(Position(Vec2::new(MINE_TRIGGER_RADIUS + 1., 0.)));
        app.update();
        assert_eq!(mine_count(&mut app), 1, "still there, now armed");
        assert!(explosion(&mut app).is_none());

        // Come back over it.
        app.world_mut().entity_mut(car).insert(Position(Vec2::ZERO));
        app.update();
        app.update();
        assert_eq!(mine_count(&mut app), 0, "an armed mine goes off underfoot");
        assert!(explosion(&mut app).is_some());
    }
}
