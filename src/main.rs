use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
};
use bevy_easy_p2p::{
    EasyP2P, EasyP2PPlugin, EasyP2PState, OnClientMessageReceived, OnHostMessageReceived,
    OnLobbyCreated, OnLobbyEntered, OnLobbyExit, OnLobbyInfoUpdate, OnLobbyJoined, OnRosterUpdate,
    P2PLobbyState,
};
use bevy_firestore_p2p::FirestoreP2PPlugin;
use bevy_firestore_p2p::FirestoreWebRtcTransport;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppPlayerData {
    pub name: String,
}

impl From<String> for AppPlayerData {
    fn from(value: String) -> Self {
        Self { name: value }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppPlayerInputData {
    pub forward: bool,
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
    info!("href: {}", href);
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

fn auto_join_from_url(
    mut easy: EasyP2P<FirestoreWebRtcTransport, AppPlayerData, AppPlayerInputData>,
) {
    info!("auto_join_from_url");
    if let Some(room) = extract_query_param("room") {
        info!("room: {}", room);
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
    mut inputs: Query<&mut Text, With<InputField>>,
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

fn on_lobby_info(mut r: MessageReader<OnLobbyInfoUpdate>) {
    for OnLobbyInfoUpdate(list) in r.read() {
        info!("Players: {:?}", list);
    }
}

fn on_client_message_received(
    mut r: MessageReader<OnClientMessageReceived>,
    mut history: ResMut<LobbyChatInputHistory>,
) {
    for OnClientMessageReceived(cid, text) in r.read() {
        info!("Client message received: {:?}: {}", cid, text);
        if !text.starts_with("__") {
            history.add(format!("Client {}: {}", cid, text));
        }
    }
}

fn on_host_message_received(
    mut r: MessageReader<OnHostMessageReceived>,
    mut history: ResMut<LobbyChatInputHistory>,
) {
    for OnHostMessageReceived(text) in r.read() {
        info!("Host message received: {}", text);
        if !text.starts_with("__") {
            history.add(format!("Host: {}", text));
        }
    }
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((
            EasyP2PPlugin::<FirestoreWebRtcTransport, AppPlayerData, AppPlayerInputData>::default(),
            FirestoreP2PPlugin::<AppPlayerData, AppPlayerInputData>(std::marker::PhantomData),
        ))
        .add_systems(Startup, (auto_join_from_url, setup))
        .add_systems(
            Update,
            (
                on_lobby_created,
                on_lobby_joined,
                on_lobby_entered,
                on_lobby_exit,
                on_lobby_info,
                on_client_message_received,
                on_host_message_received,
            ),
        )
        .insert_resource(LobbyChatInputHistory(Vec::new()))
        .add_systems(OnEnter(P2PLobbyState::OutOfLobby), spawn_menu)
        .add_systems(OnEnter(P2PLobbyState::InLobby), spawn_lobby)
        .add_systems(
            Update,
            (
                lobby_code,
                lobby_chat_input_history,
                chat_input_inputs,
                spawn_lobby_players_buttons,
                spawn_client_players_buttons,
            ),
        )
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

#[derive(Component)]
struct LobbyCodeText;

fn lobby_code(state: Res<EasyP2PState>, mut texts: Query<&mut Text, With<LobbyCodeText>>) {
    for mut text in texts.iter_mut() {
        *text = Text::new(state.lobby_code.clone());
    }
}

#[derive(Component)]
struct InputField {
    capitalize: bool,
    no_spaces: bool,
}

#[derive(EntityEvent)]
struct InputFieldSubmit {
    entity: Entity,
    text: String,
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

fn chat_input_inputs(
    mut commands: Commands,
    mut evr_kbd: MessageReader<KeyboardInput>,
    mut inputs: Query<(Entity, &mut Text, &InputField)>,
) {
    for ev in evr_kbd.read() {
        // We don't care about key releases, only key presses
        if ev.state == ButtonState::Released {
            continue;
        }
        for (entity, mut input, input_field) in inputs.iter_mut() {
            match &ev.logical_key {
                // Handle pressing Enter to finish the input
                Key::Enter => {
                    println!("Text input: {}", &*input.0);
                    commands.entity(entity).trigger(|entity| InputFieldSubmit {
                        entity: entity,
                        text: input.0.clone(),
                    });

                    input.0.clear();
                }
                // Handle pressing Backspace to delete last char
                Key::Backspace => {
                    input.0.pop();
                }
                Key::Space => {
                    if !input_field.no_spaces {
                        input.0.push(' ');
                    }
                }
                // Handle key presses that produce text characters
                Key::Character(str) => {
                    // Ignore any input that contains control (special) characters
                    if str.chars().any(|c| c.is_control()) {
                        continue;
                    }
                    if input_field.capitalize {
                        input.0.push_str(str.to_uppercase().as_str());
                    } else {
                        input.0.push_str(str.as_str());
                    }
                }
                _ => {}
            }
        }
    }
}

fn players_buttons(
    commands: &mut Commands,
    parent: Entity,
    players: Vec<(String, bool)>,
    is_host: bool,
) {
    for (id, is_person_host) in players {
        if is_person_host == false && is_host {
            let button = commands
                .spawn((
                    Button,
                    Node {
                        width: px(150),
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
                    children![(
                        Text::new(id),
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
                     mut easy: EasyP2P<
                        FirestoreWebRtcTransport,
                        AppPlayerData,
                        AppPlayerInputData,
                    >,
                     parents: Query<&Children>,
                     texts: Query<&Text>| {
                        let parent = parents.get(trigger.entity).unwrap();
                        for child in parent.iter() {
                            if let Ok(text) = texts.get(child) {
                                easy.kick(text.0.to_string().parse().unwrap());
                            }
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
                width: px(150),
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
                Text::new(id),
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
    easy: EasyP2P<FirestoreWebRtcTransport, AppPlayerData, AppPlayerInputData>,
) {
    if !easy.is_host() {
        return;
    }
    for button in buttons.iter() {
        commands.entity(button).despawn_children();
        players_buttons(&mut commands, button, easy.get_players(), easy.is_host());
    }
}

fn spawn_client_players_buttons(
    mut commands: Commands,
    buttons: Query<Entity, With<LobbyPlayersButtons>>,
    mut roster_r: MessageReader<OnRosterUpdate>,
    easy: EasyP2P<FirestoreWebRtcTransport, AppPlayerData, AppPlayerInputData>,
) {
    if easy.is_host() {
        return;
    }
    for OnRosterUpdate(list) in roster_r.read() {
        info!("spawn_client_players_buttons: {:?}", list);
        for button in buttons.iter() {
            commands.entity(button).despawn_children();
            players_buttons(
                &mut commands,
                button,
                list.clone().into_iter().map(|id| (id, false)).collect(),
                false,
            );
        }
    }
}

fn spawn_lobby(mut commands: Commands) {
    let lobby = commands
        .spawn((
            DespawnOnExit(P2PLobbyState::InLobby),
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
                width: px(150),
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
        .observe(
            |_trigger: On<Pointer<Press>>, mut easy: EasyP2P<FirestoreWebRtcTransport, AppPlayerData, AppPlayerInputData>| {
                easy.exit_lobby();
            },
        )
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
                right: px(5),
                ..default()
            },
            LobbyCodeText,
        ))
        .id();

    let lobby_chat_input_text = commands
        .spawn((
            // Accepts a `String` or any type that converts into a `String`, such as `&str`
            Text::new(""),
            InputField {
                capitalize: false,
                no_spaces: false,
            },
            Node {
                position_type: PositionType::Absolute,
                bottom: px(5),
                left: px(5),
                ..default()
            },
        ))
        .observe(
            |trigger: On<InputFieldSubmit>,
             mut easy: EasyP2P<FirestoreWebRtcTransport, AppPlayerData, AppPlayerInputData>,
             mut history: ResMut<LobbyChatInputHistory>| {
                if easy.is_host() {
                    easy.send_message_all(trigger.text.clone());
                } else {
                    easy.send_message_to_host(trigger.text.clone());
                }
                history.add(format!("You: {}", trigger.text.clone()));
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
    commands.entity(lobby).add_children(&[
        button,
        lobby_code_text,
        lobby_chat_input_text,
        lobby_chat_input_history,
        buttons,
    ]);
}

fn spawn_menu(mut commands: Commands) {
    let menu = commands
        .spawn((
            DespawnOnExit(P2PLobbyState::OutOfLobby),
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
                width: px(150),
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
        .observe(
            |_trigger: On<Pointer<Press>>, mut easy: EasyP2P<FirestoreWebRtcTransport, AppPlayerData, AppPlayerInputData>| {
                easy.create_lobby();
            },
        )
        .id();
    let code_input = commands
        .spawn((
            InputField {
                capitalize: true,
                no_spaces: true,
            },
            Text::new(""),
            Node {
                position_type: PositionType::Absolute,
                top: px(5),
                left: px(5),
                ..default()
            },
        ))
        .observe(
            |trigger: On<InputFieldSubmit>, mut easy: EasyP2P<FirestoreWebRtcTransport, AppPlayerData, AppPlayerInputData>| {
                easy.join_lobby(&trigger.text);
            },
        )
        .id();
    commands.entity(menu).add_children(&[button, code_input]);
}
