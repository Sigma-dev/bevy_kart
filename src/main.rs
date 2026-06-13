use avian2d::interpolation::PhysicsInterpolationPlugin;
use avian2d::prelude::*;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use audio_manager::AudioManagerPlugin;
use bevy_ensemble::prelude::*;
use bevy_ensemble_webrtc::BevyEnsembleWebrtcPlugin;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;
use bevy_ticked_networking_ensemble::TickedNetworkingEnsemblePlugin;
use bevy_timer::TimerPlugin;
use bevy_ui_text_input::TextInputPlugin;

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
pub mod theme;
pub mod track;

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

fn main() {
    App::new()
        .add_plugins((NecessaryBevyPlugins, TextInputPlugin))
        // Networking stack
        .add_plugins((
            EnsemblePlugin,
            LobbyBroadcastPlugin,
            PlayerDataPlugin::<AppPlayerData>::default(),
        ))
        .add_plugins(BevyEnsembleWebrtcPlugin {
            server_url: "wss://signal.sigma-dev.eu/ws".into(),
            display_name: "Player".into(),
            ..default()
        })
        .add_plugins(TickedPlugin)
        .add_plugins(
            PhysicsPlugins::new(TickedSimulation)
                .set(PhysicsInterpolationPlugin::interpolate_all()),
        )
        .insert_resource(Gravity::ZERO)
        .add_plugins(TickedServerPlugin::<PlayerInput>::new())
        .add_plugins(TickedClientPlugin::<PlayerInput>::new())
        .add_plugins(TickedNetworkingEnsemblePlugin::<PlayerInput>::new())
        // Register networked components (ORDER MUST MATCH ON ALL PEERS)
        .register_networked_ticked_component::<Position>()
        .register_networked_ticked_component::<Rotation>()
        .register_networked_ticked_component::<LinearVelocity>()
        .register_networked_ticked_component::<AngularVelocity>()
        .register_networked_ticked_component::<OwnerPlayer>()
        .register_networked_ticked_component::<EntityKind>()
        .register_networked_ticked_component::<NetworkedPosition>()
        .register_networked_ticked_component::<NetworkedRotation>()
        .register_networked_ticked_component::<car_controller_2d::CarControllerInputs>()
        .register_networked_ticked_component::<car_controller_2d::SteeringState>()
        .register_networked_ticked_component::<items::HeldItem>()
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
        .insert_resource(ClearColor(AppColors::Grass.color()))
        .run();
}

fn setup(mut commands: Commands) {
    let mut projection = OrthographicProjection::default_2d();
    projection.scaling_mode = bevy::camera::ScalingMode::Fixed {
        width: RESOLUTION.x,
        height: RESOLUTION.y,
    };
    commands.spawn((Camera2d, Projection::Orthographic(projection)));
}
