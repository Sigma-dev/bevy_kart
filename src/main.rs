use crate::items::{ItemType, ItemsPlugin};
use crate::kart::{
    FollowTransform, KART_COLORS_COUNT, KART_SIZE, KartColor, KartPlugin, LapUpdate, LapsCounter,
    LocalKart,
};
use crate::menu::MenuPlugin;
use crate::menu::lobby::{BACKGROUND_ELEMENT_TYPES_COUNT, spawn_lobby};
use crate::menu::start::spawn_menu;
use crate::track::{TrackPlugin, spawn_track};
use audio_manager::AudioManagerPlugin;
use avian2d::prelude::*;
use bevy::a11y::AccessibilityPlugin;
use bevy::app::{PanicHandlerPlugin, TaskPoolPlugin};
use bevy::asset::AssetMetaCheck;
use bevy::audio::AudioPlugin;
use bevy::camera::CameraPlugin;
use bevy::core_pipeline::CorePipelinePlugin;
use bevy::diagnostic::{DiagnosticsPlugin, FrameCountPlugin};
use bevy::gizmos::GizmoPlugin;
use bevy::input::InputPlugin;
use bevy::log::LogPlugin;
use bevy::mesh::MeshPlugin;
use bevy::picking::{InteractionPlugin, PickingPlugin, input::PointerInputPlugin};
use bevy::platform::collections::HashMap;
use bevy::scene::ScenePlugin;
use bevy::prelude::*;
use bevy_bundled_observers::observers;
use bevy::render::RenderPlugin;
use bevy::sprite::SpritePlugin;
use bevy::sprite_render::SpriteRenderPlugin;
use bevy::state::app::StatesPlugin;
use bevy::text::TextPlugin;
use bevy::time::TimePlugin;
use bevy::transform::TransformPlugin;
use bevy::ui::UiPlugin;
use bevy::ui_render::UiRenderPlugin;
use bevy::window::PrimaryWindow;
use bevy::winit::WinitPlugin;
use bevy_ensemble::prelude::*;
use bevy_ensemble_webrtc::{
    BevyEnsembleWebrtcPlugin, JoinWebrtcLobbyByCode, LobbyWebrtcCode,
};
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;
use bevy_ticked_networking_ensemble::TickedNetworkingEnsemblePlugin;
use bevy_ui_text_input::TextInputPlugin;
use serde::{Deserialize, Serialize};

pub mod car_controller_2d;
pub mod items;
pub mod kart;
pub mod menu;
pub mod track;
use bevy_timer::TimerPlugin;
use car_controller_2d::CarController2dPlugin;

const RESOLUTION: Vec2 = Vec2::new(256., 144.);

// --- Networking types ---

/// Input data sent each tick from each player.
/// This is the tick input type for bevy_ticked.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PlayerInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub using_item: bool,
}

/// Networked component: identifies which player owns an entity.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct OwnerPlayer(pub u128);

/// Networked position proxy for non-physics tracked entities (items, rockets).
/// Copied to Transform.translation by sync_non_physics_visuals.
#[derive(Component, Clone, Debug, Serialize, Deserialize, Default)]
pub struct NetworkedPosition(pub Vec2);

/// Networked rotation proxy for non-physics tracked entities (rockets).
/// Copied to Transform.rotation by sync_non_physics_visuals.
#[derive(Component, Clone, Debug, Serialize, Deserialize, Default)]
pub struct NetworkedRotation(pub f32);

/// Networked component: what kind of networked entity this is.
/// Used by clients in On<Add, TickTrackedEntity> observer to add visuals.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub enum EntityKind {
    Kart,
    ItemPickup(ItemType),
    Rocket,
}

/// Player metadata shared via ensemble messages.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Message)]
pub struct AppPlayerData {
    pub name: String,
    pub kart_color: KartColor,
}

impl Default for AppPlayerData {
    fn default() -> Self {
        Self {
            name: "YOUR_NAME".to_string(),
            kart_color: KartColor::new_random(),
        }
    }
}

/// Ensemble message for chat.
#[derive(Clone, Debug, Serialize, Deserialize, Message)]
pub struct ChatMessage {
    pub sender: u128,
    pub text: String,
}

/// Broadcast message: game state changed (host -> all peers).
#[derive(Clone, Debug, Serialize, Deserialize, Message)]
pub struct GameStateChanged(pub AppState);

/// Local player's data (stored locally, pushed via SetPlayerData when in a lobby).
#[derive(Resource)]
pub struct LocalPlayerData(pub AppPlayerData);

impl Default for LocalPlayerData {
    fn default() -> Self {
        Self(AppPlayerData::default())
    }
}

#[derive(States, Default, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    OutOfGame,
    Game,
}

/// Replaces P2PLobbyState from bevy_easy_p2p.
#[derive(States, Default, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LobbyState {
    #[default]
    OutOfLobby,
    InLobby,
}

#[derive(Resource)]
pub struct AssetHandles {
    numbers_texture: Handle<Image>,
    numbers_atlas: Handle<TextureAtlasLayout>,
    menu_background_texture: Handle<Image>,
    logo_texture: Handle<Image>,
    buttons_texture: Handle<Image>,
    buttons_atlas: Handle<TextureAtlasLayout>,
    arrow_texture: Handle<Image>,
    kick_texture: Handle<Image>,
    name_texture: Handle<Image>,
    track_texture: Handle<Image>,
    traffic_light_texture: Handle<Image>,
    karts_texture: Handle<Image>,
    karts_atlas: Handle<TextureAtlasLayout>,
    wheel_texture: Handle<Image>,
    crate_texture: Handle<Image>,
    items_texture: Handle<Image>,
    rocket_texture: Handle<Image>,
    background_elements_texture: Handle<Image>,
    background_elements_atlas: Handle<TextureAtlasLayout>,
    clouds_texture: Handle<Image>,
    clouds_atlas: Handle<TextureAtlasLayout>,
}

pub enum SpriteLayers {
    Background,
    OnGround,
    Wheels,
    Car,
    AboveCar,
}

impl SpriteLayers {
    fn to_z(&self) -> f32 {
        match self {
            SpriteLayers::Background => -100.,
            SpriteLayers::OnGround => -10.,
            SpriteLayers::Wheels => -1.,
            SpriteLayers::Car => 10.,
            SpriteLayers::AboveCar => 100.,
        }
    }
}

pub enum AppColors {
    Dark,
    Road,
    Grass,
}

impl AppColors {
    fn color(&self) -> Color {
        match self {
            AppColors::Dark => Srgba::hex("2e222f").unwrap().into(),
            AppColors::Road => Srgba::hex("323353").unwrap().into(),
            AppColors::Grass => Srgba::hex("239063").unwrap().into(),
        }
    }
}

struct NecessaryBevyPlugins;

impl Plugin for NecessaryBevyPlugins {
    fn build(&self, app: &mut App) {
        // Order matches DefaultPlugins, with unused plugins removed.
        app.add_plugins((
            PanicHandlerPlugin,
            LogPlugin::default(),
            TaskPoolPlugin::default(),
            FrameCountPlugin,
            TimePlugin,
            TransformPlugin,
            DiagnosticsPlugin,
            InputPlugin,
            WindowPlugin::default(),
            AccessibilityPlugin,
        ));
        app.add_plugins((
            AssetPlugin {
                meta_check: AssetMetaCheck::Never,
                ..default()
            },
            ScenePlugin,
            WinitPlugin::default(),
            RenderPlugin::default(),
            ImagePlugin::default_nearest(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
        ));
        app.add_plugins((
            SpritePlugin,
            SpriteRenderPlugin,
            TextPlugin,
            UiPlugin,
            UiRenderPlugin,
            AudioPlugin::default(),
            GizmoPlugin,
            StatesPlugin,
        ));
        // Picking — after UiPlugin, matching DefaultPlugins order
        app.add_plugins((
            PointerInputPlugin,
            PickingPlugin,
            InteractionPlugin,
        ));
    }
}

#[derive(Component)]
#[require(Text)]
struct FpsText {
    pub current: f64,
}

fn main() {
    App::new()
        .add_plugins((
            NecessaryBevyPlugins,
            TextInputPlugin,
        ))
        // Networking stack
        .add_plugins((EnsemblePlugin, LobbyBroadcastPlugin, PlayerDataPlugin::<AppPlayerData>::default()))
        .add_plugins(BevyEnsembleWebrtcPlugin {
            server_url: "wss://signal.sigma-dev.eu/ws".into(),
            display_name: "Player".into(),
            ..default()
        })
        .add_plugins(TickedPlugin)
        .add_plugins(PhysicsPlugins::new(TickedSimulation))
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
        // Register ensemble messages
        .register_broadcast_message::<ChatMessage>()
        .register_broadcast_message::<items::ItemPickedUp>()
        .register_broadcast_message::<items::RocketExploded>()
        .register_broadcast_message::<track::OnFinishTimeUpdate>()
        .register_broadcast_message::<GameStateChanged>()
        // Game plugins
        .add_plugins((
            CarController2dPlugin,
            AudioManagerPlugin::default(),
            TimerPlugin,
        ))
        .add_plugins((MenuPlugin, TrackPlugin, ItemsPlugin, KartPlugin))
        // States
        .init_state::<AppState>()
        .init_state::<LobbyState>()
        // Resources
        .init_resource::<LocalPlayerData>()
        .insert_resource(FinishTimes {
            times: HashMap::new(),
        })
        // Asset loading
        .add_plugins(|app: &mut App| {
            let asset_server = app.world().get_resource::<AssetServer>().unwrap().clone();
            let mut texture_atlases = app
                .world_mut()
                .get_resource_mut::<Assets<TextureAtlasLayout>>()
                .unwrap();
            let asset_handles = AssetHandles {
                numbers_texture: asset_server.load("sprites/numbers.png"),
                numbers_atlas: texture_atlases.add(TextureAtlasLayout::from_grid(
                    UVec2::new(32, 32),
                    10,
                    1,
                    None,
                    None,
                )),
                menu_background_texture: asset_server.load("sprites/menu_background.png"),
                logo_texture: asset_server.load("sprites/logo.png"),
                buttons_texture: asset_server.load("sprites/buttons.png"),
                buttons_atlas: texture_atlases.add(TextureAtlasLayout::from_grid(
                    UVec2::new(64, 16),
                    2,
                    4,
                    None,
                    None,
                )),
                arrow_texture: asset_server.load("sprites/arrow.png"),
                kick_texture: asset_server.load("sprites/kick.png"),
                name_texture: asset_server.load("sprites/name.png"),
                track_texture: asset_server.load("sprites/track.png"),
                traffic_light_texture: asset_server.load("sprites/start_light.png"),
                karts_texture: asset_server.load("sprites/karts.png"),
                karts_atlas: texture_atlases.add(TextureAtlasLayout::from_grid(
                    KART_SIZE,
                    KART_COLORS_COUNT,
                    1,
                    None,
                    None,
                )),
                wheel_texture: asset_server.load("sprites/wheel.png"),
                crate_texture: asset_server.load("sprites/crate.png"),
                items_texture: asset_server.load("sprites/items.png"),
                rocket_texture: asset_server.load("sprites/rocket.png"),
                background_elements_texture: asset_server.load("sprites/nature.png"),
                background_elements_atlas: texture_atlases.add(TextureAtlasLayout::from_grid(
                    UVec2::splat(8),
                    BACKGROUND_ELEMENT_TYPES_COUNT as u32,
                    1,
                    None,
                    None,
                )),
                clouds_texture: asset_server.load("sprites/clouds.png"),
                clouds_atlas: texture_atlases.add(TextureAtlasLayout::from_grid(
                    UVec2::splat(32),
                    2,
                    1,
                    None,
                    None,
                )),
            };
            app.insert_resource(asset_handles);
        })
        // Systems
        .add_systems(Startup, (auto_join_from_url, setup))
        .add_observer(on_tracked_entity_spawned)
        .add_systems(
            Update,
            (
                on_lobby_ready,
                cleanup_on_lobby_gone,
                receive_game_state_changed,
                cursor_positon_log,
                update_fps,
            ),
        )
        .add_systems(Update, (capture_local_input, sync_visuals))
        .add_systems(OnEnter(LobbyState::OutOfLobby), spawn_menu)
        .add_systems(OnEnter(LobbyState::InLobby), spawn_lobby)
        .add_systems(OnEnter(AppState::Game), spawn_track)
        .add_systems(OnExit(AppState::Game), spawn_lobby)
        .insert_resource(ClearColor(AppColors::Grass.color()))
        .run();
}

#[cfg(target_arch = "wasm32")]
fn get_url() -> Option<String> {
    web_sys::window()?.location().href().ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn get_url() -> Option<String> {
    None
}

fn current_base_url() -> Option<String> {
    let source = get_url()?;
    let no_hash = source.split('#').next().unwrap_or(source.as_str());
    let base = no_hash.split('?').next().unwrap_or(no_hash);
    Some(base.trim_end_matches('/').to_string())
}

fn extract_query_param(target: &str) -> Option<String> {
    let href = get_url()?;
    let no_hash = href.split('#').next().unwrap_or(href.as_str());
    let query = no_hash.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        let key = it.next()?;
        if key == target {
            let val = it.next().unwrap_or("");
            return Some(val.to_string());
        }
    }
    None
}

/// Capture keyboard input and write to InputQueue each tick.
fn capture_local_input(
    keys: Res<ButtonInput<KeyCode>>,
    tick: Res<CurrentTick>,
    local_client: Option<Res<LocalClientPlayer>>,
    local_server: Option<Res<LocalServerPlayer>>,
    mut input_queue: ResMut<InputQueue<PlayerInput>>,
) {
    let uuid = local_client
        .as_ref()
        .map(|p| p.0)
        .or_else(|| local_server.as_ref().map(|p| p.0));
    let Some(uuid) = uuid else { return };
    let input = PlayerInput {
        forward: keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp),
        backward: keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown),
        left: keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft),
        right: keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight),
        using_item: keys.pressed(KeyCode::Space),
    };
    input_queue.insert(tick.0 + 1, uuid, input);
}

/// Auto-join lobby from URL query parameter.
fn auto_join_from_url(mut join_writer: MessageWriter<JoinWebrtcLobbyByCode>) {
    if let Some(room) = extract_query_param("room") {
        info!("room code in url: {}", room);
        if !room.trim().is_empty() {
            join_writer.write(JoinWebrtcLobbyByCode(room));
        }
    }
}

/// Detect when a lobby is ready and insert LocalServerPlayer/LocalClientPlayer.
fn on_lobby_ready(
    mut commands: Commands,
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    server_player: Option<Res<LocalServerPlayer>>,
    client_player: Option<Res<LocalClientPlayer>>,
    host_lobbies: Query<(Entity, &LobbyWebrtcCode), (With<Lobby>, With<Host>)>,
    client_lobbies: Query<Entity, (With<Lobby>, Without<Host>)>,
    mut lobby_state: ResMut<NextState<LobbyState>>,
    local_data: Res<LocalPlayerData>,
) {
    let Some(local_player) = local_player else {
        return;
    };

    if let Ok((lobby_entity, code)) = host_lobbies.single() {
        if server_player.is_none() {
            info!("Hosting room: {}", code.0);
            if let Some(base) = current_base_url() {
                info!("Share link: {}?room={}", base, code.0);
            }
            commands.insert_resource(LocalServerPlayer(local_player.0));
            commands.insert_resource(TickConfig { paused: false });
            lobby_state.set(LobbyState::InLobby);
            let data = local_data.0.clone();
            commands
                .entity(lobby_entity)
                .trigger(move |entity| SetPlayerData::new(entity, data));
        }
    }
    if let Ok(lobby_entity) = client_lobbies.single() {
        if client_player.is_none() {
            commands.insert_resource(LocalClientPlayer(local_player.0));
            lobby_state.set(LobbyState::InLobby);
            let data = local_data.0.clone();
            commands
                .entity(lobby_entity)
                .trigger(move |entity| SetPlayerData::new(entity, data));
        }
    }
}


/// Clean up when lobby is destroyed.
fn cleanup_on_lobby_gone(
    mut commands: Commands,
    mut removed_lobbies: RemovedComponents<Lobby>,
    game_entities: Query<Entity, With<TickTrackedEntity>>,
    mut lobby_state: ResMut<NextState<LobbyState>>,
    mut app_state: ResMut<NextState<AppState>>,
) {
    if removed_lobbies.read().next().is_none() {
        return;
    }

    for entity in game_entities.iter() {
        commands.entity(entity).try_despawn();
    }
    commands.remove_resource::<LocalMultiplayerPlayerId>();
    commands.remove_resource::<LocalServerPlayer>();
    commands.remove_resource::<LocalClientPlayer>();

    commands.insert_resource(CurrentTick(0));
    commands.insert_resource(TickConfig { paused: true });

    lobby_state.set(LobbyState::OutOfLobby);
    app_state.set(AppState::OutOfGame);
}


/// Helper to check if we are the host.
pub fn is_host(server_player: Option<Res<LocalServerPlayer>>) -> bool {
    server_player.is_some()
}

/// Helper to get the local player's UUID.
pub fn local_player_uuid(
    local_client: Option<Res<LocalClientPlayer>>,
    local_server: Option<Res<LocalServerPlayer>>,
) -> Option<u128> {
    local_client
        .as_ref()
        .map(|p| p.0)
        .or_else(|| local_server.as_ref().map(|p| p.0))
}

fn setup(mut commands: Commands) {
    let mut projection = OrthographicProjection::default_2d();
    projection.scaling_mode = bevy::camera::ScalingMode::Fixed {
        width: RESOLUTION.x,
        height: RESOLUTION.y,
    };
    commands.spawn((Camera2d, Projection::Orthographic(projection)));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(5),
            ..default()
        },
        FpsText { current: 0.0 },
    ));
}

fn update_fps(time: Res<Time>, mut texts: Query<(&mut Text, &mut FpsText)>) {
    let delta = time.delta_secs_f64();
    if delta <= 0.0 || delta.is_nan() {
        return;
    }
    let current = 1.0 / delta;
    if current.is_infinite() || current.is_nan() {
        return;
    }
    for (mut text, mut fps) in texts.iter_mut() {
        fps.current = fps.current * 0.9 + current * 0.1;
        *text = Text::new(format!("FPS: {:.0}", fps.current));
    }
}

fn cursor_positon_log(
    q_window: Query<&Window, With<PrimaryWindow>>,
    q_camera: Query<(&Camera, &GlobalTransform)>,
    button_input: Res<ButtonInput<MouseButton>>,
) {
    let (camera, camera_transform) = q_camera.single().unwrap();
    let window = q_window.single().unwrap();
    if let Some(world_position) = window
        .cursor_position()
        .and_then(|cursor| Some(camera.viewport_to_world(camera_transform, cursor)))
        .map(|ray| ray.unwrap().origin.truncate())
    {
        if button_input.just_pressed(MouseButton::Left) {
            info!(
                "World coords: Vec2::new({},{})",
                world_position.x, world_position.y
            );
        }
    }
}

#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
pub struct FinishTimes {
    pub times: HashMap<u128, f32>,
}

impl FinishTimes {
    pub fn get_player_rank(&self, player_uuid: u128) -> Option<usize> {
        let mut all_times = self
            .times
            .iter()
            .map(|(id, time)| (*id, *time))
            .collect::<Vec<_>>();
        all_times.sort_by(|(_, time), (_, time2)| time.partial_cmp(time2).unwrap());
        let rank = all_times
            .iter()
            .position(|(id, _)| *id == player_uuid)
            .map(|index| index + 1)?;
        Some(rank)
    }
}

/// Observer: when a TickTrackedEntity is added (host spawn or client snapshot),
/// add visual and physics components based on EntityKind.
fn on_tracked_entity_spawned(
    trigger: On<Add, TickTrackedEntity>,
    mut commands: Commands,
    query: Query<(
        &EntityKind,
        Option<&OwnerPlayer>,
        Option<&Position>,
        Option<&Rotation>,
        Option<&NetworkedPosition>,
        Option<&NetworkedRotation>,
    )>,
    asset_handles: Res<AssetHandles>,
    participants_with_data: Query<(&LobbyParticipant, Option<&PlayerData<AppPlayerData>>)>,
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    local_server: Option<Res<LocalServerPlayer>>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let entity = trigger.entity;
    let Ok((kind, maybe_owner, maybe_pos, maybe_rot, maybe_net_pos, maybe_net_rot)) =
        query.get(entity)
    else {
        return;
    };

    match kind {
        EntityKind::Kart => {
            // Set initial Transform from avian2d Position/Rotation (snapshot sync)
            if let Some(pos) = maybe_pos {
                let z = SpriteLayers::Car.to_z();
                let mut t = Transform::from_xyz(pos.x, pos.y, z);
                if let Some(rot) = maybe_rot {
                    t.rotation = Quat::from_rotation_z(rot.as_radians());
                }
                commands.entity(entity).insert(t);
            }
            let owner_uuid = maybe_owner.map(|o| o.0).unwrap_or(0);
            let local_uuid = local_player
                .as_ref()
                .map(|p| p.0)
                .or_else(|| local_server.as_ref().map(|p| p.0));
            let is_local = local_uuid.is_some_and(|uuid| uuid == owner_uuid);

            let player = participants_with_data
                .iter()
                .find(|(p, _)| p.player_uuid == owner_uuid)
                .and_then(|(_, data)| data.map(|d| &d.0));
            let kart_color_index = player
                .map(|p| p.kart_color.to_u32() as usize)
                .unwrap_or(0);
            let player_name = player
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            let half_car_width = 2.5;
            let half_car_length = 3.0;

            commands.entity(entity).insert((
                DespawnOnExit(AppState::Game),
                car_controller_2d::CarController2d::new(1.),
                car_controller_2d::CarControllerDisabled,
                Mass(1.),
                RigidBody::Dynamic,
                Collider::rectangle(4., 8.),
                Visibility::Inherited,
                LapsCounter::new(),
                track::position::TrackPosition,
                Sprite::from_atlas_image(
                    asset_handles.karts_texture.clone(),
                    TextureAtlas {
                        layout: asset_handles.karts_atlas.clone(),
                        index: kart_color_index,
                    },
                ),
                observers![|trigger: On<LapUpdate>,
                            tick: Res<CurrentTick>,
                            karts: Query<&car_controller_2d::CarControllerDisabled>,
                            mut commands: Commands,
                            owners: Query<&OwnerPlayer>,
                            mut finish_times: ResMut<FinishTimes>| {
                    if trigger.event().count == track::LAPS_TO_WIN as i32 {
                        if karts.get(trigger.event_target()).is_ok() {
                            return;
                        }
                        if let Ok(owner) = owners.get(trigger.event_target()) {
                            commands
                                .entity(trigger.event_target())
                                .insert(car_controller_2d::CarControllerDisabled);
                            finish_times.times.insert(owner.0, tick.0 as f32);
                        }
                    }
                }],
            ));

            let wheel_tex = asset_handles.wheel_texture.clone();
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Transform::from_xyz(
                        half_car_width,
                        half_car_length - 1.,
                        SpriteLayers::Wheels.to_z(),
                    ),
                    car_controller_2d::CarController2dWheel::new(true, true),
                    Sprite::from_image(wheel_tex.clone()),
                ));
                parent.spawn((
                    Transform::from_xyz(
                        -half_car_width,
                        half_car_length - 1.,
                        SpriteLayers::Wheels.to_z(),
                    ),
                    car_controller_2d::CarController2dWheel::new(true, true),
                    Sprite::from_image(wheel_tex.clone()),
                ));
                parent.spawn((
                    Transform::from_xyz(
                        half_car_width,
                        -half_car_length,
                        SpriteLayers::Wheels.to_z(),
                    ),
                    car_controller_2d::CarController2dWheel::new(false, false),
                    Sprite::from_image(wheel_tex.clone()),
                ));
                parent.spawn((
                    Transform::from_xyz(
                        -half_car_width,
                        -half_car_length,
                        SpriteLayers::Wheels.to_z(),
                    ),
                    car_controller_2d::CarController2dWheel::new(false, false),
                    Sprite::from_image(wheel_tex.clone()),
                ));
            });

            if is_local {
                commands.entity(entity).insert(LocalKart);
            }

            commands.spawn((
                DespawnOnExit(AppState::Game),
                FollowTransform(entity),
                children![(
                    Text2d::new(player_name),
                    Transform::from_xyz(0., 5., SpriteLayers::AboveCar.to_z())
                        .with_scale(Vec3::splat(0.1)),
                )],
            ));
        }
        EntityKind::ItemPickup(_) => {
            let pos = maybe_net_pos.map(|p| p.0).unwrap_or_default();
            commands.entity(entity).insert((
                DespawnOnExit(AppState::Game),
                Transform::from_xyz(pos.x, pos.y, SpriteLayers::Car.to_z()),
                Sprite::from_image(asset_handles.crate_texture.clone()),
            ));
        }
        EntityKind::Rocket => {
            let pos = maybe_net_pos.map(|p| p.0).unwrap_or_default();
            let rot = maybe_net_rot.map(|r| r.0).unwrap_or(0.0);
            let layout = TextureAtlasLayout::from_grid(UVec2::new(3, 8), 2, 1, None, None);
            let atlas_layout = texture_atlas_layouts.add(layout);
            commands.entity(entity).insert((
                DespawnOnExit(AppState::Game),
                Transform::from_xyz(pos.x, pos.y, SpriteLayers::Car.to_z())
                    .with_rotation(Quat::from_rotation_z(rot)),
                Sprite::from_atlas_image(
                    asset_handles.rocket_texture.clone(),
                    TextureAtlas {
                        layout: atlas_layout,
                        index: 0,
                    },
                ),
            ));
        }
    }
}

/// Sync avian2d Position/Rotation to Transform every frame for physics entities.
fn sync_visuals(
    mut physics: Query<
        (&Position, &Rotation, &mut Transform),
        (With<TickTrackedEntity>, With<RigidBody>),
    >,
    mut non_physics: Query<
        (&NetworkedPosition, Option<&NetworkedRotation>, &mut Transform),
        (With<TickTrackedEntity>, Without<RigidBody>),
    >,
) {
    for (pos, rot, mut transform) in physics.iter_mut() {
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
        transform.rotation = Quat::from_rotation_z(rot.as_radians());
    }
    for (pos, rot, mut transform) in non_physics.iter_mut() {
        transform.translation.x = pos.0.x;
        transform.translation.y = pos.0.y;
        if let Some(rot) = rot {
            transform.rotation = Quat::from_rotation_z(rot.0);
        }
    }
}

/// Receive game state changes broadcast by the host.
fn receive_game_state_changed(
    server_player: Option<Res<LocalServerPlayer>>,
    mut reader: MessageReader<ReceivedEnsembleMessage<GameStateChanged>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for msg in reader.read() {
        if server_player.is_some() {
            continue; // Host already set state locally
        }
        next_state.set(msg.message.0.clone());
    }
}
