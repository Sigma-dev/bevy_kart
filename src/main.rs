use crate::items::{ItemType, ItemsPlugin, spawn_item_pickup, spawn_rocket};
use crate::kart::{
    KART_COLORS_COUNT, KART_SIZE, KartColor, KartControlType, KartPlugin, spawn_kart,
};
use crate::menu::MenuPlugin;
use crate::menu::lobby::{BACKGROUND_ELEMENT_TYPES_COUNT, spawn_lobby};
use crate::menu::start::spawn_menu;
use crate::track::{TrackPlugin, spawn_track};
use audio_manager::AudioManagerPlugin;
use avian2d::prelude::*;
use bevy::asset::AssetMetaCheck;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_easy_p2p::easy_firestore_p2p::{FirestoreP2PPlugin, FirestoreWebRtcTransport};
use bevy_easy_p2p::prelude::*;
use bevy_easy_p2p::{EasyP2PSystemSet, EasyP2PUpdate, NetworkedId, NetworkedStatesExt};
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

pub type KartEasyP2P<'w, 's> =
    EasyP2P<'w, 's, FirestoreWebRtcTransport, AppPlayerData, AppPlayerInputData, AppInstantiations>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AppPlayerData {
    pub name: String,
    pub kart_color: KartColor,
}

impl Default for AppPlayerData {
    fn default() -> Self {
        Self {
            name: "YOUR_NAME".to_string(),
            kart_color: KartColor::default(),
        }
    }
}
#[derive(States, Default, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
enum AppState {
    #[default]
    OutOfGame,
    Game,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppPlayerInputData {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub using_item: bool,
}

#[derive(Resource)]
pub struct AssetHandles {
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AppInstantiations {
    Kart(NetworkedId),
    ItemPickup(ItemType),
    Rocket,
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

#[derive(Clone, Message, Debug)]
struct AppP2PUpdate(EasyP2PUpdate<AppPlayerData, AppPlayerInputData, AppInstantiations>);

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

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(AssetPlugin {
                    // Wasm builds will check for meta files (that don't exist) if this isn't set.
                    // This causes errors and even panics on web build on itch or with SPA dev-servers.
                    // See https://github.com/bevyengine/bevy_github_ci_template/issues/48.
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                }),
            PhysicsPlugins::default(),
            TextInputPlugin,
        ))
        .insert_resource(Gravity::ZERO)
        .add_plugins((
            EasyP2PPlugin::<
                FirestoreWebRtcTransport,
                AppPlayerData,
                AppPlayerInputData,
                AppInstantiations,
            >::default(),
            FirestoreP2PPlugin::<AppPlayerData, AppPlayerInputData, AppInstantiations>::default(),
            CarController2dPlugin,
            AudioManagerPlugin::default(),
            TimerPlugin,
        ))
        .add_plugins((MenuPlugin, TrackPlugin, ItemsPlugin, KartPlugin))
        .add_systems(Startup, (auto_join_from_url, setup))
        .init_state::<AppState>()
        .init_networked_state::<AppState>()
        .add_message::<AppP2PUpdate>()
        .insert_resource(FinishTimes {
            times: HashMap::new(),
        })
        .add_plugins(|app: &mut App| {
            let asset_server = app.world().get_resource::<AssetServer>().unwrap().clone();
            let mut texture_atlases = app
                .world_mut()
                .get_resource_mut::<Assets<TextureAtlasLayout>>()
                .unwrap();
            let asset_handles = AssetHandles {
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
        .add_systems(Update, emit_easy_updates.in_set(EasyP2PSystemSet::Emit))
        .add_systems(Update, on_lobby_created.after(EasyP2PSystemSet::Emit))
        .add_systems(Update, on_instantiation.after(EasyP2PSystemSet::Core))
        .add_systems(OnEnter(P2PLobbyState::OutOfLobby), spawn_menu)
        .add_systems(OnEnter(P2PLobbyState::InLobby), spawn_lobby)
        .add_systems(OnEnter(AppState::Game), spawn_track)
        .add_systems(OnExit(AppState::Game), spawn_lobby)
        .add_systems(Update, (send_inputs, cursor_positon_log))
        .insert_resource(ClearColor(AppColors::Grass.color()))
        .run();
}

fn get_url() -> Option<String> {
    web_sys::window()?.location().href().ok()
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

fn send_inputs(mut easy: KartEasyP2P, keyboard: Res<ButtonInput<KeyCode>>) {
    easy.send_inputs(AppPlayerInputData {
        forward: keyboard.pressed(KeyCode::KeyW) || keyboard.pressed(KeyCode::ArrowUp),
        backward: keyboard.pressed(KeyCode::KeyS) || keyboard.pressed(KeyCode::ArrowDown),
        left: keyboard.pressed(KeyCode::KeyA) || keyboard.pressed(KeyCode::ArrowLeft),
        right: keyboard.pressed(KeyCode::KeyD) || keyboard.pressed(KeyCode::ArrowRight),
        using_item: keyboard.pressed(KeyCode::Space),
    });
}

fn auto_join_from_url(mut easy: KartEasyP2P) {
    if let Some(room) = extract_query_param("room") {
        info!("room code in url: {}", room);
        if !room.trim().is_empty() {
            easy.join_lobby(&room);
        }
    }
}

fn on_lobby_created(mut events: MessageReader<AppP2PUpdate>) {
    for AppP2PUpdate(update) in events.read() {
        if let EasyP2PUpdate::LobbyCreated { code } = update {
            info!("Hosting room: {}", code);
            if let Some(base) = current_base_url() {
                info!("Share link: {}?room={}", base, code);
            }
        }
    }
}

fn on_instantiation(mut commands: Commands, mut easy: KartEasyP2P) {
    for data in easy.get_instantiations() {
        match &data.instantiation {
            AppInstantiations::Kart(id) => {
                commands.run_system_cached_with(
                    spawn_kart,
                    (
                        KartControlType::Player(NetworkedEntity::new(*id, data.uuid)),
                        data.transform,
                    ),
                );
            }
            AppInstantiations::ItemPickup(item) => {
                commands.run_system_cached_with(
                    spawn_item_pickup,
                    (
                        *item,
                        data.transform,
                        NetworkedEntity::new(NetworkedId::Host, data.uuid),
                    ),
                );
            }
            AppInstantiations::Rocket => commands.run_system_cached_with(
                spawn_rocket,
                (
                    data.transform,
                    NetworkedEntity::new(NetworkedId::Host, data.uuid),
                ),
            ),
        }
    }
}

fn setup(mut commands: Commands) {
    let mut projection = OrthographicProjection::default_2d();
    projection.scaling_mode = bevy::camera::ScalingMode::Fixed {
        width: RESOLUTION.x,
        height: RESOLUTION.y,
    };
    commands.spawn((Camera2d, Projection::Orthographic(projection)));
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
struct FinishTimes {
    #[serde(serialize_with = "ser_times", deserialize_with = "de_times")]
    pub times: HashMap<NetworkedId, f32>,
}

fn ser_times<S>(map: &HashMap<NetworkedId, f32>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let as_vec: Vec<(NetworkedId, f32)> = map.iter().map(|(k, v)| (*k, *v)).collect();
    as_vec.serialize(serializer)
}

fn de_times<'de, D>(deserializer: D) -> Result<HashMap<NetworkedId, f32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let vec: Vec<(NetworkedId, f32)> = Vec::deserialize(deserializer)?;
    Ok(vec.into_iter().collect())
}

impl FinishTimes {
    fn get_player_rank(&self, player_id: NetworkedId) -> Option<usize> {
        let mut all_times = self
            .times
            .iter()
            .map(|(id, time)| (id.clone(), *time))
            .collect::<Vec<_>>();
        all_times.sort_by(|(_, time), (_, time2)| time.partial_cmp(time2).unwrap());
        let rank = all_times
            .iter()
            .position(|(id, _)| *id == player_id)
            .map(|index| index + 1)?;
        Some(rank)
    }
}

fn emit_easy_updates(mut easy: KartEasyP2P, mut writer: MessageWriter<AppP2PUpdate>) {
    for update in easy.read_updates() {
        writer.write(AppP2PUpdate(update));
    }
}
