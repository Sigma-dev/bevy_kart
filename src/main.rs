use audio_manager::AudioManagerPlugin;
use avian2d::prelude::*;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_easy_p2p::prelude::*;
use bevy_easy_p2p::{NetworkedId, NetworkedStatesExt};
use bevy_firestore_p2p::FirestoreP2PPlugin;
use bevy_firestore_p2p::FirestoreWebRtcTransport;
use bevy_text_input::prelude::*;
use serde::{Deserialize, Serialize};

use crate::car_controller_2d::{CarController2d, CarController2dWheel, CarControllerDisabled};
use crate::menu::MenuPlugin;
use crate::menu::lobby::spawn_lobby;
use crate::menu::start::spawn_menu;
use crate::track::{TrackPlugin, spawn_track};

pub mod car_controller_2d;
pub mod menu;
pub mod track;
use car_controller_2d::CarController2dPlugin;

pub type KartEasyP2P<'w, 's> =
    EasyP2P<'w, 's, FirestoreWebRtcTransport, AppPlayerData, AppPlayerInputData, AppInstantiations>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct AppPlayerData {
    pub name: String,
    pub kart_color: KartColor,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct KartColor(u32);

impl KartColor {
    fn new() -> Self {
        Self(0)
    }

    fn right(&self) -> KartColor {
        Self((self.0 + 1) % CAR_COLORS_COUNT)
    }

    fn left(&self) -> KartColor {
        if self.0 == 0 {
            return Self(CAR_COLORS_COUNT - 1);
        }
        Self(self.0 - 1)
    }

    fn to_u32(&self) -> u32 {
        self.0
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
}

#[derive(Resource)]
struct AssetHandles {
    karts_texture: Handle<Image>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AppInstantiations {
    Kart(NetworkedId),
}

#[derive(Component)]
struct LapsCounter(u32);

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
        forward: keyboard.pressed(KeyCode::KeyW),
        backward: keyboard.pressed(KeyCode::KeyS),
        left: keyboard.pressed(KeyCode::KeyA),
        right: keyboard.pressed(KeyCode::KeyD),
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

fn on_lobby_created(mut r: MessageReader<OnLobbyCreated>) {
    for OnLobbyCreated(code) in r.read() {
        info!("Hosting room: {}", code);
        if let Some(base) = current_base_url() {
            info!("Share link: {}?room={}", base, code);
        }
    }
}

fn on_instantiation(
    mut commands: Commands,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    mut easy: KartEasyP2P,
    asset_handles: Res<AssetHandles>,
) {
    for data in easy.get_instantiations() {
        match &data.instantiation {
            AppInstantiations::Kart(id) => {
                let player = easy.get_player_data(id.clone());
                let layout =
                    TextureAtlasLayout::from_grid(UVec2::splat(8), CAR_COLORS_COUNT, 1, None, None);
                let texture_atlas_layout = texture_atlas_layouts.add(layout);
                let half_car_width = 3.;
                let half_car_length = 4.;
                let id = commands
                    .spawn((
                        DespawnOnExit(AppState::Game),
                        Mass(1.),
                        RigidBody::Dynamic,
                        Collider::rectangle(4., 8.),
                        Sprite::from_atlas_image(
                            asset_handles.karts_texture.clone(),
                            TextureAtlas {
                                layout: texture_atlas_layout,
                                index: player.kart_color.to_u32() as usize,
                            },
                        ),
                        data.transform,
                        NetworkedTransform,
                        NetworkedEntity::new(id.clone()),
                        CarController2d::new(1.),
                        CarControllerDisabled,
                        LapsCounter(0),
                        children![
                            (
                                Transform::from_xyz(half_car_width, half_car_length, 0.),
                                CarController2dWheel::new(true, true)
                            ),
                            (
                                Transform::from_xyz(-half_car_width, half_car_length, 0.),
                                CarController2dWheel::new(true, true)
                            ),
                            (
                                Transform::from_xyz(half_car_width, -half_car_length, 0.),
                                CarController2dWheel::new(false, false)
                            ),
                            (
                                Transform::from_xyz(-half_car_width, -half_car_length, 0.),
                                CarController2dWheel::new(false, false)
                            ),
                        ],
                    ))
                    .id();
                commands.spawn((
                    DespawnOnExit(AppState::Game),
                    FollowTransform(id),
                    children![(
                        Text2d::new(player.name),
                        Transform::from_xyz(0., 5., 100.).with_scale(Vec3::splat(0.1)),
                    )],
                ));
            }
        }
    }
}

#[derive(Component)]
#[require(Transform)]

struct FollowTransform(Entity);

fn follow_transform(
    mut commands: Commands,
    transforms: Query<&Transform, Without<FollowTransform>>,
    mut follow_transforms: Query<(Entity, &mut Transform, &FollowTransform)>,
) {
    for (entity, mut transform, follow_transform) in follow_transforms.iter_mut() {
        if let Ok(target_transform) = transforms.get(follow_transform.0) {
            transform.translation = target_transform.translation;
        } else {
            commands.entity(entity).despawn();
        }
    }
}

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(ImagePlugin::default_nearest()),
            PhysicsPlugins::default(),
        ))
        .insert_resource(Gravity::ZERO)
        .add_plugins((
            EasyP2PPlugin::<
                FirestoreWebRtcTransport,
                AppPlayerData,
                AppPlayerInputData,
                AppInstantiations,
            >::default(),
            FirestoreP2PPlugin,
            TextInputPlugin,
            CarController2dPlugin,
            AudioManagerPlugin::default(),
        ))
        .add_plugins((MenuPlugin, TrackPlugin))
        .add_systems(Startup, (auto_join_from_url, setup))
        .init_state::<AppState>()
        .init_networked_state::<AppState>()
        .insert_resource(FinishTimes {
            times: HashMap::new(),
        })
        .insert_resource(AssetHandles {
            karts_texture: Handle::default(),
        })
        .add_systems(Update, (on_lobby_created, on_instantiation))
        .add_systems(OnEnter(P2PLobbyState::OutOfLobby), spawn_menu)
        .add_systems(OnEnter(P2PLobbyState::InLobby), spawn_lobby)
        .add_systems(OnEnter(AppState::Game), spawn_track)
        .add_systems(OnExit(AppState::Game), spawn_lobby)
        .add_systems(Update, (send_inputs, follow_transform, cursor_positon_log))
        .run();
}

fn setup(
    mut commands: Commands,
    mut handles: ResMut<AssetHandles>,
    asset_server: Res<AssetServer>,
) {
    let mut projection = OrthographicProjection::default_2d();
    projection.scaling_mode = bevy::camera::ScalingMode::Fixed {
        width: 256.,
        height: 144.,
    };
    commands.spawn((Camera2d, Projection::Orthographic(projection)));
    handles.karts_texture = asset_server.load("sprites/karts.png");
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

const LAPS_TO_WIN: u32 = 3;
const CAR_COLORS_COUNT: u32 = 8;

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
