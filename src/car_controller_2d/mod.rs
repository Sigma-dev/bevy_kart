use crate::{PlayerInput, simulates};
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_ticked::prelude::*;
use bevy_ticked_avian::avian2d::TickedSimulationSet;
use bevy_ticked_networking::prelude::*;
use serde::{Deserialize, Serialize};

pub struct CarController2dPlugin;

impl Plugin for CarController2dPlugin {
    fn build(&self, app: &mut App) {
        // Before the physics step: everything here sets forces and impulses
        // for the step to integrate.
        app.add_systems(
            TickedSimulation,
            (
                apply_networked_inputs,
                (
                    car_controller_power,
                    car_controller_steering,
                    car_controller_traction,
                    handle_boost_effect,
                ),
            )
                .chain()
                .in_set(TickedSimulationSet::Input),
        );
    }
}

/// Replicated, so a client predicts a kart that cannot move yet as not moving,
/// instead of enabling it on its own clock, which runs ahead of the host's.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct CarControllerDisabled;

#[derive(Component)]
pub struct CarController2d {
    pub engine_force: f32,
}

#[derive(Component, Default, Clone, Debug, Serialize, Deserialize)]
pub struct CarControllerInputs {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
}

#[derive(Component)]
pub struct CarController2dWheel {
    pub powered: bool,
    pub steerable: bool,
}

impl CarController2dWheel {
    pub fn new(powered: bool, steerable: bool) -> Self {
        Self { powered, steerable }
    }
}

impl CarController2d {
    pub fn new(engine_force: f32) -> Self {
        Self { engine_force }
    }
}

#[derive(Component, Default, Clone, Debug, Serialize, Deserialize)]
pub struct SteeringState {
    /// Current steering angle, smoothly interpolated from -1.0 (right) to 1.0 (left).
    pub angle: f32,
}

/// Replicated: the host decides when a boost starts, and a client that does not
/// know about it predicts the wrong acceleration for the whole boost.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct BoostEffect {
    pub multiplier: f32,
    pub remaining_ticks: u64,
}

/// Read from the input queue and apply inputs to each car this peer simulates,
/// by `Owner`.
///
/// A player with no input for this tick keeps pressing what they last pressed:
/// the host relays other players' inputs, and holding the last one is what
/// keeps a predicted kart moving through the ticks between two arrivals.
fn apply_networked_inputs(
    mut commands: Commands,
    tick: Res<CurrentTick>,
    input_queue: Res<InputQueue<PlayerInput>>,
    local_client: Option<Res<LocalClientPlayer>>,
    cars: Query<(Entity, &Owner, Option<&ReplicationMode>), With<CarController2d>>,
) {
    let tick_inputs = input_queue.at_tick_or_last(tick.0);
    for (entity, owner, mode) in cars.iter() {
        if !simulates(local_client.as_deref(), mode) {
            continue;
        }
        if let Some(input) = tick_inputs.get(&owner.0) {
            commands.entity(entity).insert(CarControllerInputs {
                forward: input.forward,
                backward: input.backward,
                left: input.left,
                right: input.right,
            });
        }
    }
}

/// World-space pose of a wheel, derived from the kart's authoritative tick state
/// (`Position`/`Rotation`/`SteeringState`) instead of the render-interpolated
/// `GlobalTransform`. `GlobalTransform` is only propagated in `PostUpdate` and is
/// overwritten every frame by the sub-tick visual interpolation, so reading it
/// inside the tick makes forces frame-rate dependent and non-reproducible across
/// rollback. Returns `(up, right, world_position)`.
fn wheel_world_pose(
    kart_pos: Vec2,
    kart_angle: f32,
    steer_rad: f32,
    wheel_local: &Transform,
    steerable: bool,
) -> (Vec2, Vec2, Vec2) {
    let angle = if steerable {
        kart_angle + steer_rad
    } else {
        kart_angle
    };
    let (sin, cos) = angle.sin_cos();
    let up = Vec2::new(-sin, cos);
    let right = Vec2::new(cos, sin);
    let world_pos = kart_pos + Vec2::from_angle(kart_angle).rotate(wheel_local.translation.xy());
    (up, right, world_pos)
}

fn car_controller_power(
    local_client: Option<Res<LocalClientPlayer>>,
    mut cars: Query<
        (
            Forces,
            &Children,
            &CarController2d,
            &CarControllerInputs,
            Option<&BoostEffect>,
            &Position,
            &Rotation,
            &SteeringState,
            Option<&ReplicationMode>,
        ),
        (
            Without<CarController2dWheel>,
            Without<CarControllerDisabled>,
        ),
    >,
    wheels: Query<(&Transform, &CarController2dWheel)>,
) {
    for (mut force, children, car, inputs, maybe_boost_effect, pos, rot, steering, mode) in
        cars.iter_mut()
    {
        if !simulates(local_client.as_deref(), mode) {
            continue;
        }
        let mut dir = None;
        if inputs.forward {
            dir = Some(1.);
        } else if inputs.backward {
            dir = Some(-1.);
        }
        let Some(dir) = dir else {
            continue;
        };

        let base_mult = 16.;
        // Forward only: a boost is a burst of speed down the track, and applying
        // it to reverse as well made the item a way to rocket backwards out of a
        // bad spot.
        let boost = if dir > 0. {
            maybe_boost_effect.map_or(1., |boost_effect| boost_effect.multiplier)
        } else {
            1.
        };
        let kart_angle = rot.as_radians();
        let steer_rad = (steering.angle * 45.).to_radians();
        for child in children.iter() {
            let Ok((wheel_local, wheel)) = wheels.get(child) else {
                continue;
            };
            if !wheel.powered {
                continue;
            }
            let (up, _right, world_pos) =
                wheel_world_pose(pos.0, kart_angle, steer_rad, wheel_local, wheel.steerable);
            let power = up * car.engine_force * base_mult * boost * dir;
            force.apply_force_at_point(power, world_pos);
        }
    }
}

/// Smoothing rate per tick. At 64 tps, reaches ~95% of target in ~16 ticks (≈0.25s).
const STEERING_RATE: f32 = 0.18;

/// Turn the steering toward the input, and the front wheels with it.
///
/// The angle advances only on karts this peer simulates; on the others it is
/// the host's, replicated. The wheels follow the angle on every kart, because
/// they are the drawn part of it.
fn car_controller_steering(
    local_client: Option<Res<LocalClientPlayer>>,
    mut cars: Query<
        (
            &CarControllerInputs,
            &mut SteeringState,
            &Children,
            Option<&ReplicationMode>,
        ),
        With<CarController2d>,
    >,
    mut wheels: Query<(&mut Transform, &CarController2dWheel)>,
) {
    for (inputs, mut steering, children, mode) in cars.iter_mut() {
        if simulates(local_client.as_deref(), mode) {
            let target: f32 = if inputs.left {
                1.
            } else if inputs.right {
                -1.
            } else {
                0.
            };
            steering.angle += (target - steering.angle) * STEERING_RATE;
        }

        let rotation = Quat::from_rotation_z((steering.angle * 45.).to_radians());
        for child in children.iter() {
            let Ok((mut transform, wheel)) = wheels.get_mut(child) else {
                continue;
            };
            if !wheel.steerable {
                continue;
            }
            transform.rotation = rotation;
        }
    }
}

fn car_controller_traction(
    time: Res<Time>,
    local_client: Option<Res<LocalClientPlayer>>,
    wheels: Query<(&Transform, &CarController2dWheel, &ChildOf)>,
    mut cars: Query<(
        Forces,
        &Position,
        &Rotation,
        &SteeringState,
        Option<&ReplicationMode>,
    )>,
) {
    // Inside the tick, `Time` is the tick clock: one tick, on the first run and
    // on every replay.
    let dt = time.delta_secs();
    for (wheel_local, wheel, child_of) in wheels.iter() {
        let Ok((mut forces, pos, rot, steering, mode)) = cars.get_mut(child_of.0) else {
            continue;
        };
        if !simulates(local_client.as_deref(), mode) {
            continue;
        }
        let kart_angle = rot.as_radians();
        let steer_rad = (steering.angle * 45.).to_radians();
        let (_up, steering_dir, world_pos) =
            wheel_world_pose(pos.0, kart_angle, steer_rad, wheel_local, wheel.steerable);
        let velocity = forces.velocity_at_point(world_pos);
        let steering_vel = steering_dir.dot(velocity);
        let desired_vel_change = -steering_vel * 1. * 0.0002;
        let desired_accel = desired_vel_change / dt;
        let force = steering_dir * desired_accel;
        forces.apply_linear_impulse_at_point(force, world_pos);
    }
}

fn handle_boost_effect(
    mut commands: Commands,
    local_client: Option<Res<LocalClientPlayer>>,
    mut boost_effects: Query<(Entity, &mut BoostEffect, Option<&ReplicationMode>)>,
) {
    for (car_entity, mut boost_effect, mode) in boost_effects.iter_mut() {
        if !simulates(local_client.as_deref(), mode) {
            continue;
        }
        if boost_effect.remaining_ticks == 0 {
            commands.entity(car_entity).remove::<BoostEffect>();
        } else {
            boost_effect.remaining_ticks -= 1;
        }
    }
}
