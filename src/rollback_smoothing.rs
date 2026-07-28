use std::f32::consts::{PI, TAU};

use avian2d::prelude::*;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy_ticked::prelude::*;

use crate::kart::LocalKart;

pub struct RollbackSmoothingPlugin;

impl Plugin for RollbackSmoothingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PreRollbackState>()
            // Both of these must bracket the tick itself, so they live in
            // TickedLoop rather than in Bevy's fixed schedules: with the tick
            // rate decoupled from FixedUpdate, FixedPreUpdate no longer runs
            // once per tick and the saved baseline would go stale.
            .add_systems(
                TickedLoop,
                save_pre_rollback_positions.before(TickedSystems::PreTick),
            )
            .add_systems(
                TickedLoop,
                detect_corrections
                    .after(TickedSystems::PreTick)
                    .before(TickedSystems::Tick),
            )
            .add_systems(
                PostUpdate,
                apply_correction_offset
                    .in_set(ApplyCorrectionSet)
                    .before(TransformSystems::Propagate),
            );
    }
}

/// Stores a visual offset that absorbs rollback correction jumps and decays over time.
/// Also handles sub-tick interpolation (replacing avian's easing for karts).
#[derive(Component, Default)]
pub struct CorrectionSmoothing {
    prev_position: Vec2,
    prev_rotation: f32,
    position_offset: Vec2,
    rotation_offset: f32,
    initialized: bool,
}

/// System set for correction offset application, used for ordering.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApplyCorrectionSet;

/// Decay rate (1/seconds) for correction offsets.
const CORRECTION_DECAY_RATE: f32 = 20.0;

/// Max correction offset magnitude. Corrections beyond this snap immediately
/// to avoid the visual clipping through walls.
const MAX_CORRECTION_OFFSET: f32 = 5.0;

/// Positions saved before rollback to detect correction deltas.
#[derive(Resource, Default)]
struct PreRollbackState {
    entries: HashMap<Entity, (Vec2, f32)>,
}

fn save_pre_rollback_positions(
    mut state: ResMut<PreRollbackState>,
    karts: Query<(Entity, &Position, &Rotation), With<CorrectionSmoothing>>,
) {
    state.entries.clear();
    for (entity, pos, rot) in karts.iter() {
        state.entries.insert(entity, (pos.0, rot.as_radians()));
    }
}

fn detect_corrections(
    state: Res<PreRollbackState>,
    mut karts: Query<(Entity, &Position, &Rotation, &mut CorrectionSmoothing)>,
    local_kart: Query<Entity, With<LocalKart>>,
) {
    let local_entity = local_kart.iter().next();

    for (entity, pos, rot, mut smoothing) in karts.iter_mut() {
        let curr_pos = pos.0;
        let curr_rot = rot.as_radians();

        if smoothing.initialized {
            // Only absorb corrections for remote karts. The local kart's predictions
            // are accurate (same inputs replayed) and smoothing them causes jitter
            // from physics non-determinism during rollback replay.
            let is_local = local_entity.is_some_and(|e| e == entity);
            if !is_local {
                if let Some(&(old_pos, old_rot)) = state.entries.get(&entity) {
                    let correction_pos = curr_pos - old_pos;
                    let correction_rot = (curr_rot - old_rot + PI).rem_euclid(TAU) - PI;
                    smoothing.position_offset -= correction_pos;
                    smoothing.rotation_offset -= correction_rot;

                    // Clamp to prevent visual clipping through walls
                    smoothing.position_offset =
                        smoothing.position_offset.clamp_length_max(MAX_CORRECTION_OFFSET);
                }
            }
        } else {
            smoothing.initialized = true;
        }

        smoothing.prev_position = curr_pos;
        smoothing.prev_rotation = curr_rot;
    }
}

/// Sub-tick interpolation + correction offset decay and application.
/// Replaces avian's easing for karts (they have NoTransformEasing).
fn apply_correction_offset(
    interpolation: TickInterpolation,
    time: Res<Time>,
    mut query: Query<(&Position, &Rotation, &mut Transform, &mut CorrectionSmoothing)>,
) {
    let alpha = interpolation.fraction();
    let dt = time.delta_secs();
    let decay = if dt > 0.0 { (-CORRECTION_DECAY_RATE * dt).exp() } else { 1.0 };

    for (pos, rot, mut transform, mut smoothing) in query.iter_mut() {
        if !smoothing.initialized {
            continue;
        }

        let curr_pos = pos.0;
        let curr_rot = rot.as_radians();

        // Sub-tick interpolation between prev (tick N) and current (tick N+1)
        let interp_pos = smoothing.prev_position.lerp(curr_pos, alpha);
        let rot_diff = (curr_rot - smoothing.prev_rotation + PI).rem_euclid(TAU) - PI;
        let interp_rot = smoothing.prev_rotation + rot_diff * alpha;

        // Decay correction offset
        smoothing.position_offset *= decay;
        smoothing.rotation_offset *= decay;
        if smoothing.position_offset.length_squared() < 1e-6 {
            smoothing.position_offset = Vec2::ZERO;
        }
        if smoothing.rotation_offset.abs() < 1e-6 {
            smoothing.rotation_offset = 0.0;
        }

        let z = transform.translation.z;
        transform.translation = (interp_pos + smoothing.position_offset).extend(z);
        transform.rotation = Quat::from_rotation_z(interp_rot + smoothing.rotation_offset);
    }
}
