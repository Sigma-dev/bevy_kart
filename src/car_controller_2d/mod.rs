use crate::{AppP2PUpdate, KartEasyP2P};
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_easy_p2p::{EasyP2PUpdate, NetworkedEntity};

pub struct CarController2dPlugin;

impl Plugin for CarController2dPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (
                handle_networked_inputs,
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

#[derive(Component, Default)]
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
    pub duration: f32,
    pub start_time: f32,
}

fn handle_networked_inputs(
    mut commands: Commands,
    mut cars: Query<(Entity, &NetworkedEntity), With<CarController2d>>,
    mut param_set: ParamSet<(KartEasyP2P, MessageReader<AppP2PUpdate>)>,
) {
    let inputs = param_set
        .p1()
        .read()
        .filter_map(|AppP2PUpdate(update)| match update {
            EasyP2PUpdate::ClientInput { sender, input } => Some((sender.clone(), input.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();

    for (entity, networked_entity) in cars.iter_mut() {
        if let Some(input) = inputs
            .iter()
            .find(|(id, _)| *id == networked_entity.owner_id())
            .map(|(_, input)| input)
        {
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
    mut cars: Query<(Entity, &Children), With<CarController2d>>,
    mut wheels: Query<(&mut Transform, &CarController2dWheel)>,
    mut param_set: ParamSet<(KartEasyP2P, MessageReader<AppP2PUpdate>)>,
) {
    let inputs = param_set
        .p1()
        .read()
        .filter_map(|AppP2PUpdate(update)| match update {
            EasyP2PUpdate::ClientInput { sender, input } => Some((sender.clone(), input.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    for (sender, input) in inputs {
        for (entity, children) in cars.iter_mut() {
            if !param_set.p0().inputs_belong_to_player(entity, &sender) {
                continue;
            }
            let mut dir: f32 = 0.;

            if input.left {
                dir = 1.;
            } else if input.right {
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
}

fn car_controller_traction(
    time: Res<Time>,
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
        let desired_accel = desired_vel_change / time.delta_secs();
        let force = steering_dir * desired_accel;
        forces.apply_linear_impulse_at_point(force, global_transform.translation().xy());
    }
}

fn handle_boost_effect(
    mut commands: Commands,
    time: Res<Time>,
    mut boost_effects: Query<(Entity, &BoostEffect)>,
) {
    for (car_entity, boost_effect) in boost_effects.iter_mut() {
        if time.elapsed_secs() - boost_effect.start_time > boost_effect.duration {
            commands.entity(car_entity).remove::<BoostEffect>();
        }
    }
}
