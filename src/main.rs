use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_easy_p2p::{
    EasyP2P, EasyP2PPlugin, EasyP2PState, NetworkedEventsExt, OnClientMessageReceived,
    OnHostMessageReceived, OnLobbyCreated, OnLobbyEntered, OnLobbyExit, OnLobbyJoined,
    OnRosterUpdate, P2PLobbyState,
};
use bevy_easy_p2p::{NetworkedId, NetworkedStatesExt};
use bevy_firestore_p2p::FirestoreP2PPlugin;
use bevy_firestore_p2p::FirestoreWebRtcTransport;
use bevy_text_input::prelude::*;
use serde::{Deserialize, Serialize};

use crate::car_controller_2d::{CarController2d, CarController2dWheel};

pub mod car_controller_2d;
use car_controller_2d::CarController2dPlugin;

pub type KartEasyP2P<'w, 's> =
    EasyP2P<'w, 's, FirestoreWebRtcTransport, AppPlayerData, AppPlayerInputData, AppInstantiations>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct AppPlayerData {
    pub name: String,
}

#[derive(States, Default, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
enum AppState {
    #[default]
    OutOfGame,
    Game,
}

impl From<String> for AppPlayerData {
    fn from(value: String) -> Self {
        Self { name: value }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppPlayerInputData {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AppInstantiations {
    Kart(NetworkedId),
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

fn on_lobby_joined(mut r: MessageReader<OnLobbyJoined>) {
    for OnLobbyJoined(code) in r.read() {
        info!("Joined room: {}", code);
    }
}

fn on_lobby_entered(mut r: MessageReader<OnLobbyEntered>) {
    for OnLobbyEntered(code) in r.read() {
        info!("Entered room: {}", code);
    }
}

fn on_lobby_exit(
    mut r: MessageReader<OnLobbyExit>,
    mut inputs: Query<&mut Text, With<TextInput>>,
    mut history: ResMut<LobbyChatInputHistory>,
) {
    for OnLobbyExit(reason) in r.read() {
        info!("Lobby exit: {:?}", reason);
        for mut input in inputs.iter_mut() {
            input.0.clear();
        }
        history.0.clear();
    }
}

fn on_instantiation(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    mut easy: KartEasyP2P,
) {
    for data in easy.get_instantiations() {
        match &data.instantiation {
            AppInstantiations::Kart(id) => {
                let player = easy.get_player_data(id.clone());
                let texture = asset_server.load("sprites/karts.png");
                let layout = TextureAtlasLayout::from_grid(UVec2::splat(8), 8, 1, None, None);
                let texture_atlas_layout = texture_atlas_layouts.add(layout);
                let half_car_width = 3.;
                let half_car_length = 4.;
                let id = commands
                    .spawn((
                        DespawnOnExit(AppState::Game),
                        Mass(1.),
                        RigidBody::Dynamic,
                        Collider::rectangle(6., 8.),
                        Sprite::from_atlas_image(
                            texture,
                            TextureAtlas {
                                layout: texture_atlas_layout,
                                index: 0,
                            },
                        ),
                        data.transform,
                        NetworkedTransform,
                        id.clone(),
                        CarController2d::new(1.),
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
    transforms: Query<&Transform, Without<FollowTransform>>,
    mut follow_transforms: Query<(&mut Transform, &FollowTransform)>,
) {
    for (mut transform, follow_transform) in follow_transforms.iter_mut() {
        let target_transform = transforms.get(follow_transform.0).unwrap();
        transform.translation = target_transform.translation;
    }
}

// Lobby info updates are now routed via OnRosterUpdate and EasyP2PState

fn on_client_message_received(
    mut r: MessageReader<OnClientMessageReceived>,
    mut history: ResMut<LobbyChatInputHistory>,
    easy: KartEasyP2P,
) {
    for OnClientMessageReceived(cid, text) in r.read() {
        history.add(format!(
            "{}: {}",
            easy.get_player_data(NetworkedId::ClientId(*cid)).name,
            text
        ));
    }
}

fn on_host_message_received(
    mut r: MessageReader<OnHostMessageReceived>,
    mut history: ResMut<LobbyChatInputHistory>,
    easy: KartEasyP2P,
) {
    for OnHostMessageReceived(text) in r.read() {
        history.add(format!(
            "{}: {}",
            easy.get_player_data(NetworkedId::Host).name,
            text
        ));
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
        ))
        .add_systems(Startup, (auto_join_from_url, setup))
        .init_state::<AppState>()
        .init_networked_state::<AppState>()
        .add_systems(
            Update,
            (
                on_lobby_created,
                on_lobby_joined,
                on_lobby_entered,
                on_lobby_exit,
                on_client_message_received,
                on_host_message_received,
                on_instantiation,
            ),
        )
        .insert_resource(LobbyChatInputHistory(Vec::new()))
        .init_networked_event::<OnNetworkedTransformUpdate>()
        .add_systems(OnEnter(P2PLobbyState::OutOfLobby), spawn_menu)
        .add_systems(OnEnter(P2PLobbyState::InLobby), spawn_lobby)
        .add_systems(OnEnter(AppState::Game), spawn_track)
        .add_systems(
            Update,
            (
                send_inputs,
                follow_transform,
                lobby_code,
                lobby_chat_input_history,
                spawn_lobby_players_buttons,
                spawn_client_players_buttons,
                networked_transform,
                apply_networked_transform,
            ),
        )
        .run();
}

fn setup(mut commands: Commands) {
    let mut projection = OrthographicProjection::default_2d();
    projection.scaling_mode = bevy::camera::ScalingMode::Fixed {
        width: 256.,
        height: 144.,
    };
    commands.spawn((Camera2d, Projection::Orthographic(projection)));
}

#[derive(Component)]
struct LobbyCodeText;

fn lobby_code(
    state: Res<EasyP2PState<AppPlayerData>>,
    mut texts: Query<&mut Text, With<LobbyCodeText>>,
) {
    for mut text in texts.iter_mut() {
        *text = Text::new(state.lobby_code.clone());
    }
}

#[derive(Resource)]
struct LobbyChatInputHistory(Vec<String>);

impl LobbyChatInputHistory {
    fn add(&mut self, text: String) {
        self.0.push(text);
        if self.0.len() > 10 {
            self.0.remove(0);
        }
    }
}

#[derive(Component)]
struct LobbyChatInputHistoryText;

#[derive(Component)]
struct LobbyPlayersButtons;

fn lobby_chat_input_history(
    history: Res<LobbyChatInputHistory>,
    mut texts: Query<&mut Text, With<LobbyChatInputHistoryText>>,
) {
    for mut text in texts.iter_mut() {
        *text = Text::new(history.0.join("\n"));
    }
}

#[derive(Component)]
struct KickTarget(NetworkedId);

fn players_buttons(
    commands: &mut Commands,
    parent: Entity,
    players: Vec<bevy_easy_p2p::PlayerInfo<AppPlayerData>>,
    is_host: bool,
) {
    for player in players {
        let is_person_host = player.id == NetworkedId::Host;
        if is_person_host == false && is_host {
            let button = commands
                .spawn((
                    Button,
                    Node {
                        height: px(65),
                        border: UiRect::all(px(5)),
                        // horizontally center child text
                        justify_content: JustifyContent::Center,
                        // vertically center child text
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BorderColor::all(Color::WHITE),
                    BorderRadius::MAX,
                    BackgroundColor(Color::linear_rgb(0.94, 0.00, 0.00)),
                    KickTarget(player.id),
                    children![(
                        Text::new(player.data.name.clone()),
                        TextFont {
                            font_size: 33.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.9, 0.9, 0.9)),
                        TextShadow::default(),
                    )],
                ))
                .observe(
                    |trigger: On<Pointer<Press>>,
                     mut easy: KartEasyP2P,
                     kick_targets: Query<&KickTarget>| {
                        let target = kick_targets.get(trigger.entity).unwrap();
                        if let NetworkedId::ClientId(cid) = target.0 {
                            easy.kick(cid);
                        }
                    },
                )
                .id();
            commands.entity(parent).add_child(button);

            continue;
        }
        commands.entity(parent).with_child((
            Button,
            Node {
                height: px(65),
                border: UiRect::all(px(5)),
                // horizontally center child text
                justify_content: JustifyContent::Center,
                // vertically center child text
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::all(Color::WHITE),
            BorderRadius::MAX,
            BackgroundColor(Color::linear_rgb(0.00, 0.00, 0.00)),
            children![(
                Text::new(player.data.name.clone()),
                TextFont {
                    font_size: 33.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.9, 0.9)),
                TextShadow::default(),
            )],
        ));
    }
}

fn spawn_lobby_players_buttons(
    mut commands: Commands,
    buttons: Query<(Entity, Option<&Children>), With<LobbyPlayersButtons>>,
    easy: KartEasyP2P,
) {
    if !easy.is_host() {
        return;
    }
    for (button, children) in buttons.iter() {
        let players = easy.get_players();
        if children.map(|c| c.len()).unwrap_or(0) == players.len() {
            continue;
        }
        commands.entity(button).despawn_children();
        players_buttons(&mut commands, button, players, easy.is_host());
    }
}

fn spawn_client_players_buttons(
    mut commands: Commands,
    buttons: Query<Entity, With<LobbyPlayersButtons>>,
    mut roster_r: MessageReader<OnRosterUpdate<AppPlayerData>>,
    easy: KartEasyP2P,
) {
    if easy.is_host() {
        return;
    }
    for OnRosterUpdate(list) in roster_r.read() {
        for button in buttons.iter() {
            commands.entity(button).despawn_children();
            players_buttons(&mut commands, button, list.clone(), false);
        }
    }
}

fn spawn_track(mut commands: Commands, asset_server: Res<AssetServer>, mut easy: KartEasyP2P) {
    commands.spawn((
        DespawnOnExit(AppState::Game),
        Sprite::from_image(asset_server.load("sprites/track.png")),
    ));
    if !easy.is_host() {
        return;
    }
    for (i, player) in easy.get_players().iter().enumerate() {
        let i = i as i32;
        let position: Vec3 = Vec3::new(
            (-25 + (i / 3) * -10) as f32,
            (-39 + (i % 3) * -7) as f32,
            10.,
        );
        easy.instantiate(
            AppInstantiations::Kart(player.id.clone()),
            Transform::from_translation(position)
                .with_rotation(Quat::from_rotation_z(-90_f32.to_radians())),
        );
    }
}

#[derive(Component)]
struct NetworkedTransform;

#[derive(Message, Clone, Debug, Serialize, Deserialize)]
struct OnNetworkedTransformUpdate(NetworkedId, (Vec3, Quat));

fn networked_transform(
    easy: KartEasyP2P,
    mut transforms: Query<(Entity, &mut Transform), With<NetworkedTransform>>,
    mut events_w: MessageWriter<OnNetworkedTransformUpdate>,
) {
    if !easy.is_host() {
        return;
    }
    for (entity, transform) in transforms.iter_mut() {
        let Some(networked_id) = easy.get_closest_networked_id(entity) else {
            continue;
        };
        events_w.write(OnNetworkedTransformUpdate(
            networked_id,
            (transform.translation, transform.rotation),
        ));
    }
}

fn apply_networked_transform(
    easy: KartEasyP2P,
    mut transforms: Query<(Entity, &mut Transform), With<NetworkedTransform>>,
    mut events_r: MessageReader<OnNetworkedTransformUpdate>,
) {
    if easy.is_host() {
        return;
    }
    for OnNetworkedTransformUpdate(networked_id, (new_translation, new_rotation)) in events_r.read()
    {
        for (entity, mut transform) in transforms.iter_mut() {
            let closest_networked_id = easy.get_closest_networked_id(entity);
            if closest_networked_id == Some(networked_id.clone()) {
                transform.translation = *new_translation;
                transform.rotation = *new_rotation;
            }
        }
    }
}

fn spawn_lobby(mut commands: Commands, easy: KartEasyP2P) {
    let is_host = easy.is_host();
    let lobby = commands
        .spawn((
            DespawnOnExit(P2PLobbyState::InLobby),
            DespawnOnExit(AppState::OutOfGame),
            Node {
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .id();
    let exit_button = commands
        .spawn((
            Button,
            Node {
                height: px(65),
                border: UiRect::all(px(5)),
                // horizontally center child text
                justify_content: JustifyContent::Center,
                // vertically center child text
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::all(Color::WHITE),
            BorderRadius::MAX,
            BackgroundColor(Color::BLACK),
            children![(
                Text::new("Exit Lobby"),
                TextFont {
                    font_size: 33.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.9, 0.9)),
                TextShadow::default(),
            )],
        ))
        .observe(|_trigger: On<Pointer<Press>>, mut easy: KartEasyP2P| {
            easy.exit_lobby();
        })
        .id();

    let lobby_code_text = commands
        .spawn((
            // Accepts a `String` or any type that converts into a `String`, such as `&str`
            Text::new(""),
            TextFont {
                // This font is loaded and will be used instead of the default font.
                font_size: 67.0,
                ..default()
            },
            TextShadow::default(),
            // Set the justification of the Text
            TextLayout::new_with_justify(Justify::Center),
            // Set the style of the Node itself.
            Node {
                position_type: PositionType::Absolute,
                bottom: px(5),
                left: px(5),
                ..default()
            },
            LobbyCodeText,
        ))
        .id();

    let lobby_chat_input_text = commands
        .spawn((
            // Accepts a `String` or any type that converts into a `String`, such as `&str`
            Text::new(""),
            TextInput::new(false, false, true),
            Node {
                position_type: PositionType::Absolute,
                bottom: px(5),
                left: px(5),
                ..default()
            },
        ))
        .observe(
            |trigger: On<InputFieldSubmit>,
             mut easy: KartEasyP2P,
             mut history: ResMut<LobbyChatInputHistory>| {
                if easy.is_host() {
                    easy.send_message_all(trigger.text().to_string());
                } else {
                    easy.send_message_to_host(trigger.text().to_string());
                }
                history.add(format!("You: {}", trigger.text()));
            },
        )
        .id();

    let lobby_chat_input_history = commands
        .spawn((
            // Accepts a `String` or any type that converts into a `String`, such as `&str`
            Text::new(""),
            LobbyChatInputHistoryText,
            Node {
                position_type: PositionType::Absolute,
                top: px(5),
                left: px(5),
                ..default()
            },
        ))
        .id();
    let buttons = commands
        .spawn((
            LobbyPlayersButtons,
            Node {
                position_type: PositionType::Absolute,
                top: px(5),
                right: px(5),
                ..default()
            },
        ))
        .id();

    if is_host {
        let start_button = commands
            .spawn((
                Button,
                Node {
                    height: px(65),
                    border: UiRect::all(px(5)),
                    // horizontally center child text
                    justify_content: JustifyContent::Center,
                    // vertically center child text
                    align_items: AlignItems::Center,
                    ..default()
                },
                BorderColor::all(Color::WHITE),
                BorderRadius::MAX,
                BackgroundColor(Color::BLACK),
                children![(
                    Text::new("Start Game"),
                    TextFont {
                        font_size: 33.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.9, 0.9, 0.9)),
                    TextShadow::default(),
                )],
            ))
            .observe(
                |_trigger: On<Pointer<Press>>, mut next_state: ResMut<NextState<AppState>>| {
                    next_state.set(AppState::Game);
                },
            )
            .id();
        commands.entity(lobby).add_child(start_button);
    }
    commands.entity(lobby).add_children(&[
        exit_button,
        lobby_code_text,
        lobby_chat_input_text,
        lobby_chat_input_history,
        buttons,
    ]);
}

fn spawn_menu(mut commands: Commands, mut easy: KartEasyP2P) {
    if easy.get_local_player_data().name.is_empty() {
        easy.set_local_player_data(AppPlayerData {
            name: "YOUR_NAME".to_string(),
        });
    }
    let menu = commands
        .spawn((
            DespawnOnExit(P2PLobbyState::OutOfLobby),
            DespawnOnExit(AppState::OutOfGame),
            Node {
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .id();
    let button = commands
        .spawn((
            Button,
            Node {
                height: px(65),
                border: UiRect::all(px(5)),
                // horizontally center child text
                justify_content: JustifyContent::Center,
                // vertically center child text
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::all(Color::WHITE),
            BorderRadius::MAX,
            BackgroundColor(Color::BLACK),
            children![(
                Text::new("Create Lobby"),
                TextFont {
                    font_size: 33.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.9, 0.9)),
                TextShadow::default(),
            )],
        ))
        .observe(|_trigger: On<Pointer<Press>>, mut easy: KartEasyP2P| {
            easy.create_lobby();
        })
        .id();
    let code_input = commands
        .spawn((
            TextInput::new(true, true, true),
            Node {
                position_type: PositionType::Absolute,
                top: px(15),
                left: px(15),
                height: px(25),
                width: px(150),
                ..default()
            },
            Outline {
                width: px(6),
                offset: px(6),
                color: Color::WHITE,
            },
        ))
        .observe(|trigger: On<InputFieldSubmit>, mut easy: KartEasyP2P| {
            easy.join_lobby(&trigger.text().to_string());
        })
        .id();
    let name_input = commands
        .spawn((
            TextInput::new(false, false, false).with_max_characters(10),
            Text::new(easy.get_local_player_data().name.clone()),
            Node {
                position_type: PositionType::Absolute,
                top: px(15),
                right: px(15),
                height: px(25),
                ..default()
            },
            Outline {
                width: px(6),
                offset: px(6),
                color: Color::WHITE,
            },
        ))
        .observe(|trigger: On<InputFieldChange>, mut easy: KartEasyP2P| {
            easy.set_local_player_data(AppPlayerData {
                name: trigger.text().to_string(),
            });
        })
        .id();
    commands
        .entity(menu)
        .add_children(&[button, code_input, name_input]);
}
