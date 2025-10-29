use crate::AppPlayerData;
use crate::AppState;
use crate::FinishTimes;
use crate::KartEasyP2P;
use bevy::prelude::*;
use bevy_easy_p2p::prelude::*;
use bevy_text_input::prelude::*;

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LobbyChatInputHistory(Vec::new()))
            .add_systems(
                Update,
                (
                    lobby_code,
                    lobby_chat_input_history,
                    spawn_lobby_players_buttons,
                    on_lobby_exit,
                    on_client_message_received,
                    on_host_message_received,
                ),
            )
            .insert_resource(LobbyChatInputHistory(Vec::new()));
    }
}

#[derive(Component)]
struct LobbyCodeText;

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

fn lobby_code(
    state: Res<EasyP2PState<AppPlayerData>>,
    mut texts: Query<&mut Text, With<LobbyCodeText>>,
) {
    for mut text in texts.iter_mut() {
        *text = Text::new(state.lobby_code.clone());
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

#[derive(Component)]
struct KickTarget(NetworkedId);

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

fn players_buttons(
    commands: &mut Commands,
    parent: Entity,
    players: Vec<bevy_easy_p2p::PlayerInfo<AppPlayerData>>,
    is_host: bool,
    finish_times: &FinishTimes,
) {
    for player in players {
        let mut player_name_and_rank = player.data.name.clone();
        if let Some(rank) = finish_times.get_player_rank(player.id) {
            player_name_and_rank += &format!(" {}", rank);
        }
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
                        Text::new(&player_name_and_rank),
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
                Text::new(&player_name_and_rank),
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
    buttons: Query<Entity, With<LobbyPlayersButtons>>,
    easy: KartEasyP2P,
    finish_times: Res<FinishTimes>,
) {
    for button in buttons.iter() {
        let players = easy.get_players();
        commands.entity(button).despawn_children();
        players_buttons(
            &mut commands,
            button,
            players,
            easy.is_host(),
            &finish_times,
        );
    }
}

pub fn spawn_lobby(mut commands: Commands, easy: KartEasyP2P) {
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
                right: px(5),
                height: px(25),
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
