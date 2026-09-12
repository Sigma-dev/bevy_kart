use avian2d::prelude::*;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use audio_manager::AudioManagerPlugin;
use bevy_ensemble::prelude::*;
use bevy_ensemble_webrtc::BevyEnsembleWebrtcPlugin;
use bevy_ticked::prelude::*;
use bevy_ticked_avian::avian2d::TickedAvianPlugin;
use bevy_ticked_networking::prelude::*;
use bevy_ticked_networking_ensemble::{
    TickedEnsembleSessionPlugin, TickedNetworkingEnsemblePlugin,
};
use bevy_timer::TimerPlugin;

pub mod assets;
pub mod bevy_plugins;
pub mod camera;
pub mod car_controller_2d;
pub mod debug;
pub mod decor;
pub mod editor;
pub mod entity_spawn;
pub mod hud;
pub mod input;
pub mod items;
pub mod kart;
pub mod lobby;
pub mod map_sync;
pub mod menu;
pub mod networking;
pub mod scene_util;
pub mod screen;
pub mod theme;
pub mod track;

pub use assets::AssetHandles;
pub use networking::*;
pub use screen::{EditorState, Screen};
pub use theme::*;
pub use track::FinishTimes;

use bevy_plugins::NecessaryBevyPlugins;
use camera::CameraPlugin;
use car_controller_2d::CarController2dPlugin;
use debug::DebugPlugin;
use entity_spawn::EntitySpawnPlugin;
use items::ItemsPlugin;
use kart::KartPlugin;
use lobby::LobbyLifecyclePlugin;
use menu::MenuPlugin;
use menu::lobby::spawn_lobby;
use menu::start::spawn_menu;
use track::{
    TrackPlugin, build_current_map, grid::spawn_starting_grid, spawn::spawn_map, start_countdown,
};

/// Every networked component of the game's own, each under its wire name.
///
/// The name is the type's identity on the wire. A component's index in a
/// snapshot is its rank among every registered name, sorted, so the order of
/// these lines is not a format, and the join handshake names the first
/// registration two builds disagree on rather than letting them play it out.
/// Renaming a Rust type is free; changing one of these strings is a wire break.
///
/// Not here: avian's four body components, which `TickedAvianPlugin` registers
/// under `avian::*`, and `Owner`, which the role plugins register.
///
/// A free function rather than an inline chain so the headless test in `items`
/// can register the same set without building the whole app.
pub fn register_networked_components(app: &mut App) {
    // What an entity is never changes, so it travels with the entity's first
    // record and in keyframes, never in a delta.
    app.register_networked_ticked_component_once::<EntityKind>("EntityKind")
        .register_networked_ticked_component::<car_controller_2d::CarControllerInputs>(
            "CarControllerInputs",
        )
        .register_networked_ticked_component::<car_controller_2d::SteeringState>("SteeringState")
        .register_networked_ticked_component::<items::HeldItem>("HeldItem")
        .register_networked_ticked_component::<car_controller_2d::BoostEffect>("BoostEffect")
        .register_networked_ticked_component::<car_controller_2d::CarControllerDisabled>(
            "CarControllerDisabled",
        )
        // Rollback-only: a peer's own view of a rocket hit, never sent.
        .register_ticked_component_as::<items::RocketHit>("RocketHit");
}

/// Every broadcast message, each under its wire name. As with the components,
/// the name is the identity and the order is nothing; the transport's own
/// handshake compares the sorted lists at the join.
pub fn register_broadcast_messages(app: &mut App) {
    app.register_broadcast_message::<ChatMessage>("ChatMessage")
        .register_broadcast_message::<track::OnFinishTimeUpdate>("OnFinishTimeUpdate")
        .register_broadcast_message::<GameStateChanged>("GameStateChanged")
        .register_broadcast_message::<map_sync::MapSelected>("MapSelected")
        .register_broadcast_message::<map_sync::StartRace>("StartRace");
}

fn main() {
    let signalling_url = signalling_server_url();
    let mut app = App::new();
    app.add_plugins(NecessaryBevyPlugins)
        // Networking stack
        .add_plugins((
            EnsemblePlugin,
            LobbyBroadcastPlugin,
            PlayerDataPlugin::<AppPlayerData>::default(),
        ))
        .add_plugins(BevyEnsembleWebrtcPlugin {
            server_url: signalling_url.clone(),
            display_name: "Player".into(),
            // STUN, plus a relay when this build was given one, and neither when signalling is
            // loopback. Decided upstream so every game makes the call the same way.
            ice_servers: bevy_ensemble_webrtc::ice_servers_from_env!(&signalling_url),
            ..default()
        })
        // Ticks come from the crate's own accumulator rather than FixedUpdate,
        // so the client can steer its prediction lead by dilating the tick rate
        // instead of adding or dropping whole ticks. The networking plugins
        // refuse `FixedUpdate` outright.
        .add_plugins(TickedPlugin {
            source: TickSource::Hz(64.0),
            ..default()
        })
        // avian2d on the tick, replay-safe: the four body components on the
        // wire under `avian::*`, sleeping off, the solver's contact graph
        // rolled back with the bodies, and avian's `Transform` -> `Position`
        // sync off so the blended transform the renderer sees is never read
        // back as the body's place. Everything that moves a body writes
        // `Position`.
        //
        // Warm starting kept, explicitly. The bundle used to zero it, and a kart
        // driven into a wall was then held there: two seconds of reverse at
        // exactly zero speed. The rolled-back contact graph carries the impulses
        // warm starting seeds from, so a replay is unaffected. The default flips
        // in Sigma-studios/bevy_ticked#15; until that is merged and the pin
        // bumped, this call is what makes the difference, and after, a no-op
        // that can go.
        .add_plugins(TickedAvianPlugin::default().keep_warm_starting())
        .insert_resource(Gravity::ZERO)
        .add_plugins(TickedServerPlugin::<PlayerInput>::new())
        .add_plugins(TickedClientPlugin::<PlayerInput>::new())
        .add_plugins(TickedNetworkingEnsemblePlugin::<PlayerInput>::new())
        // Adopts the host or client role from the ensemble lobby, releases it
        // when the lobby goes, exchanges the registries at the join so two
        // builds that disagree about the wire format end the session instead
        // of playing it out, and hands each client its spawner slot.
        // `lobby.rs` keeps only the menu's side of all that.
        .add_plugins(TickedEnsembleSessionPlugin::default())
        // Not on focus loss. A desktop window that is not in front still renders
        // and ticks at full rate, and with two copies of the game on one screen
        // -- the local session script, or a friend on the same machine -- exactly
        // one window is in front, so the focus pause froze every session the
        // moment the second window opened. A host that really stalls (a browser
        // tab in the background gets a frame a second) still pauses, from the
        // gap between its frames.
        .insert_resource(PausePolicy {
            auto_pause_on_focus_loss: false,
            ..default()
        })
        // The renderer blends every tracked body between its last two tick
        // states, and a correction to a predicted body slides into place
        // instead of blinking there. Neither reaches the simulation.
        .add_plugins((TickedInterpolationPlugin, TickedSmoothingPlugin))
        // The local player's input, sampled once per tick inside the loop and
        // filed for the tick about to run: a keypress costs no extra frame,
        // and a frame that runs two ticks samples twice.
        .add_plugins(TickedInputPlugin::<PlayerInput>::new(
            input::sample_local_input,
        ))
        // Game plugins
        .add_plugins((
            CarController2dPlugin,
            EntitySpawnPlugin,
            LobbyLifecyclePlugin,
            DebugPlugin,
            AudioManagerPlugin { volume_mult: 0.3 },
            TimerPlugin,
        ))
        .add_plugins((
            MenuPlugin,
            TrackPlugin,
            ItemsPlugin,
            KartPlugin,
            CameraPlugin,
            map_sync::MapSyncPlugin,
            editor::EditorPlugin,
        ))
        // States & resources
        .init_state::<AppState>()
        .init_state::<LobbyState>()
        .init_state::<EditorState>()
        .init_resource::<track::SelectedMap>()
        .init_resource::<menu::map_picker::ListedTracks>()
        .init_resource::<menu::map_picker::PreviewOutline>()
        // `Screen` is the cross-product of the three above, named. Everything that
        // used to test two states at once tests this instead.
        .add_computed_state::<Screen>()
        .init_resource::<LocalPlayerData>()
        .insert_resource(FinishTimes {
            times: HashMap::new(),
        })
        // Assets
        .add_plugins(assets::load_assets)
        // Startup & state transitions
        // One transition per screen. `spawn_lobby` used to be registered on both
        // `OnEnter(LobbyState::InLobby)` and `OnExit(AppState::Game)`, because
        // neither alone meant "the lobby screen"; `Screen::Lobby` means it once.
        .add_systems(OnEnter(Screen::StartMenu), spawn_menu)
        .add_systems(OnEnter(Screen::Lobby), spawn_lobby)
        // Chained, and the first is exclusive so the built track is in the world
        // before anything reads it. Geometry, then the karts on it, then the HUD
        // over the top -- three jobs that used to be one 250-line function.
        .add_systems(
            OnEnter(Screen::Race),
            (
                build_current_map,
                camera::set_camera_bounds,
                spawn_map,
                spawn_starting_grid,
                track::minimap::spawn_minimap,
                hud::spawn_race_hud,
                start_countdown,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                map_sync::bail_out_of_a_race_with_no_track,
                track::minimap::track_minimap_viewport,
                track::minimap::spawn_minimap_blips,
                #[cfg(debug_assertions)]
                decor::decor_never_reaches_the_simulation,
            ),
        )
        // Self-clearing, so leaving the editor by any route -- the back button, or
        // a lobby appearing underneath it -- cannot bounce straight back in.
        .add_systems(
            OnExit(Screen::Editor),
            |mut next: ResMut<NextState<EditorState>>| next.set(EditorState::Closed),
        )
        .insert_resource(ClearColor(AppColors::Grass.color()));

    // The wire: both registries are frozen the first time a snapshot or a
    // handshake reads them, so everything is registered before the app runs.
    register_networked_components(&mut app);
    register_broadcast_messages(&mut app);

    // Dev-only network debug overlay + condition simulator (F3 toggles the panel).
    #[cfg(feature = "netdebug")]
    app.add_plugins(NetDebugPlugin::default());

    app.run();
}

/// Where the signalling server is.
///
/// The public one unless `SIGNALLING_SERVER_URL` says otherwise, at build time
/// or, on native, at launch. The launch-time read is what lets
/// `scripts/local-session.sh` point a whole session at a server on this
/// machine without a rebuild.
fn signalling_server_url() -> String {
    std::env::var("SIGNALLING_SERVER_URL")
        .ok()
        .or_else(|| option_env!("SIGNALLING_SERVER_URL").map(String::from))
        .unwrap_or_else(|| "wss://signal.sigma-dev.eu/ws".into())
}
