use bevy::platform::collections::HashMap;

use crate::{
    AppState,
    car_controller_2d::CarControllerDisabled,
    hud::{update_held_item_icon, update_position_ui},
    kart::LapsCounter,
    track::map::build::BuildLevel,
    track::map::builtin::default_map,
    track::position::RacePositionPlugin,
};
use audio_manager::prelude::*;
use bevy::prelude::*;
use bevy_ensemble::prelude::*;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;
use serde::{Deserialize, Serialize};

pub mod grid;
pub mod map;
pub mod minimap;
pub(crate) mod position;
pub mod spawn;

pub struct TrackPlugin;

impl Plugin for TrackPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RacePositionPlugin);
        app.add_systems(
            Update,
            (
                on_receive_finish_times,
                handle_end_race.run_if(in_state(AppState::Game)),
                end_with_delay,
                start_light,
                update_held_item_icon,
                update_position_ui,
            ),
        );
    }
}

pub const LAPS_TO_WIN: u32 = 3;

#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
pub struct FinishTimes {
    pub times: HashMap<u128, u64>,
}

impl FinishTimes {
    pub fn get_player_rank(&self, player_uuid: u128) -> Option<usize> {
        let mut all_times = self
            .times
            .iter()
            .map(|(id, time)| (*id, *time))
            .collect::<Vec<_>>();
        all_times.sort_by(|(_, time), (_, time2)| time.cmp(time2));
        all_times
            .iter()
            .position(|(id, _)| *id == player_uuid)
            .map(|index| index + 1)
    }
}

/// Ensemble message: finish times update.
#[derive(Clone, Debug, Serialize, Deserialize, Message)]
pub struct OnFinishTimeUpdate(pub FinishTimes);


#[derive(Resource)]
struct RaceEnded(f32);

#[derive(Component)]
pub(crate) struct StartLight;

#[derive(Resource)]
struct RaceStarted(u64);


fn on_receive_finish_times(
    mut commands: Commands,
    mut reader: MessageReader<ReceivedEnsembleMessage<OnFinishTimeUpdate>>,
) {
    for msg in reader.read() {
        commands.insert_resource(msg.message.0.clone());
    }
}

fn handle_end_race(
    time: Res<Time>,
    mut commands: Commands,
    input: Res<ButtonInput<KeyCode>>,
    server_player: Option<Res<LocalServerPlayer>>,
    cars: Query<&LapsCounter>,
    lobbies: Query<Entity, With<Lobby>>,
    finish_times: Res<FinishTimes>,
    race_ended: Option<Res<RaceEnded>>,
) {
    if server_player.is_none() {
        return;
    }
    let car_counts: Vec<i32> = cars.iter().map(|c| c.count).collect();
    let race_not_over =
        car_counts.is_empty() || car_counts.iter().any(|&c| c < LAPS_TO_WIN as i32);
    let cheat = input.pressed(KeyCode::KeyU) && input.pressed(KeyCode::KeyK);
    if race_not_over && !cheat {
        return;
    }
    if race_ended.is_some() {
        return;
    }
    info!("Race ended! car_counts={:?}, cheat={}", car_counts, cheat);
    if let Some(lobby) = lobbies.iter().next() {
        let msg = OnFinishTimeUpdate(finish_times.clone());
        commands
            .entity(lobby)
            .trigger(move |entity| BroadcastLobbyMessage::new(entity, msg));
    }
    commands.insert_resource(RaceEnded(time.elapsed_secs()));
}

fn end_with_delay(
    mut commands: Commands,
    time: Res<Time>,
    race_ended: Option<Res<RaceEnded>>,
    server_player: Option<Res<LocalServerPlayer>>,
    mut next_state: ResMut<NextState<AppState>>,
    lobbies: Query<Entity, With<Lobby>>,
) {
    if server_player.is_none() {
        return;
    }
    let Some(race_ended) = race_ended else {
        return;
    };
    let elapsed = time.elapsed_secs() - race_ended.0;
    if elapsed < 3. {
        return;
    }
    info!("end_with_delay: transitioning to OutOfGame (waited {:.1}s)", elapsed);
    next_state.set(AppState::OutOfGame);
    if let Some(lobby) = lobbies.iter().next() {
        let msg = crate::GameStateChanged(crate::AppState::OutOfGame);
        commands
            .entity(lobby)
            .trigger(move |e| BroadcastLobbyMessage::new(e, msg));
    }
    commands.remove_resource::<RaceEnded>();
}

fn start_light(
    mut commands: Commands,
    tick: Res<CurrentTick>,
    mut lights: Query<&mut Sprite, With<StartLight>>,
    race_started: Option<Res<RaceStarted>>,
    disabled_cars: Query<Entity, With<CarControllerDisabled>>,
) {
    let Some(race) = race_started else {
        return;
    };
    let ticks_elapsed = tick.0.saturating_sub(race.0);
    let seconds_elapsed = ticks_elapsed as f32 * SECONDS_PER_TICK;
    for mut light in lights.iter_mut() {
        let Some(texture_atlas) = &mut light.texture_atlas else {
            continue;
        };
        let new_index = seconds_elapsed.floor() as usize + 1;
        if new_index > 4 {
            continue;
        }
        texture_atlas.index = new_index;
    }
    if seconds_elapsed > 3. && seconds_elapsed < 4. {
        for entity in disabled_cars.iter() {
            commands.entity(entity).remove::<CarControllerDisabled>();
        }
    }
}

/// Build the track for the race about to start, before anything spawns from it.
///
/// An exclusive system on purpose. The systems after it in the chain take
/// `Res<BuiltTrack>`, and `Commands::insert_resource` would not be applied until
/// the next sync point -- so this writes the resource into the world directly,
/// and the question of whether the chain inserts one in the right place does not
/// arise.
pub(crate) fn build_current_map(world: &mut World) {
    let map = world
        .get_resource::<SelectedMap>()
        .map(|selected| selected.0.clone())
        .unwrap_or_else(|| {
            // Nothing chose a map. Not fatal -- race the default and say so,
            // because an empty world is far harder to diagnose than a line of log.
            warn!("entering a race with no map selected; falling back to the default");
            default_map()
        });
    let built = map::build(&map, BuildLevel::Full);
    info!(
        "racing `{}`: {} nodes, {:.0} units round, bounds {:?}",
        map.name,
        map.nodes.len(),
        built.length,
        built.bounds.size(),
    );
    world.insert_resource(built);
}

/// The map this peer will race. Chosen in the lobby; defaulted at startup so a
/// session started by a script, or a headless test, always has one.
#[derive(Resource, Clone, Debug)]
pub struct SelectedMap(pub map::MapData);

impl Default for SelectedMap {
    fn default() -> Self {
        Self(default_map())
    }
}

/// Reset the race's own bookkeeping. Everything the track *is* comes from
/// [`build_current_map`]; this is only what a fresh race forgets.
pub(crate) fn start_countdown(
    mut commands: Commands,
    tick: Res<CurrentTick>,
    mut finish_times: ResMut<FinishTimes>,
    mut audio_manager: AudioManager,
) {
    finish_times.times.clear();
    commands.remove_resource::<RaceEnded>();
    audio_manager.play_sound(PlayAudio2D::new_once("sounds/countdown.wav"));
    commands.insert_resource(RaceStarted(tick.0));
}
