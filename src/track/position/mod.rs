use bevy::prelude::*;
use bevy_ticked::prelude::*;

use crate::{
    kart::{LapUpdate, LapsCounter},
    track::position::progress_line::{Progress, TrackProgress},
};

pub struct RacePositionPlugin;

pub mod progress_line;

impl Plugin for RacePositionPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(progress_line::ProgressLinePlugin)
            .add_systems(TickedSimulation, compute_race_position);
    }
}

#[derive(Component, Debug)]
pub struct RacePosition {
    pub position: u32,
}

#[derive(Component)]
#[require(TrackProgress)]
pub struct TrackPosition;

fn compute_race_position(
    mut commands: Commands,
    mut progress: Query<(Entity, &Progress, &mut LapsCounter)>,
) {
    for (entity, progress, mut laps_counter) in progress.iter_mut() {
        let previous_count = laps_counter.count;
        laps_counter.update(progress.progress);
        if previous_count != laps_counter.count {
            commands.entity(entity).trigger(|e| LapUpdate {
                count: laps_counter.count,
                entity: e,
            });
        }
    }
    let mut sorted_racers: Vec<(Entity, &Progress, &LapsCounter)> = progress.iter().collect();
    sorted_racers.sort_by(
        |(_, a_progress, a_laps_counter), (_, b_progress, b_laps_counter)| {
            let a_value = a_progress.progress + a_laps_counter.count as f32 * 10.;
            let b_value = b_progress.progress + b_laps_counter.count as f32 * 10.;
            b_value.partial_cmp(&a_value).unwrap()
        },
    );
    for (index, (entity, _, _)) in sorted_racers.iter().enumerate() {
        commands.entity(*entity).insert(RacePosition {
            position: index as u32,
        });
    }
}
