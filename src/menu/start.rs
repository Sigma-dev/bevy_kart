use crate::{AppPlayerData, AppState, KartColor, KartEasyP2P};
use bevy::prelude::*;
use bevy_easy_p2p::prelude::*;
use bevy_text_input::prelude::*;

pub fn spawn_menu(mut commands: Commands, mut easy: KartEasyP2P) {
    if easy.get_local_player_data().name.is_empty() {
        easy.set_local_player_data(AppPlayerData {
            name: "YOUR_NAME".to_string(),
            kart_color: KartColor::new(),
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
                height: px(25),
                width: px(150),
                ..default()
            },
        ))
        .observe(|trigger: On<InputFieldSubmit>, mut easy: KartEasyP2P| {
            easy.join_lobby(&trigger.text().to_string());
        })
        .id();
    let mut code_parent = commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(15),
            left: px(15),
            column_gap: px(10),
            ..default()
        },
        children![(Text::new("Lobby Code:"))],
    ));
    code_parent.add_child(code_input);
    let code_parent_id = code_parent.id();
    let name_input = commands
        .spawn((
            TextInput::new(false, false, false).with_max_characters(10),
            Text::new(easy.get_local_player_data().name.clone()),
            Node {
                height: px(25),
                ..default()
            },
        ))
        .observe(|trigger: On<InputFieldChange>, mut easy: KartEasyP2P| {
            easy.set_local_player_data(AppPlayerData {
                name: trigger.text().to_string(),
                kart_color: KartColor::new(),
            });
        })
        .id();
    let mut name_parent = commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(15),
            right: px(15),
            column_gap: px(10),
            ..default()
        },
        children![(Text::new("Name:"),)],
    ));
    name_parent.add_child(name_input);
    let name_parent_id = name_parent.id();
    commands
        .entity(menu)
        .add_children(&[button, code_parent_id, name_parent_id]);
}
