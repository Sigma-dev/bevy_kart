use crate::kart::{KART_SIZE, KartControlType, spawn_kart};
use crate::menu::{AnimatedButton, animated_button_bundle};
use crate::{
    AppColors, AppP2PUpdate, AppPlayerData, AppState, AssetHandles, FinishTimes, KartEasyP2P,
    RESOLUTION, SpriteLayers,
};
use bevy::prelude::*;
use bevy_bundled_observers::observers;
use bevy_easy_p2p::prelude::*;
use bevy_easy_p2p::{EasyP2PUpdate, NetworkedId};
use bevy_ui_text_input::{SubmitText, TextInputMode, TextInputNode};
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
                        on_host_message_received,
                        receive_ping,
                        update_lobby_cars,
                    )
                        .run_if(in_state(AppState::OutOfGame))
                        .run_if(in_state(P2PLobbyState::InLobby)),
                    on_lobby_exit,
                    spawn_background_elements,
                    handle_background_elements,
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
pub struct LobbyCar(pub NetworkedId);

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
    mut events: MessageReader<AppP2PUpdate>,
    mut history: ResMut<LobbyChatInputHistory>,
    easy: KartEasyP2P,
) {
    for AppP2PUpdate(update) in events.read() {
        if let EasyP2PUpdate::ClientChat { client_id, text } = update {
            history.add(format!(
                "{}: {}",
                easy.get_player_data(NetworkedId::ClientId(*client_id))
                    .unwrap()
                    .name,
                text
            ));
        }
    }
}

fn on_host_message_received(
    mut events: MessageReader<AppP2PUpdate>,
    mut history: ResMut<LobbyChatInputHistory>,
    easy: KartEasyP2P,
) {
    for AppP2PUpdate(update) in events.read() {
        if let EasyP2PUpdate::HostChat { text } = update {
            history.add(format!(
                "{}: {}",
                easy.get_player_data(NetworkedId::Host).unwrap().name,
                text
            ));
        }
    }
}

fn on_lobby_exit(
    mut events: MessageReader<AppP2PUpdate>,
    mut inputs: Query<&mut Text, With<TextInputNode>>,
    mut history: ResMut<LobbyChatInputHistory>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for AppP2PUpdate(update) in events.read() {
        if let EasyP2PUpdate::LobbyExited { reason: _ } = update {
            for mut input in inputs.iter_mut() {
                input.0.clear();
            }
            history.0.clear();
            next_state.set(AppState::OutOfGame);
        }
    }
}

fn spawn_lobby_players_buttons(
    mut set: ParamSet<(KartEasyP2P, MessageReader<AppP2PUpdate>)>,
    mut commands: Commands,
    finish_times: Res<FinishTimes>,
    cars: Query<&LobbyCar>,
) {
    let has_update = set
        .p1()
        .read()
        .any(|AppP2PUpdate(update)| matches!(update, EasyP2PUpdate::RosterUpdated { .. }));
    let first_time = cars.count() == 0;
    if !has_update && !first_time {
        return;
    }
    let players = set.p0().get_players();
    let count = players.len();
    for player in players {
        if cars.iter().any(|car| car.0 == player.id) {
            continue;
        }
        let Some(local_player_id) = set.p0().get_local_player_id() else {
            continue;
        };
        let player_index = set.p0().get_player_index(player.id).unwrap();
        let is_local = local_player_id == player.id;
        let left_spawn = -RESOLUTION.x / 2.;
        let starting_pos = if first_time {
            if is_local {
                left_spawn
            } else {
                compute_desired_x(player_index, count)
            }
        } else {
            left_spawn
        };
        commands.run_system_cached_with(
            spawn_kart,
            (
                KartControlType::LobbyCar(player.id, finish_times.get_player_rank(player.id)),
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
    easy: KartEasyP2P,
    mut cars: Query<(Entity, &LobbyCar, &mut Transform, &mut Sprite)>,
) {
    let count = easy.get_players().len();
    if count == 0 {
        return;
    }
    for (entity, car, mut transform, mut sprite) in cars.iter_mut() {
        let Some(player) = easy.get_player_data(car.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        let player_index = easy.get_player_index(car.0).unwrap();
        sprite.texture_atlas.as_mut().unwrap().index = player.kart_color.to_u32() as usize;
        let desired_x = compute_desired_x(player_index, count);
        transform.translation.x = transform.translation.x.lerp(desired_x, time.delta_secs());
    }
}

pub fn spawn_lobby(
    mut commands: Commands,
    easy: KartEasyP2P,
    handles: Res<AssetHandles>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
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
            Pickable::IGNORE,
        ))
        .id();

    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(RESOLUTION.x, 10.))),
        MeshMaterial2d(materials.add(AppColors::Road.color())),
        Transform::from_xyz(0., 0., SpriteLayers::Background.to_z()),
        DespawnOnExit(P2PLobbyState::InLobby),
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
                observers!(|_: On<Pointer<Press>>, mut easy: KartEasyP2P| {
                    easy.exit_lobby();
                }),
            )],
        ))
        .id();

    let lobby_code_text = commands
        .spawn((
            // Accepts a `String` or any type that converts into a `String`, such as `&str`
            Text::new(""),
            TextFont {
                // This font is loaded and will be used instead of the default font.
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

    let lobby_chat_input_text = commands
        .spawn((
            // Accepts a `String` or any type that converts into a `String`, such as `&str`
            TextInputNode {
                mode: TextInputMode::SingleLine,
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                bottom: px(5),
                right: px(5),
                height: px(25),
                ..default()
            },
            BackgroundColor(AppColors::Dark.color()),
        ))
        .observe(
            |trigger: On<SubmitText>,
             mut easy: KartEasyP2P,
             mut history: ResMut<LobbyChatInputHistory>| {
                if easy.is_host() {
                    easy.send_message_all(trigger.text.clone());
                } else {
                    easy.send_message_to_host(trigger.text.clone());
                }
                history.add(format!("You: {}", trigger.text));
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
            observers!(
                |_: On<Pointer<Press>>, mut next_state: ResMut<NextState<AppState>>| {
                    next_state.set(AppState::Game);
                }
            ),
        ));
    }
    commands.entity(lobby).add_children(&[
        lobby_code_text,
        lobby_chat_input_text,
        lobby_chat_input_history,
        players_buttons,
        buttons,
    ]);

    commands.spawn((
        DespawnOnExit(P2PLobbyState::InLobby),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(5),
            ..default()
        },
        PingText,
    ));
}

fn receive_ping(
    mut updates: MessageReader<PingUpdate>,
    mut texts: Query<&mut Text, With<PingText>>,
) {
    for PingUpdate(ping) in updates.read() {
        for mut text in texts.iter_mut() {
            *text = Text::new(format!("Ping: {} ms", ping.as_millis()));
        }
    }
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
    p2p_state: Res<State<P2PLobbyState>>,
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
            && *p2p_state.get() == P2PLobbyState::InLobby
        {
            Visibility::Visible
        } else {
            Visibility::Hidden
        }
    }
}
