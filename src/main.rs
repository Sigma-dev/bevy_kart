use avian2d::interpolation::PhysicsInterpolationPlugin;
use avian2d::prelude::*;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use audio_manager::AudioManagerPlugin;
use bevy_ensemble::prelude::*;
use bevy_ensemble_webrtc::BevyEnsembleWebrtcPlugin;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;
use bevy_ticked_networking_ensemble::{TickedEnsembleSessionPlugin, TickedNetworkingEnsemblePlugin};
use bevy_timer::TimerPlugin;

pub mod assets;
pub mod bevy_plugins;
pub mod car_controller_2d;
pub mod debug;
pub mod entity_spawn;
pub mod items;
pub mod kart;
pub mod lobby;
pub mod menu;
pub mod networking;
pub mod rollback_smoothing;
pub mod scene_util;
pub mod theme;
pub mod track;
pub mod wire_format;

pub use assets::AssetHandles;
pub use networking::*;
pub use rollback_smoothing::{ApplyCorrectionSet, CorrectionSmoothing};
pub use theme::*;
pub use track::FinishTimes;

use bevy_plugins::NecessaryBevyPlugins;
use car_controller_2d::CarController2dPlugin;
use debug::DebugPlugin;
use entity_spawn::EntitySpawnPlugin;
use items::ItemsPlugin;
use kart::KartPlugin;
use lobby::LobbyLifecyclePlugin;
use menu::MenuPlugin;
use menu::lobby::spawn_lobby;
use menu::start::spawn_menu;
use rollback_smoothing::RollbackSmoothingPlugin;
use track::{TrackPlugin, spawn_track};


/// Register every networked component. **This order is a wire format.**
///
/// Indices are assigned by position as `u16` and travel in every snapshot, so
/// reordering two lines makes a peer read one component's bytes as another's,
/// silently. Append only, never reorder, never delete.
///
/// The string is what `TickedComponentRegistry::wire_hash()` hashes, and the
/// handshake in `TickedEnsembleSessionPlugin` compares it between every pair of
/// peers at the join. Renaming the Rust type is free; changing one of these
/// strings is a wire break.
///
/// A free function rather than an inline chain so the golden test in
/// `wire_format` can build a registry without building an app.
pub fn register_networked_components(app: &mut App) {
    app
        .register_networked_ticked_component_as::<Position>("Position")
        .register_networked_ticked_component_as::<Rotation>("Rotation")
        .register_networked_ticked_component_as::<LinearVelocity>("LinearVelocity")
        .register_networked_ticked_component_as::<AngularVelocity>("AngularVelocity")
        .register_networked_ticked_component_as::<OwnerPlayer>("OwnerPlayer")
        .register_networked_ticked_component_as::<EntityKind>("EntityKind")
        // Retired (see `networking.rs`), kept so the indices after them hold.
        .register_networked_ticked_component_as::<NetworkedPosition>("NetworkedPosition")
        .register_networked_ticked_component_as::<NetworkedRotation>("NetworkedRotation")
        .register_networked_ticked_component_as::<car_controller_2d::CarControllerInputs>("CarControllerInputs")
        .register_networked_ticked_component_as::<car_controller_2d::SteeringState>("SteeringState")
        .register_networked_ticked_component_as::<items::HeldItem>("HeldItem")
        .register_networked_ticked_component_as::<car_controller_2d::BoostEffect>("BoostEffect")
        .register_networked_ticked_component_as::<car_controller_2d::CarControllerDisabled>("CarControllerDisabled")
        // Rollback-only: a peer's own view of a rocket hit, never sent. Still an
        // entry in the registry, so it is part of the hash and of this order.
        .register_ticked_component_as::<items::RocketHit>("RocketHit");
}

fn main() {
    let mut app = App::new();
    app.add_plugins(NecessaryBevyPlugins)
        // Networking stack
        .add_plugins((
            EnsemblePlugin,
            LobbyBroadcastPlugin,
            PlayerDataPlugin::<AppPlayerData>::default(),
        ))
        .add_plugins(BevyEnsembleWebrtcPlugin {
            server_url: signalling_server_url(),
            display_name: "Player".into(),
            ..default()
        })
        // Ticks come from the crate's own accumulator rather than FixedUpdate,
        // so the client can steer its prediction lead by dilating the tick rate
        // instead of adding or dropping whole ticks.
        .add_plugins(TickedPlugin {
            source: TickSource::Hz(64.0),
            ..default()
        })
        .add_plugins(
            PhysicsPlugins::new(TickedSimulation)
                .set(PhysicsInterpolationPlugin::interpolate_all()),
        )
        .insert_resource(Gravity::ZERO)
        // `Position` and `Rotation` are the simulation's truth and `Transform` is
        // a view of them. The kart smoothing writes an interpolated `Transform`
        // every frame, and with this sync on avian copied it back into
        // `Position` at the start of every tick, dragging every body back by the
        // interpolation lag: 30% speed loss in a 16-tick sawtooth. Everything
        // that moves a body writes `Position` directly.
        .insert_resource(avian2d::physics_transform::PhysicsTransformConfig {
            transform_to_position: false,
            ..default()
        })
        .add_plugins(TickedServerPlugin::<PlayerInput>::new())
        .add_plugins(TickedClientPlugin::<PlayerInput>::new())
        .add_plugins(TickedNetworkingEnsemblePlugin::<PlayerInput>::new())
        // Adopts the host or client role from the ensemble lobby, releases it
        // when the lobby goes, stops a lone host serialising snapshots for
        // nobody, and exchanges the registry hashes at the join so two builds
        // that disagree about the wire format end the session instead of
        // playing it out. `lobby.rs` keeps only the menu's side of all that.
        .add_plugins(TickedEnsembleSessionPlugin)
        // Register ensemble messages
        .register_broadcast_message::<ChatMessage>()
        .register_broadcast_message::<track::OnFinishTimeUpdate>()
        .register_broadcast_message::<GameStateChanged>()
        // Game plugins
        .add_plugins((
            CarController2dPlugin,
            RollbackSmoothingPlugin,
            EntitySpawnPlugin,
            LobbyLifecyclePlugin,
            DebugPlugin,
            AudioManagerPlugin {
                volume_mult: 0.3,
                ..default()
            },
            TimerPlugin,
        ))
        .add_plugins((MenuPlugin, TrackPlugin, ItemsPlugin, KartPlugin))
        // States & resources
        .init_state::<AppState>()
        .init_state::<LobbyState>()
        .init_resource::<LocalPlayerData>()
        .insert_resource(FinishTimes {
            times: HashMap::new(),
        })
        // Assets
        .add_plugins(assets::load_assets)
        // Startup & state transitions
        .add_systems(Startup, setup)
        .add_systems(OnEnter(LobbyState::OutOfLobby), spawn_menu)
        .add_systems(OnEnter(LobbyState::InLobby), spawn_lobby)
        .add_systems(OnEnter(AppState::Game), spawn_track)
        .add_systems(OnExit(AppState::Game), spawn_lobby)
        .insert_resource(ClearColor(AppColors::Grass.color()));

    // The wire format. Kept in a function of its own, and guarded by a golden test,
    // because the order of those calls is a protocol rather than a style choice.
    register_networked_components(&mut app);

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

fn setup(mut commands: Commands) {
    let mut projection = OrthographicProjection::default_2d();
    projection.scaling_mode = bevy::camera::ScalingMode::Fixed {
        width: RESOLUTION.x,
        height: RESOLUTION.y,
    };
    commands.spawn((Camera2d, Projection::Orthographic(projection)));
}
