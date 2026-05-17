use crate::{OwnerPlayer, PlayerInput};
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;
use serde::{Deserialize, Serialize};

pub struct CarController2dPlugin;

impl Plugin for CarController2dPlugin {
    fn build(&self, app: &mut App) {
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
                .chain(),
        );
    }
}

#[derive(Component)]
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

#[derive(Component)]
pub struct BoostEffect {
    pub multiplier: f32,
    pub remaining_ticks: u64,
}

/// Read from InputQueue and apply inputs to each car based on OwnerPlayer.
fn apply_networked_inputs(
    mut commands: Commands,
    tick: Res<CurrentTick>,
    input_queue: Res<InputQueue<PlayerInput>>,
    mut cars: Query<(Entity, &OwnerPlayer), With<CarController2d>>,
) {
    let Some(tick_inputs) = input_queue.at_tick(tick.0) else {
        return;
    };
    for (entity, owner) in cars.iter_mut() {
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

fn car_controller_power(
    mut cars: Query<
        (
            Forces,
            &Children,
            &CarController2d,
            &CarControllerInputs,
            Option<&BoostEffect>,
        ),
        (
            Without<CarController2dWheel>,
            Without<CarControllerDisabled>,
        ),
    >,
    wheels: Query<(&GlobalTransform, &CarController2dWheel)>,
) {
    for (mut force, children, car, inputs, maybe_boost_effect) in cars.iter_mut() {
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
        for child in children.iter() {
            let Ok((global_transform, wheel)) = wheels.get(child) else {
                continue;
            };
            if !wheel.powered {
                continue;
            }
            let power = global_transform.up().xy()
                * car.engine_force
                * base_mult
                * maybe_boost_effect.map_or(1., |boost_effect| boost_effect.multiplier)
                * dir;
            force.apply_force_at_point(power, global_transform.translation().xy());
        }
    }
}

fn car_controller_steering(
    cars: Query<(&CarControllerInputs, &Children), With<CarController2d>>,
    mut wheels: Query<(&mut Transform, &CarController2dWheel)>,
) {
    for (inputs, children) in cars.iter() {
        let mut dir: f32 = 0.;
        if inputs.left {
            dir = 1.;
        } else if inputs.right {
            dir = -1.;
        }
        let rotation = Quat::from_rotation_z((dir * 45.).to_radians());
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
    wheels: Query<(&GlobalTransform, &CarController2dWheel, &ChildOf)>,
    mut cars: Query<Forces>,
) {
    for (global_transform, _wheel, child_of) in wheels.iter() {
        let Ok(mut forces) = cars.get_mut(child_of.0) else {
            continue;
        };
        let steering_dir = global_transform.right().as_vec3().xy();
        let velocity = forces.velocity_at_point(global_transform.translation().xy());
        let steering_vel = steering_dir.dot(velocity);
        let desired_vel_change = -steering_vel * 1. * 0.0002;
        let desired_accel = desired_vel_change / SECONDS_PER_TICK;
        let force = steering_dir * desired_accel;
        forces.apply_linear_impulse_at_point(force, global_transform.translation().xy());
    }
}

fn handle_boost_effect(
    mut commands: Commands,
    mut boost_effects: Query<(Entity, &mut BoostEffect)>,
) {
    for (car_entity, mut boost_effect) in boost_effects.iter_mut() {
        if boost_effect.remaining_ticks == 0 {
            commands.entity(car_entity).remove::<BoostEffect>();
        } else {
            boost_effect.remaining_ticks -= 1;
        }
    }
}
