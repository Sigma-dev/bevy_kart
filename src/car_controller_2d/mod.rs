use crate::{AppPlayerInputData, KartEasyP2P};
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_easy_p2p::OnClientInput;

pub struct CarController2dPlugin;

impl Plugin for CarController2dPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (
                car_controller_power,
                car_controller_steering,
                car_controller_traction,
            ),
        );
    }
}

#[derive(Component)]
pub struct CarController2d {
    pub engine_force: f32,
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

fn car_controller_power(
    mut cars: Query<(Forces, Entity, &Children, &CarController2d), Without<CarController2dWheel>>,
    wheels: Query<(&GlobalTransform, &CarController2dWheel)>,
    mut param_set: ParamSet<(
        KartEasyP2P,
        MessageReader<OnClientInput<AppPlayerInputData>>,
    )>,
) {
    let inputs = param_set.p1().read().cloned().collect::<Vec<_>>();
    for OnClientInput(target, input) in inputs {
        for (mut force, entity, children, car) in cars.iter_mut() {
            if !param_set.p0().inputs_belong_to_player(entity, &target) {
                continue;
            }
            let mut dir = None;

            if input.forward {
                dir = Some(1.);
            } else if input.backward {
                dir = Some(-1.);
            }
            let Some(dir) = dir else {
                continue;
            };

            let base_mult = 8.;
            for child in children.iter() {
                let Ok((global_transform, wheel)) = wheels.get(child) else {
                    continue;
                };
                let power = global_transform.up().xy() * car.engine_force * base_mult * dir;
                if !wheel.powered {
                    continue;
                }
                force.apply_force_at_point(power, global_transform.translation().xy());
            }
        }
    }
}

fn car_controller_steering(
    mut cars: Query<(Entity, &Children), With<CarController2d>>,
    mut wheels: Query<(&mut Transform, &CarController2dWheel)>,
    mut param_set: ParamSet<(
        KartEasyP2P,
        MessageReader<OnClientInput<AppPlayerInputData>>,
    )>,
) {
    let inputs = param_set.p1().read().cloned().collect::<Vec<_>>();
    for OnClientInput(target, input) in inputs {
        for (entity, children) in cars.iter_mut() {
            if !param_set.p0().inputs_belong_to_player(entity, &target) {
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
