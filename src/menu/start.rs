use crate::{
    AppColors, AppState, AssetHandles, LobbyState, LocalPlayerData,
    RESOLUTION, SpriteLayers,
    kart::{AutoCar, KartControlType, spawn_kart},
    menu::animated_button_bundle,
};
use bevy::prelude::*;
use bevy_bundled_observers::observers;
use bevy_ensemble::prelude::*;
use bevy_ensemble_webrtc::JoinWebrtcLobbyByCode;
use bevy_ui_text_input::{
    SubmitText, TextInputBuffer, TextInputContents, TextInputFilter, TextInputMode,
    TextInputModifier, TextInputNode,
};
use rand::Rng;

use super::AnimatedButton;

pub struct StartPlugin;

impl Plugin for StartPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                handle_spawning_menu_cars,
                handle_despawning_menu_cars,
                handle_code_submit,
                handle_name_change,
                hide_controls_while_joining,
            ),
        );
    }
}

#[derive(Component)]
struct MenuCarSpawner {
    next_spawn: Option<f32>,
}

impl MenuCarSpawner {
    fn new() -> Self {
        Self { next_spawn: None }
    }
}

#[derive(Component)]
struct MenuControls;

#[derive(Component)]
struct CodeInput;

#[derive(Component)]
struct NameInput;

pub(crate) fn spawn_menu(
    mut commands: Commands,
    mut texture_atlases: ResMut<Assets<TextureAtlasLayout>>,
    handles: Res<AssetHandles>,
    local_data: Res<LocalPlayerData>,
) {
    let name_atlas = texture_atlases.add(TextureAtlasLayout::from_grid(
        UVec2::new(32, 8),
        2,
        1,
        None,
        None,
    ));
    let logo_atlas = texture_atlases.add(TextureAtlasLayout::from_grid(
        UVec2::new(128, 68),
        2,
        1,
        None,
        None,
    ));
    let menu = commands
        .spawn((
            DespawnOnExit(LobbyState::OutOfLobby),
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
    let car_spawners = commands
        .spawn(children![
            (
                MenuCarSpawner::new(),
                Transform::from_translation(Vec3::new(-134., -6., SpriteLayers::Car.to_z()))
                    .with_rotation(Quat::from_rotation_z(-53_f32.to_radians())),
            ),
            (
                MenuCarSpawner::new(),
                Transform::from_translation(Vec3::new(-142., 0., SpriteLayers::Car.to_z()))
                    .with_rotation(Quat::from_rotation_z(-53_f32.to_radians())),
            ),
            (
                MenuCarSpawner::new(),
                Transform::from_translation(Vec3::new(10., -80., SpriteLayers::Car.to_z()))
                    .with_rotation(Quat::from_rotation_z(72_f32.to_radians())),
            ),
            (
                MenuCarSpawner::new(),
                Transform::from_translation(Vec3::new(20., -80., SpriteLayers::Car.to_z()))
                    .with_rotation(Quat::from_rotation_z(73_f32.to_radians())),
            ),
            (
                MenuCarSpawner::new(),
                Transform::from_translation(Vec3::new(30., 80., SpriteLayers::Car.to_z()))
                    .with_rotation(Quat::from_rotation_z(-147_f32.to_radians())),
            ),
            (
                MenuCarSpawner::new(),
                Transform::from_translation(Vec3::new(40., 80., SpriteLayers::Car.to_z()))
                    .with_rotation(Quat::from_rotation_z(-147_f32.to_radians())),
            ),
        ])
        .id();
    let menu_background = commands
        .spawn((
            Sprite::from_image(handles.menu_background_texture.clone()),
            Transform::from_translation(Vec3::Z * SpriteLayers::Background.to_z()),
        ))
        .id();
    let buttons = commands
        .spawn((
            MenuControls,
            Node {
                position_type: PositionType::Absolute,
                bottom: vh(20),
                flex_direction: FlexDirection::Column,
                row_gap: px(30),
                align_items: AlignItems::Center,
                ..default()
            },
            children![
                (
                    animated_button_bundle(
                        AnimatedButton(0),
                        &handles,
                        handles.buttons_atlas.clone()
                    ),
                    observers!(|_trigger: On<Pointer<Press>>,
                                mut start_hosting: MessageWriter<StartHosting>| {
                        start_hosting.write(StartHosting);
                    }),
                ),
                (
                    Node {
                        row_gap: px(10),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    children![
                        (
                            TextInputNode {
                                mode: TextInputMode::SingleLine,
                                max_chars: Some(4),
                                clear_on_submit: false,
                                ..default()
                            },
                            TextFont {
                                font_size: 50.,
                                ..default()
                            },
                            TextInputContents::default(),
                            TextInputModifier::AllCaps,
                            TextInputFilter::Custom(Box::new(|c: &str| c
                                .chars()
                                .all(|c| c.is_ascii_uppercase()))),
                            Node {
                                height: px(64),
                                width: px(128),
                                ..default()
                            },
                            BackgroundColor(AppColors::Dark.color()),
                            CodeInput,
                        ),
                        (
                            animated_button_bundle(
                                AnimatedButton(2),
                                &handles,
                                handles.buttons_atlas.clone()
                            ),
                            observers!(
                            |_: On<Pointer<Press>>,
                             mut join_writer: MessageWriter<JoinWebrtcLobbyByCode>,
                             code: Single<&TextInputContents, With<CodeInput>>| {
                                join_writer.write(JoinWebrtcLobbyByCode(code.get().to_string()));
                            }
                        ),
                        ),
                    ]
                ),
            ],
        ))
        .id();
    let logo = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: vh(10),
                height: px(68. * 4.),
                width: px(128. * 4.),
                ..default()
            },
            ImageNode::from_atlas_image(
                handles.logo_texture.clone(),
                TextureAtlas::from(logo_atlas.clone()),
            ),
            AnimatedButton(0),
        ))
        .id();
    let name_parent = commands
        .spawn((
            MenuControls,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(15),
                column_gap: px(10),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            children![
                (
                    Button,
                    ImageNode::from_atlas_image(
                        handles.name_texture.clone(),
                        TextureAtlas::from(name_atlas.clone()),
                    ),
                    AnimatedButton(0),
                    Node {
                        height: px(8. * 4.),
                        width: px(32. * 4.),
                        ..default()
                    },
                ),
                (
                    TextInputNode {
                        mode: TextInputMode::SingleLine,
                        max_chars: Some(11),
                        clear_on_submit: false,
                        ..default()
                    },
                    TextInputBuffer::new(local_data.0.name.clone()),
                    TextInputContents::default(),
                    TextFont {
                        font_size: 28.,
                        ..default()
                    },
                    BackgroundColor(AppColors::Dark.color()),
                    Node {
                        height: px(34),
                        width: px(200),
                        ..default()
                    },
                    NameInput,
                )
            ],
        ))
        .id();
    commands.entity(menu).add_children(&[
        menu_background,
        buttons,
        name_parent,
        logo,
        car_spawners,
    ]);
}

/// Handle code input submission (join lobby by code).
fn handle_code_submit(
    mut submit_reader: MessageReader<SubmitText>,
    code_inputs: Query<Entity, With<CodeInput>>,
    mut join_writer: MessageWriter<JoinWebrtcLobbyByCode>,
) {
    for submit in submit_reader.read() {
        // Only act on submits from a CodeInput entity
        if code_inputs.get(submit.entity).is_ok() {
            join_writer.write(JoinWebrtcLobbyByCode(submit.text.to_string()));
        }
    }
}

/// Sync name input text to local player data.
fn handle_name_change(
    name_inputs: Query<&TextInputContents, (With<NameInput>, Changed<TextInputContents>)>,
    mut local_data: ResMut<LocalPlayerData>,
    mut commands: Commands,
    lobbies: Query<Entity, With<Lobby>>,
) {
    for contents in name_inputs.iter() {
        local_data.0.name = contents.get().to_string();
        if let Some(lobby) = lobbies.iter().next() {
            let data = local_data.0.clone();
            commands
                .entity(lobby)
                .trigger(move |entity| SetPlayerData::new(entity, data));
        }
    }
}

fn handle_spawning_menu_cars(
    time: Res<Time>,
    mut commands: Commands,
    mut spawners: Query<(&Transform, &mut MenuCarSpawner)>,
) {
    for (transform, mut spawner) in spawners.iter_mut() {
        if let Some(next_spawn) = spawner.next_spawn {
            if time.elapsed_secs() > next_spawn {
                commands.run_system_cached_with(
                    spawn_kart,
                    (KartControlType::AutoCar, transform.clone()),
                );
                spawner.next_spawn = None;
            }
        }
        if spawner.next_spawn.is_none() {
            spawner.next_spawn = Some(time.elapsed_secs() + rand::rng().random_range(1.0..5.0));
        }
    }
}

fn handle_despawning_menu_cars(
    mut commands: Commands,
    cars: Query<(Entity, &Transform), With<AutoCar>>,
) {
    for (entity, transform) in cars.iter() {
        if outside_of(transform.translation.xy(), RESOLUTION) {
            commands.entity(entity).despawn();
        }
    }
}

fn hide_controls_while_joining(
    pending: Query<(), With<PendingLobby>>,
    mut controls: Query<&mut Visibility, With<MenuControls>>,
) {
    let hidden = !pending.is_empty();
    for mut vis in controls.iter_mut() {
        *vis = if hidden {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
}

fn outside_of(pos: Vec2, zone: Vec2) -> bool {
    pos.x < -zone.x || pos.x > zone.x || pos.y < -zone.y || pos.y > zone.y
}
