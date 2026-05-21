use crate::kart::{KART_SIZE, KartControlType, spawn_kart};
use crate::menu::{AnimatedButton, animated_button_bundle};
use crate::{
    AppColors, AppPlayerData, AppState, AssetHandles, ChatMessage, FinishTimes, LobbyState,
    RESOLUTION, SpriteLayers,
};
use bevy::prelude::*;
use bevy_bundled_observers::observers;
use bevy_ensemble::prelude::*;
use bevy_ensemble_webrtc::LobbyWebrtcCode;
use bevy_ticked_networking::prelude::*;
use bevy_ui_text_input::{SubmitText, TextInputMode, TextInputNode};

/// Handle chat text submissions as a system (SubmitText is a Message, not EntityEvent).
fn on_chat_submit(
    mut submit_reader: MessageReader<SubmitText>,
    mut commands: Commands,
    lobbies: Query<Entity, With<Lobby>>,
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    local_server: Option<Res<LocalServerPlayer>>,
    mut history: ResMut<LobbyChatInputHistory>,
) {
    for submit in submit_reader.read() {
        let uuid = local_player
            .as_ref()
            .map(|p| p.0)
            .or_else(|| local_server.as_ref().map(|p| p.0));
        if let (Some(lobby), Some(uuid)) = (lobbies.iter().next(), uuid) {
            let msg = ChatMessage {
                sender: uuid,
                text: submit.text.clone(),
            };
            commands
                .entity(lobby)
                .trigger(move |entity| BroadcastLobbyMessage::new(entity, msg));
        }
        history.add(format!("You: {}", submit.text));
    }
}
use rand::distr::Distribution;
use rand::distr::weighted::WeightedIndex;
use rand::{Rng, rng};

pub const BACKGROUND_ELEMENT_TYPES_COUNT: usize = 8;

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LobbyChatInputHistory(Vec::new()))
            .add_systems(
                Update,
                (
                    (
                        lobby_code,
                        lobby_chat_input_history,
                        spawn_lobby_players_buttons,
                        on_client_message_received,
                        on_chat_submit,
                        update_lobby_cars,
                        receive_ping,
                    )
                        .run_if(in_state(AppState::OutOfGame))
                        .run_if(in_state(LobbyState::InLobby)),
                    on_lobby_exit,
                    spawn_background_elements,
                    handle_background_elements,
                    log_last_pong,
                ),
            )
            .insert_resource(LobbyChatInputHistory(Vec::new()));
    }
}

#[derive(Component)]
struct LobbyCodeText;

#[derive(Component)]
#[require(Text)]
struct PingText;

#[derive(Resource)]
struct LobbyChatInputHistory(Vec<String>);

impl LobbyChatInputHistory {
    fn add(&mut self, text: String) {
        self.0.push(text);
        if self.0.len() > 12 {
            self.0.remove(0);
        }
    }
}

#[derive(Component)]
struct LobbyChatInputHistoryText;

#[derive(Component)]
struct LobbyPlayersButtons;

#[derive(Component)]
pub struct LobbyCar(pub u128);

#[derive(Component)]
pub struct LobbyCarName(pub u128);

#[derive(Component, Clone, Copy)]
enum BackgroundElement {
    Tree,
    Grass,
    YellowFlower,
    RedFlower,
    BlueFlower,
    PurpleFlower,
    Fox,
    Worm,
    Cloud1,
    Cloud2,
}

impl BackgroundElement {
    fn as_sprite(&self, handles: &AssetHandles) -> Sprite {
        let index = match self {
            BackgroundElement::Tree => 0,
            BackgroundElement::Grass => 1,
            BackgroundElement::YellowFlower => 2,
            BackgroundElement::RedFlower => 3,
            BackgroundElement::BlueFlower => 4,
            BackgroundElement::PurpleFlower => 5,
            BackgroundElement::Fox => 6,
            BackgroundElement::Worm => 7,
            BackgroundElement::Cloud1 => 0,
            BackgroundElement::Cloud2 => 1,
        };
        let texture_and_atlas = match self {
            BackgroundElement::Cloud1 => {
                (handles.clouds_texture.clone(), handles.clouds_atlas.clone())
            }
            BackgroundElement::Cloud2 => {
                (handles.clouds_texture.clone(), handles.clouds_atlas.clone())
            }
            _ => (
                handles.background_elements_texture.clone(),
                handles.background_elements_atlas.clone(),
            ),
        };
        Sprite::from_atlas_image(
            texture_and_atlas.0,
            TextureAtlas {
                layout: texture_and_atlas.1,
                index,
            },
        )
    }

    fn pick_random() -> Self {
        let choices = [
            BackgroundElement::Tree,
            BackgroundElement::Grass,
            BackgroundElement::YellowFlower,
            BackgroundElement::RedFlower,
            BackgroundElement::BlueFlower,
            BackgroundElement::PurpleFlower,
            BackgroundElement::Fox,
            BackgroundElement::Worm,
            BackgroundElement::Cloud1,
            BackgroundElement::Cloud2,
        ];
        let weights = [5, 200, 10, 10, 10, 10, 2, 2, 2, 2];
        let dist = WeightedIndex::new(weights).unwrap();
        choices[dist.sample(&mut rng())]
    }

    fn speed(&self) -> f32 {
        match self {
            BackgroundElement::Cloud1 => 30.,
            BackgroundElement::Cloud2 => 45.,
            _ => 100.,
        }
    }

    fn layer(&self) -> SpriteLayers {
        match self {
            BackgroundElement::Cloud1 => SpriteLayers::AboveCar,
            BackgroundElement::Cloud2 => SpriteLayers::AboveCar,
            _ => SpriteLayers::OnGround,
        }
    }
}

fn lobby_code(
    lobby_codes: Query<&LobbyWebrtcCode, With<Lobby>>,
    mut texts: Query<&mut Text, With<LobbyCodeText>>,
) {
    for code in lobby_codes.iter() {
        for mut text in texts.iter_mut() {
            *text = Text::new(code.0.clone());
        }
    }
}

fn lobby_chat_input_history(
    history: Res<LobbyChatInputHistory>,
    mut texts: Query<&mut Text, With<LobbyChatInputHistoryText>>,
) {
    for mut text in texts.iter_mut() {
        *text = Text::new(history.0.join("\n"));
    }
}

fn on_client_message_received(
    mut history: ResMut<LobbyChatInputHistory>,
    mut reader: MessageReader<ReceivedEnsembleMessage<ChatMessage>>,
    participants: Query<(&LobbyParticipant, Option<&PlayerData<AppPlayerData>>)>,
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    local_server: Option<Res<LocalServerPlayer>>,
) {
    let local_uuid = local_player
        .as_ref()
        .map(|p| p.0)
        .or_else(|| local_server.as_ref().map(|p| p.0));
    for msg in reader.read() {
        let chat = &msg.message;
        // Skip own messages — already added locally by on_chat_submit
        if local_uuid.is_some_and(|uuid| uuid == chat.sender) {
            continue;
        }
        let sender_name = participants
            .iter()
            .find(|(p, _)| p.player_uuid == chat.sender)
            .and_then(|(_, data)| data.map(|d| d.0.name.as_str()))
            .unwrap_or("Unknown");
        history.add(format!("{}: {}", sender_name, chat.text));
    }
}

fn on_lobby_exit(
    mut removed_lobbies: RemovedComponents<Lobby>,
    mut inputs: Query<&mut Text, With<TextInputNode>>,
    mut history: ResMut<LobbyChatInputHistory>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if removed_lobbies.read().next().is_none() {
        return;
    }
    for mut input in inputs.iter_mut() {
        input.0.clear();
    }
    history.0.clear();
    next_state.set(AppState::OutOfGame);
}

fn spawn_lobby_players_buttons(
    mut commands: Commands,
    finish_times: Res<FinishTimes>,
    cars: Query<&LobbyCar>,
    participants: Query<&LobbyParticipant>,
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    local_server: Option<Res<LocalServerPlayer>>,
    new_participants: Query<(), Added<LobbyParticipant>>,
) {
    let first_time = cars.count() == 0;
    let has_new = !new_participants.is_empty();
    if !has_new && !first_time {
        return;
    }
    let local_uuid = local_player
        .as_ref()
        .map(|p| p.0)
        .or_else(|| local_server.as_ref().map(|p| p.0));
    let all_participants: Vec<_> = participants.iter().collect();
    let count = all_participants.len();
    for (i, participant) in all_participants.iter().enumerate() {
        let player_uuid = participant.player_uuid;
        if cars.iter().any(|car| car.0 == player_uuid) {
            continue;
        }
        let is_local = local_uuid.is_some_and(|uuid| uuid == player_uuid);
        let is_last = i == count - 1;
        let left_spawn = -RESOLUTION.x / 2.;
        let starting_pos = if first_time {
            if is_local && is_last {
                left_spawn
            } else {
                compute_desired_x(i, count)
            }
        } else {
            left_spawn
        };
        commands.run_system_cached_with(
            spawn_kart,
            (
                KartControlType::LobbyCar(player_uuid, finish_times.get_player_rank(player_uuid)),
                Transform::from_translation(Vec3::X * starting_pos)
                    .with_rotation(Quat::from_rotation_z(-90_f32.to_radians())),
            ),
        );
    }
}

fn compute_desired_x(player_index: usize, count: usize) -> f32 {
    let spacing = 6;
    let total_length = count * KART_SIZE.y as usize + (count - 1) * spacing;
    let front = total_length as f32 / 2.;
    let offset_from_front =
        (player_index as f32 + 0.5) * (KART_SIZE.y as f32) + player_index as f32 * spacing as f32;
    front - offset_from_front
}

fn update_lobby_cars(
    time: Res<Time>,
    mut commands: Commands,
    participants: Query<(&LobbyParticipant, Option<&PlayerData<AppPlayerData>>)>,
    mut cars: Query<(Entity, &LobbyCar, &mut Transform, &mut Sprite)>,
    mut names: Query<(&LobbyCarName, &mut Text2d)>,
) {
    let all_participants: Vec<_> = participants.iter().collect();
    let count = all_participants.len();
    if count == 0 {
        return;
    }
    for (entity, car, mut transform, mut sprite) in cars.iter_mut() {
        let Some((player_index, _)) = all_participants
            .iter()
            .enumerate()
            .find(|(_, (p, _))| p.player_uuid == car.0)
        else {
            // Participant is truly gone — despawn the car
            commands.entity(entity).despawn();
            continue;
        };
        // PlayerData may not have arrived yet — only update visuals if present
        if let Some(player) = all_participants[player_index].1.map(|d| &d.0) {
            if let Some(atlas) = sprite.texture_atlas.as_mut() {
                atlas.index = player.kart_color.to_u32() as usize;
            }
        }
        let desired_x = compute_desired_x(player_index, count);
        transform.translation.x = transform.translation.x.lerp(desired_x, time.delta_secs());
    }
    // Update lobby car name labels when PlayerData arrives or changes
    for (car_name, mut text) in names.iter_mut() {
        if let Some(player) = all_participants
            .iter()
            .find(|(p, _)| p.player_uuid == car_name.0)
            .and_then(|(_, data)| data.map(|d| &d.0))
        {
            let new_name = player.name.as_str();
            if text.0 != new_name && new_name != "..." {
                text.0 = new_name.to_string();
            }
        }
    }
}

pub fn spawn_lobby(
    mut commands: Commands,
    server_player: Option<Res<LocalServerPlayer>>,
    handles: Res<AssetHandles>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let is_host = server_player.is_some();
    let lobby = commands
        .spawn((
            DespawnOnExit(LobbyState::InLobby),
            DespawnOnExit(AppState::OutOfGame),
            Node {
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .id();

    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(RESOLUTION.x, 10.))),
        MeshMaterial2d(materials.add(AppColors::Road.color())),
        Transform::from_xyz(0., 0., SpriteLayers::Background.to_z()),
        DespawnOnExit(LobbyState::InLobby),
        DespawnOnExit(AppState::OutOfGame),
    ));
    let buttons = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: vh(10),
                flex_direction: FlexDirection::ColumnReverse,
                row_gap: px(30),
                ..default()
            },
            children![(
                animated_button_bundle(AnimatedButton(6), &handles, handles.buttons_atlas.clone()),
                observers!(|_: On<Pointer<Press>>,
                            mut commands: Commands,
                            lobbies: Query<Entity, With<Lobby>>| {
                    for lobby in lobbies.iter() {
                        commands.entity(lobby).despawn();
                    }
                }),
            )],
        ))
        .id();

    let lobby_code_text = commands
        .spawn((
            Text::new(""),
            TextFont {
                font_size: 80.0,
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                top: vh(20),
                ..default()
            },
            LobbyCodeText,
        ))
        .id();

    let lobby_chat = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(50),
                left: px(5),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                ..default()
            },
            children![
                (
                    Text::new(""),
                    LobbyChatInputHistoryText,
                    Node {
                        height: px(300),
                        width: px(300),
                        ..default()
                    },
                ),
                (
                    TextInputNode {
                        mode: TextInputMode::SingleLine,
                        ..default()
                    },
                    Node {
                        height: px(25),
                        ..default()
                    },
                    BackgroundColor(AppColors::Dark.color()),
                )
            ],
        ))
        .id();

    let players_buttons = commands
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
        commands.entity(buttons).with_child((
            animated_button_bundle(AnimatedButton(4), &handles, handles.buttons_atlas.clone()),
            observers!(|_: On<Pointer<Press>>,
                        mut next_state: ResMut<NextState<AppState>>,
                        mut commands: Commands,
                        lobbies: Query<Entity, With<Lobby>>| {
                next_state.set(AppState::Game);
                if let Some(lobby) = lobbies.iter().next() {
                    let msg = crate::GameStateChanged(AppState::Game);
                    commands
                        .entity(lobby)
                        .trigger(move |e| BroadcastLobbyMessage::new(e, msg));
                }
            }),
        ));
    }
    commands
        .entity(lobby)
        .add_children(&[lobby_code_text, lobby_chat, players_buttons, buttons]);

    commands.spawn((
        DespawnOnExit(LobbyState::InLobby),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(5),
            ..default()
        },
        PingText,
    ));
}

fn receive_ping(
    lobby_rtt: Query<(&PeerRtt, Option<&PeerLastPong>), With<Lobby>>,
    mut texts: Query<&mut Text, With<PingText>>,
) {
    let Ok((rtt, last_pong)) = lobby_rtt.single() else {
        return;
    };
    let ms = (rtt.0 * 1000.0) as u64;
    let bad = last_pong.is_some_and(|p| p.0 > 2.);
    let label = if bad {
        format!("Ping: {} ms - Bad connection", ms)
    } else {
        format!("Ping: {} ms", ms)
    };
    for mut text in texts.iter_mut() {
        *text = Text::new(label.clone());
    }
}

fn log_last_pong(lobby: Query<&PeerLastPong, With<Lobby>>) {
    let Ok(last_pong) = lobby.single() else {
        return;
    };
    info!("Last pong: {} ms ago", (last_pong.0 * 1000.0) as u64);
}

fn spawn_background_elements(
    mut commands: Commands,
    background_elements: Query<&BackgroundElement>,
    handles: Res<AssetHandles>,
) {
    let max_amount = 60;
    if background_elements.count() >= max_amount {
        return;
    }
    let random_x = rng().random_range(RESOLUTION.x..RESOLUTION.x + 512.);
    let random_y = rng().random_range(-RESOLUTION.y / 2.0..RESOLUTION.y / 2.);
    if random_y.abs() < 10. {
        return;
    }
    let element = BackgroundElement::pick_random();
    commands.spawn((
        element,
        Transform::from_translation(Vec3::new(random_x, random_y, element.layer().to_z())),
        element.as_sprite(&handles),
    ));
}

fn handle_background_elements(
    time: Res<Time>,
    mut commands: Commands,
    mut background_elements: Query<(Entity, &mut Visibility, &mut Transform, &BackgroundElement)>,
    app_state: Res<State<AppState>>,
    lobby_state: Res<State<LobbyState>>,
) {
    for (entity, mut visibility, mut transform, element) in background_elements.iter_mut() {
        let speed = if time.elapsed_secs() < 0.5 {
            10000.
        } else {
            element.speed()
        };

        transform.translation.x -= speed * time.delta_secs();
        if transform.translation.x < -RESOLUTION.x {
            commands.entity(entity).despawn();
        }
        *visibility = if *app_state.get() == AppState::OutOfGame
            && *lobby_state.get() == LobbyState::InLobby
        {
            Visibility::Visible
        } else {
            Visibility::Hidden
        }
    }
}
