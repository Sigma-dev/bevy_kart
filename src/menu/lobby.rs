use crate::kart::{KART_SIZE, KartControlType, spawn_kart};
use crate::menu::animated_button;
use crate::decor::BackgroundElement;
use crate::menu::widgets::text_button;
use crate::scene_util::insert;
use crate::{
    AppColors, AppPlayerData, AppState, AssetHandles, ChatMessage, FinishTimes, RESOLUTION,
    Screen, SpriteLayers,
};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy_ensemble::prelude::*;
use bevy_ensemble_webrtc::LobbyWebrtcCode;
use bevy_ticked_networking::prelude::*;

use super::TextSubmit;

/// Handle chat text submissions as a system (`TextSubmit` is a Message, not EntityEvent).
fn on_chat_submit(
    mut submit_reader: MessageReader<TextSubmit>,
    mut commands: Commands,
    lobbies: Query<Entity, With<Lobby>>,
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    local_server: Option<Res<LocalServerPlayer>>,
    mut history: ResMut<LobbyChatInputHistory>,
    mut inputs: Query<&mut EditableText>,
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
        // Clear the chat box after sending.
        if let Ok(mut input) = inputs.get_mut(submit.entity) {
            input.clear();
        }
    }
}
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
                        show_selected_map,
                    )
                        .run_if(in_state(Screen::Lobby)),
                    on_lobby_exit,
                    spawn_background_elements,
                    handle_background_elements,
                ),
            );
    }
}

#[derive(Component, Default, Clone)]
struct LobbyCodeText;

#[derive(Component, Default, Clone)]
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

#[derive(Component, Default, Clone)]
struct LobbyChatInputHistoryText;

#[derive(Component, Default, Clone)]
struct LobbyPlayersButtons;

/// The lobby's parallax scenery, as opposed to a track's.
///
/// Both are [`BackgroundElement`]s and draw from the same sprite sheet, but only
/// these drift across the screen and hide themselves outside the lobby. Without
/// the distinction the systems below reach into a race and hide every tree on
/// the map -- which is exactly what they did.
#[derive(Component)]
struct ScrollingBackground;

/// Shows the name of the map the race will use.
#[derive(Component, Default, Clone)]
#[require(Text)]
struct SelectedMapText;

#[derive(Component)]
pub struct LobbyCar(pub u128);

#[derive(Component, Default, Clone)]
pub struct LobbyCarName(pub u128);

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
    mut inputs: Query<&mut EditableText>,
    mut history: ResMut<LobbyChatInputHistory>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if removed_lobbies.read().next().is_none() {
        return;
    }
    for mut input in inputs.iter_mut() {
        input.clear();
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
) {
    let is_host = server_player.is_some();
    let lobby = commands
        .spawn_scene(bsn! {
            {insert(DespawnOnExit(Screen::Lobby))}
            Pickable::IGNORE
            Node {
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            }
        })
        .id();

    // Road stripe behind the lobby cars. `asset_value` creates the mesh/material
    // inline at scene-resolve time, so no `Assets` params are needed here.
    commands.spawn_scene(bsn! {
        Mesh2d(asset_value(Rectangle::new(RESOLUTION.x, 10.)))
        MeshMaterial2d::<ColorMaterial>(asset_value(ColorMaterial::from(AppColors::Road.color())))
        Transform::from_xyz(0., 0., SpriteLayers::Background.to_z())
        {insert(DespawnOnExit(Screen::Lobby))}
    });

    // Bottom button column: leave-lobby always, start-game only for the host.
    let buttons = commands
        .spawn_scene(bsn! {
            Node {
                position_type: PositionType::Absolute,
                bottom: vh(10),
                flex_direction: FlexDirection::ColumnReverse,
                row_gap: px(30),
            }
            Children [
                (
                    animated_button(6, handles.buttons_texture.clone(), handles.buttons_atlas.clone())
                    on(|_: On<Pointer<Press>>,
                        mut commands: Commands,
                        lobbies: Query<Entity, With<Lobby>>| {
                        for lobby in lobbies.iter() {
                            commands.entity(lobby).despawn();
                        }
                    })
                )
            ]
        })
        .id();
    if is_host {
        commands
            .spawn_scene(bsn! {
                animated_button(4, handles.buttons_texture.clone(), handles.buttons_atlas.clone())
                on(|_: On<Pointer<Press>>,
                    mut next_state: ResMut<NextState<AppState>>,
                    mut commands: Commands,
                    selected: Res<crate::track::SelectedMap>,
                    lobbies: Query<Entity, With<Lobby>>| {
                    // The map travels with the start, in one message: two would
                    // race each other on the native transport. See `map_sync`.
                    let map = selected.0.clone();
                    if let Some(lobby) = lobbies.iter().next() {
                        let msg = crate::map_sync::StartRace(map.clone());
                        commands
                            .entity(lobby)
                            .trigger(move |e| BroadcastLobbyMessage::new(e, msg));
                    }
                    crate::map_sync::begin_race(&mut commands, &mut next_state, map);
                })
            })
            .insert(ChildOf(buttons));
    }

    let lobby_code_text = commands
        .spawn_scene(bsn! {
            Text::new("")
            TextFont { font_size: {FontSize::Px(80.0)} }
            Node {
                position_type: PositionType::Absolute,
                top: vh(20),
            }
            LobbyCodeText
        })
        .id();

    let lobby_chat = commands
        .spawn_scene(bsn! {
            Node {
                position_type: PositionType::Absolute,
                top: px(50),
                left: px(5),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
            }
            Children [
                (
                    Text::new("")
                    LobbyChatInputHistoryText
                    Node { height: px(300), width: px(300) }
                ),
                (
                    EditableText { allow_newlines: false }
                    Node { height: px(25), width: px(300) }
                    BackgroundColor({AppColors::Dark.color()})
                ),
            ]
        })
        .id();

    // Which map the race will use. Everyone sees the name; only the host can
    // change it, because only the host's choice is the one that travels.
    let map_panel = commands
        .spawn_scene(bsn! {
            Node {
                position_type: PositionType::Absolute,
                top: px(50),
                right: px(5),
                width: px(280),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                align_items: AlignItems::FlexEnd,
            }
            Children [
                (
                    Text::new("MAP")
                    TextFont { font_size: {FontSize::Px(20.)} }
                ),
                (
                    Text::new("")
                    TextFont { font_size: {FontSize::Px(26.)} }
                    SelectedMapText
                ),
            ]
        })
        .id();
    if is_host {
        commands
            .spawn_scene(bsn! {
                text_button("NEXT MAP")
                on(|_: On<Pointer<Press>>, mut selected: ResMut<crate::track::SelectedMap>| {
                    // Cycles the built-ins. A placeholder for the real picker,
                    // but enough to prove a map the clients never chose reaches
                    // them and is what everybody races.
                    let names: Vec<String> = crate::track::map::BUILTINS
                        .iter()
                        .map(|builtin| builtin.load().name)
                        .collect();
                    let next = names
                        .iter()
                        .position(|name| *name == selected.0.name)
                        .map(|index| (index + 1) % names.len())
                        .unwrap_or(0);
                    selected.0 = crate::track::map::BUILTINS[next].load();
                })
            })
            .insert(ChildOf(map_panel));
    }

    let players_buttons = commands
        .spawn_scene(bsn! {
            LobbyPlayersButtons
            Node {
                position_type: PositionType::Absolute,
                top: px(5),
                right: px(5),
            }
        })
        .id();

    commands
        .entity(lobby)
        .add_children(&[lobby_code_text, lobby_chat, players_buttons, buttons, map_panel]);

    // Ping display.
    commands.spawn_scene(bsn! {
        {insert(DespawnOnExit(Screen::Lobby))}
        Node {
            position_type: PositionType::Absolute,
            bottom: px(5),
        }
        PingText
    });
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

fn spawn_background_elements(
    mut commands: Commands,
    background_elements: Query<&BackgroundElement, With<ScrollingBackground>>,
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
        ScrollingBackground,
        Transform::from_translation(Vec3::new(random_x, random_y, element.layer().to_z())),
        element.as_sprite(&handles),
        DespawnOnExit(Screen::Lobby),
    ));
}

fn handle_background_elements(
    time: Res<Time>,
    mut commands: Commands,
    mut background_elements: Query<
        (Entity, &mut Visibility, &mut Transform, &BackgroundElement),
        With<ScrollingBackground>,
    >,
    screen: Res<State<Screen>>,
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
        *visibility = if *screen.get() == Screen::Lobby {
            Visibility::Visible
        } else {
            Visibility::Hidden
        }
    }
}

/// Keep the lobby's map name in step with the selection.
///
/// Reads the same resource on the host and on a client: the host writes it from
/// the picker, and a client has it written by `map_sync` when the host's choice
/// arrives. Neither knows which it is.
fn show_selected_map(
    selected: Res<crate::track::SelectedMap>,
    mut texts: Query<&mut Text, With<SelectedMapText>>,
) {
    for mut text in texts.iter_mut() {
        if text.0 != selected.0.name {
            *text = Text::new(selected.0.name.clone());
        }
    }
}
