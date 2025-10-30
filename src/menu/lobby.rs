use crate::AppPlayerData;
use crate::AppState;
use crate::AssetHandles;
use crate::CAR_COLORS_COUNT;
use crate::FinishTimes;
use crate::KartColor;
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
                    handle_kart_preview_add,
                    handle_kart_preview,
                    handle_local_kart_preview,
                )
                    .chain(),
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

#[derive(Component)]
struct KartPreview(KartColor);

impl KartPreview {
    fn new(kart_color: KartColor) -> Self {
        Self(kart_color)
    }
}

impl Default for KartPreview {
    fn default() -> Self {
        Self::new(KartColor::new())
    }
}

#[derive(Component)]
struct LocalKartPreview;

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
    players: &Vec<bevy_easy_p2p::PlayerInfo<AppPlayerData>>,
    is_host: bool,
    finish_times: &FinishTimes,
) {
    for player in players {
        let mut player_name_and_rank = player.data.name.clone();
        if let Some(rank) = finish_times.get_player_rank(player.id) {
            player_name_and_rank += &format!(" {}", rank);
        }
        let is_person_host = player.id == NetworkedId::Host;
        let mut base = commands.spawn(Node {
            height: px(65),
            border: UiRect::all(px(5)),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            ..default()
        });
        if is_person_host == false && is_host {
            base.with_child((
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
            );
        } else {
            base.with_child((
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
        base.with_child((
            KartPreview::new(player.data.kart_color),
            Node {
                width: px(50),
                height: px(50),
                ..default()
            },
            player.id,
        ));
        let base_id = base.id();

        commands.entity(parent).add_child(base_id);
    }
}

fn spawn_lobby_players_buttons(
    mut set: ParamSet<(KartEasyP2P, MessageReader<OnRosterUpdate<AppPlayerData>>)>,
    mut commands: Commands,
    buttons: Query<(Entity, Option<&Children>), With<LobbyPlayersButtons>>,
    finish_times: Res<FinishTimes>,
) {
    let easy = set.p0();
    let players = easy.get_players();
    let is_host = easy.is_host();

    let mut roster = set.p1();
    for (button, children) in buttons.iter() {
        if roster.read().len() == 0 && children.is_some() {
            continue;
        }
        commands.entity(button).despawn_children();
        players_buttons(&mut commands, button, &players, is_host, &finish_times);
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
        .observe(|_: On<Pointer<Press>>, mut easy: KartEasyP2P| {
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

    let kart_buttons = commands
        .spawn((Node {
            position_type: PositionType::Absolute,
            right: px(5),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            ..default()
        },))
        .id();
    let left_kart_button = commands
        .spawn((Button, Text::new("<")))
        .observe(|_: On<Pointer<Press>>, mut easy: KartEasyP2P| {
            let current_kart = easy.get_local_player_data().kart_color;
            easy.set_local_player_data(AppPlayerData {
                kart_color: current_kart.left(),
                ..easy.get_local_player_data()
            });
        })
        .id();
    let right_kart_button = commands
        .spawn((Button, Text::new(">")))
        .observe(|_: On<Pointer<Press>>, mut easy: KartEasyP2P| {
            let current_kart = easy.get_local_player_data().kart_color;
            easy.set_local_player_data(AppPlayerData {
                kart_color: current_kart.right(),
                ..easy.get_local_player_data()
            });
        })
        .id();
    let kart_image = commands
        .spawn((
            KartPreview::default(),
            LocalKartPreview,
            Node {
                width: px(50),
                height: px(50),
                ..default()
            },
        ))
        .id();
    commands
        .entity(kart_buttons)
        .add_children(&[left_kart_button, kart_image, right_kart_button]);

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
        kart_buttons,
    ]);
}

fn handle_kart_preview_add(
    mut commands: Commands,
    karts: Query<Entity, Added<KartPreview>>,
    handles: Res<AssetHandles>,
    mut texture_atlases: ResMut<Assets<TextureAtlasLayout>>,
) {
    for entity in karts.iter() {
        let texture_atlas =
            TextureAtlasLayout::from_grid(UVec2::splat(8), CAR_COLORS_COUNT, 1, None, None);
        let texture_atlas_handle = texture_atlases.add(texture_atlas);
        commands.entity(entity).insert(ImageNode::from_atlas_image(
            handles.karts_texture.clone(),
            TextureAtlas::from(texture_atlas_handle),
        ));
    }
}

fn handle_kart_preview(mut image_nodes: Query<(&mut ImageNode, &KartPreview)>) {
    for (mut image_node, kart) in image_nodes.iter_mut() {
        let index = kart.0.to_u32() as usize;
        if let Some(atlas) = &mut image_node.texture_atlas {
            atlas.index = index;
        }
    }
}

fn handle_local_kart_preview(
    easy: KartEasyP2P,
    mut image_nodes: Query<&mut KartPreview, With<LocalKartPreview>>,
) {
    let current_kart = easy.get_local_player_data().kart_color;
    for mut kart in image_nodes.iter_mut() {
        kart.0 = current_kart;
    }
}
