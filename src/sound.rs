//! The music, and the karts' engines.
//!
//! Both are Orchestre files: `.orch` songs and an `.orsfx` sound, synthesized as they play. The
//! songs are run-2d's -- the menu's plays on the start menu and in the lobby, the round's for the
//! length of a race. The engine is Orchestre's two-stroke preset (a real kart is a single-cylinder
//! two-stroke), one per kart, with its RPM following the kart.
//!
//! # RPM
//!
//! A kart here has no top speed: two powered wheels push it at a steady 32 px/s² for as long as
//! the throttle is held, and only the track's corners stop it. So the RPM is the speed, scaled to
//! reach the redline at [`REDLINE_SPEED`] -- about where the corners keep a kart -- and holding
//! there beyond it. On the throttle from a standstill it jumps to [`CLUTCH_RPM`], the way a
//! kart's centrifugal clutch lets the engine rev before the wheels bite; off the throttle it sits
//! a little lower, coasting. The throttle is also the engine's load: louder and harsher pulling
//! than coasting, and harder still on a boost.
//!
//! Everything here is presentation, read in `Update` off the karts as they are drawn. Nothing
//! reaches the simulation.

use avian2d::prelude::LinearVelocity;
use bevy::audio::{AudioSink, AudioSinkPlayback, Volume};
use bevy::prelude::*;
use orchestre_bevy::{OrchestrePlugin, OrchestreSong, OrchestreSound, PlaySfx, SfxVoice};

use crate::Screen;
use crate::car_controller_2d::{BoostEffect, CarController2d, CarControllerInputs};
use crate::kart::LocalKart;
use audio_manager::AudioManagerResource;

const MENU_SONG: &str = "music/menu.orch";
const ROUND_SONG: &str = "music/round.orch";
const ENGINE_SOUND: &str = "sounds/kart-engine.orsfx";

/// Before the game's master volume. The menu's song is the only thing playing there; the race's
/// has engines, items and the countdown to sit under.
const MENU_VOLUME: f32 = 1.0;
const ROUND_VOLUME: f32 = 0.7;
const ENGINE_VOLUME: f32 = 0.8;

/// The RPM control's name in `kart-engine.orsfx`, as made in the editor.
const RPM: &str = "RPM";
/// Ticking over.
const IDLE_RPM: f32 = 1800.0;
/// Where the throttle takes it from a standstill, before the wheels bite.
const CLUTCH_RPM: f32 = 3500.0;
const REDLINE_RPM: f32 = 9000.0;
/// Speed at the redline, px/s.
const REDLINE_SPEED: f32 = 250.0;
/// Coasting revs this much lower than pulling at the same speed.
const COAST: f32 = 0.85;
/// Seconds for the revs to get most of the way to where they are going.
const REV_TIME: f32 = 0.1;
/// Past this far from the camera (px), another kart's engine is as quiet as it gets.
const HEARING: f32 = 300.0;
const FAR_VOLUME: f32 = 0.15;

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(OrchestrePlugin)
            .init_resource::<Music>()
            .add_systems(Startup, load_engine)
            .add_systems(OnEnter(Screen::StartMenu), play_menu_music)
            .add_systems(OnEnter(Screen::Lobby), play_menu_music)
            .add_systems(OnEnter(Screen::Editor), stop::<MenuMusic>)
            .add_systems(OnEnter(Screen::Race), (stop::<MenuMusic>, play_round_music))
            .add_systems(OnExit(Screen::Race), (stop::<RoundMusic>, stop::<Engine>))
            .add_systems(
                Update,
                engines
                    .run_if(in_state(Screen::Race))
                    .run_if(resource_exists::<EngineSound>),
            );
    }
}

/// The songs, loaded once and held for the run: the menu is a screen you come back to.
///
/// Loaded as the plugin is built rather than in `Startup`: the start menu is the screen the game
/// boots into, and its `OnEnter` runs before `PreStartup`.
#[derive(Resource)]
struct Music {
    menu: Handle<OrchestreSong>,
    round: Handle<OrchestreSong>,
}

impl FromWorld for Music {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Music {
            menu: assets.load(MENU_SONG),
            round: assets.load(ROUND_SONG),
        }
    }
}

#[derive(Resource)]
struct EngineSound(Handle<OrchestreSound>);

fn load_engine(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(EngineSound(assets.load(ENGINE_SOUND)));
}

#[derive(Component)]
struct MenuMusic;

#[derive(Component)]
struct RoundMusic;

fn master(manager: &Option<Res<AudioManagerResource>>) -> f32 {
    manager.as_ref().map_or(1.0, |m| m.volume_mult())
}

/// `Once` and not `LOOP`, though the songs repeat: an Orchestre song loops inside its own
/// decoder, on the beat, where Bevy's loop would restart it after the reverb tail.
fn song(
    handle: &Handle<OrchestreSong>,
    volume: f32,
) -> (AudioPlayer<OrchestreSong>, PlaybackSettings) {
    (
        AudioPlayer(handle.clone()),
        PlaybackSettings {
            volume: Volume::Linear(volume),
            ..PlaybackSettings::ONCE
        },
    )
}

/// Unless it is already playing: from the start menu into a lobby and back is one stretch of
/// the same song.
fn play_menu_music(
    mut commands: Commands,
    music: Res<Music>,
    manager: Option<Res<AudioManagerResource>>,
    playing: Query<(), With<MenuMusic>>,
) {
    if playing.is_empty() {
        commands.spawn((MenuMusic, song(&music.menu, MENU_VOLUME * master(&manager))));
    }
}

fn play_round_music(
    mut commands: Commands,
    music: Res<Music>,
    manager: Option<Res<AudioManagerResource>>,
) {
    commands.spawn((
        RoundMusic,
        song(&music.round, ROUND_VOLUME * master(&manager)),
    ));
}

fn stop<T: Component>(mut commands: Commands, playing: Query<Entity, With<T>>) {
    for entity in &playing {
        commands.entity(entity).try_despawn();
    }
}

/// A kart's engine: the sound playing for it, and where its revs have got to.
#[derive(Component)]
struct Engine {
    kart: Entity,
    rpm: f32,
}

/// Where a kart's revs are headed: the speed on the RPM scale, never below the clutch while the
/// throttle is down, a little under it coasting.
fn target_rpm(speed: f32, throttle: bool) -> f32 {
    let by_speed = IDLE_RPM + speed.abs() / REDLINE_SPEED * (REDLINE_RPM - IDLE_RPM);
    let rpm = if throttle {
        by_speed.max(CLUTCH_RPM)
    } else {
        IDLE_RPM.max(by_speed * COAST)
    };
    rpm.min(REDLINE_RPM)
}

/// One engine per kart in the race, following its speed, throttle and boost, and quieter the
/// further it is from the camera -- except your own, which is always as close as you are.
fn engines(
    mut commands: Commands,
    time: Res<Time>,
    sound: Res<EngineSound>,
    manager: Option<Res<AudioManagerResource>>,
    karts: Query<
        (
            Entity,
            &GlobalTransform,
            &LinearVelocity,
            &CarControllerInputs,
            Has<BoostEffect>,
            Has<LocalKart>,
        ),
        With<CarController2d>,
    >,
    camera: Query<&GlobalTransform, With<Camera2d>>,
    mut engines: Query<(
        Entity,
        &mut Engine,
        &mut PlaySfx,
        Option<&SfxVoice>,
        Option<&mut AudioSink>,
    )>,
) {
    let heard_from = camera.iter().next().map(|c| c.translation().xy());
    let master = master(&manager);
    let blend = 1.0 - (-time.delta_secs() / REV_TIME).exp();

    for (engine_entity, mut engine, mut play, voice, sink) in &mut engines {
        let Ok((_, at, velocity, inputs, boosted, local)) = karts.get(engine.kart) else {
            // The kart has gone: let the engine die away rather than cut.
            match voice {
                Some(voice) => voice.release(),
                None => commands.entity(engine_entity).try_despawn(),
            }
            commands.entity(engine_entity).remove::<Engine>();
            continue;
        };
        let throttle = inputs.forward || inputs.backward;
        engine.rpm += (target_rpm(velocity.length(), throttle) - engine.rpm) * blend;
        let load = match (throttle, boosted) {
            (_, true) => 1.3,
            (true, false) => 1.0,
            (false, false) => 0.6,
        };
        let now = play
            .controls
            .iter()
            .find(|(name, _)| name == RPM)
            .map(|c| c.1);
        if now.is_none_or(|rpm| (rpm - engine.rpm).abs() > 20.0) || play.intensity != load {
            *play = play.clone().control(RPM, engine.rpm).intensity(load);
        }
        if let Some(mut sink) = sink {
            let near = match (local, heard_from) {
                (false, Some(camera)) => {
                    let d = at.translation().xy().distance(camera);
                    (1.0 - d / HEARING).clamp(FAR_VOLUME, 1.0)
                }
                _ => 1.0,
            };
            sink.set_volume(Volume::Linear(ENGINE_VOLUME * master * near));
        }
    }

    let running: Vec<Entity> = engines.iter().map(|(_, e, ..)| e.kart).collect();
    for (kart, ..) in &karts {
        if !running.contains(&kart) {
            commands.spawn((
                Engine {
                    kart,
                    rpm: IDLE_RPM,
                },
                PlaySfx::new(sound.0.clone())
                    .control(RPM, IDLE_RPM)
                    .volume(ENGINE_VOLUME * master),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::{AssetPlugin, LoadState};
    use orchestre_bevy::{OrchestreLoader, OrchestreSoundLoader};

    use super::*;

    #[test]
    fn the_revs_follow_the_kart() {
        // Standing still, it ticks over; on the throttle, the clutch lets it rev.
        assert_eq!(target_rpm(0.0, false), IDLE_RPM);
        assert_eq!(target_rpm(0.0, true), CLUTCH_RPM);
        // Faster is higher, all the way up.
        let mut last = 0.0;
        for speed in [20.0, 80.0, 150.0, 240.0] {
            let rpm = target_rpm(speed, true);
            assert!(rpm >= last, "{speed} px/s: {rpm} after {last}");
            last = rpm;
        }
        // The redline holds, however fast a boost takes it.
        assert_eq!(target_rpm(REDLINE_SPEED, true), REDLINE_RPM);
        assert_eq!(target_rpm(3.0 * REDLINE_SPEED, true), REDLINE_RPM);
        // Coasting sits lower than pulling, but never under idle.
        assert!(target_rpm(150.0, false) < target_rpm(150.0, true));
        assert!(target_rpm(1.0, false) >= IDLE_RPM);
        // Reversing revs as driving forward does.
        assert_eq!(target_rpm(-100.0, true), target_rpm(100.0, true));
    }

    /// The files named above are on disk, parse and make a sound -- through the real loaders,
    /// without `OrchestrePlugin`, which wants an audio device.
    #[test]
    fn the_songs_and_the_engine_load() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<OrchestreSong>()
            .init_asset::<OrchestreSound>()
            .register_asset_loader(OrchestreLoader)
            .register_asset_loader(OrchestreSoundLoader);
        let server = app.world().resource::<AssetServer>().clone();
        let songs: Vec<Handle<OrchestreSong>> =
            [MENU_SONG, ROUND_SONG].map(|p| server.load(p)).to_vec();
        let engine: Handle<OrchestreSound> = server.load(ENGINE_SOUND);
        let ids: Vec<_> = songs
            .iter()
            .map(|h| h.id().untyped())
            .chain([engine.id().untyped()])
            .collect();
        for _ in 0..1000 {
            app.update();
            if ids.iter().all(|id| server.load_state(*id).is_loaded()) {
                break;
            }
        }
        for id in &ids {
            match server.load_state(*id) {
                LoadState::Loaded => {}
                LoadState::Failed(err) => panic!("could not load {id:?}: {err}"),
                state => panic!("{id:?} never finished loading: {state:?}"),
            }
        }
        let sounds = app.world().resource::<Assets<OrchestreSound>>();
        let engine = sounds.get(&engine).unwrap().sound();
        assert!(
            engine.controls.iter().any(|c| c.name == RPM),
            "the engine has no {RPM} control to follow the kart with"
        );
        assert!(engine.max_voices >= 8, "not enough engines for a full grid");
    }
}
