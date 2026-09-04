use bevy::prelude::*;
use bevy_ticked::prelude::*;

use crate::{
    kart::{LapUpdate, LapsCounter},
    track::{
        LAPS_TO_WIN,
        position::progress_line::{Progress, TrackProgress},
    },
};

pub struct RacePositionPlugin;

pub mod progress_line;

impl Plugin for RacePositionPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(progress_line::ProgressLinePlugin)
            .add_systems(TickedSimulation, compute_race_position);
    }
}

/// Where a kart stands in the field, zero-based.
#[derive(Component, Debug)]
pub struct RacePosition {
    pub position: u32,
    /// Set once the kart has crossed the finish line on its last lap. From then
    /// on `position` is its result and stops following the field: a rival that
    /// coasts further past the line, or a shove back over it, does not move it.
    pub is_final: bool,
}

#[derive(Component)]
#[require(TrackProgress)]
pub struct TrackPosition;

/// Orders the racers by laps, then progress along the line, and hands out the
/// positions. Finished karts keep the position they crossed with; the karts still
/// racing are sorted among themselves and fill the slots behind them.
fn compute_race_position(
    mut commands: Commands,
    mut racers: Query<(Entity, &Progress, &mut LapsCounter, Option<&RacePosition>)>,
) {
    for (entity, progress, mut laps_counter, _) in racers.iter_mut() {
        let previous_count = laps_counter.count;
        laps_counter.update(progress.progress);
        if previous_count != laps_counter.count {
            commands.entity(entity).trigger(|e| LapUpdate {
                count: laps_counter.count,
                entity: e,
            });
        }
    }
    let (finished, mut racing): (Vec<_>, Vec<_>) = racers
        .iter()
        .partition(|(_, _, _, position)| position.is_some_and(|p| p.is_final));
    racing.sort_by(
        |(_, a_progress, a_laps_counter, _), (_, b_progress, b_laps_counter, _)| {
            let a_value = a_progress.progress + a_laps_counter.count as f32 * 10.;
            let b_value = b_progress.progress + b_laps_counter.count as f32 * 10.;
            b_value.partial_cmp(&a_value).unwrap()
        },
    );
    // A kart crossing its last line this tick outranks everyone still racing, so
    // it lands right behind the earlier finishers and that is where it stays.
    for (index, (entity, _, laps_counter, _)) in racing.iter().enumerate() {
        commands.entity(*entity).insert(RacePosition {
            position: (finished.len() + index) as u32,
            is_final: laps_counter.count >= LAPS_TO_WIN as i32,
        });
    }
}

#[cfg(test)]
mod tests {
    //! The ordering on its own: entities with a progress and a lap count, the
    //! system run by hand, progress moved between runs to stand in for driving.
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    const LAST_LAP: i32 = LAPS_TO_WIN as i32 - 1;

    fn racer(world: &mut World, progress: f32, laps: i32) -> Entity {
        world
            .spawn((
                Progress { progress },
                LapsCounter {
                    count: laps,
                    last_frame_progress: progress,
                },
            ))
            .id()
    }

    fn drive_to(world: &mut World, racer: Entity, progress: f32) {
        world.get_mut::<Progress>(racer).unwrap().progress = progress;
        world.run_system_once(compute_race_position).unwrap();
    }

    fn positions(world: &World, racers: &[Entity]) -> Vec<u32> {
        racers
            .iter()
            .map(|e| world.get::<RacePosition>(*e).unwrap().position)
            .collect()
    }

    fn is_final(world: &World, racer: Entity) -> bool {
        world.get::<RacePosition>(racer).unwrap().is_final
    }

    #[test]
    fn a_finisher_keeps_first_place_when_the_next_one_coasts_further_past_the_line() {
        let mut world = World::new();
        let a = racer(&mut world, 0.98, LAST_LAP);
        let b = racer(&mut world, 0.90, LAST_LAP);
        world.run_system_once(compute_race_position).unwrap();
        assert_eq!(positions(&world, &[a, b]), [0, 1]);
        assert!(!is_final(&world, a), "still racing until the line");

        // `a` crosses and stops almost on the line.
        drive_to(&mut world, a, 0.01);
        assert_eq!(positions(&world, &[a, b]), [0, 1]);
        assert!(
            is_final(&world, a),
            "crossing the last line decides the position"
        );

        // `b` crosses next and rolls on further than `a` did.
        drive_to(&mut world, b, 0.98);
        drive_to(&mut world, b, 0.04);
        assert_eq!(
            positions(&world, &[a, b]),
            [0, 1],
            "the finish order stands"
        );
        assert!(is_final(&world, b));
        drive_to(&mut world, b, 0.20);
        assert_eq!(positions(&world, &[a, b]), [0, 1]);
    }

    #[test]
    fn a_finisher_shoved_back_over_the_line_keeps_its_position() {
        let mut world = World::new();
        let a = racer(&mut world, 0.97, LAST_LAP);
        let b = racer(&mut world, 0.60, LAST_LAP);
        drive_to(&mut world, a, 0.02);
        assert_eq!(positions(&world, &[a, b]), [0, 1]);

        // Knocked backwards across the line: the lap count drops, the position does not.
        drive_to(&mut world, a, 0.96);
        drive_to(&mut world, b, 0.98);
        assert_eq!(world.get::<LapsCounter>(a).unwrap().count, LAST_LAP);
        assert_eq!(positions(&world, &[a, b]), [0, 1]);

        // And `b` finishing takes the slot behind `a`, not the lead.
        drive_to(&mut world, b, 0.03);
        assert_eq!(positions(&world, &[a, b]), [0, 1]);
        assert!(is_final(&world, b));
    }

    #[test]
    fn the_karts_still_racing_swap_places_behind_the_finishers() {
        let mut world = World::new();
        let a = racer(&mut world, 0.99, LAST_LAP);
        let b = racer(&mut world, 0.50, 1);
        let c = racer(&mut world, 0.40, 1);
        drive_to(&mut world, a, 0.01);
        assert_eq!(positions(&world, &[a, b, c]), [0, 1, 2]);

        drive_to(&mut world, c, 0.55);
        assert_eq!(positions(&world, &[a, b, c]), [0, 2, 1]);
        assert!(!is_final(&world, b));
        assert!(!is_final(&world, c));
    }
}
