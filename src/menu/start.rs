use crate::scene_util::insert;
use crate::{
    AppColors, AssetHandles, LocalPlayerData, RESOLUTION, Screen, SpriteLayers,
    kart::{AutoCar, KartControlType, spawn_kart},
    menu::animated_button,
};
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter};
use bevy_ensemble::prelude::*;
use bevy_ensemble_webrtc::JoinWebrtcLobbyByCode;
use rand::Rng;

use super::{AnimatedButton, TextSubmit};

pub struct StartPlugin;

impl Plugin for StartPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastJoinFailure>().add_systems(
            Update,
            (
                handle_spawning_menu_cars,
                handle_despawning_menu_cars,
                handle_code_submit,
                handle_name_change,
                show_joining_controls,
                report_join_failure,
            ),
        );
    }
}

#[derive(Component, Default, Clone)]
struct MenuCarSpawner {
    next_spawn: Option<f32>,
}

#[derive(Component, Default, Clone)]
struct MenuControls;

/// The controls that replace [`MenuControls`] while a session is being opened.
///
/// Hiding the menu's buttons during a join was right; hiding them and offering nothing was not.
/// A join that cannot complete used to leave the player looking at a menu with no controls at
/// all, and the only way out of it was to restart the game.
#[derive(Component, Default, Clone)]
struct JoiningPanel;

/// Where the reason a session ended is shown.
#[derive(Component, Default, Clone)]
struct JoinErrorText;

/// The last reason a session failed to open, held so the text survives the panel.
///
/// A resource rather than only the text node, because the panel above disappears with the lobby
/// that caused it, and the menu is rebuilt on some of the paths that get here. A message that
/// vanishes along with the thing that caused it is not a message.
#[derive(Resource, Default)]
struct LastJoinFailure(Option<String>);

#[derive(Component, Default, Clone)]
struct CodeInput;

#[derive(Component, Default, Clone)]
struct NameInput;

/// One repeating menu-car spawn point (position + facing), as a [`Scene`].
fn menu_car_spawner(x: f32, y: f32, angle_deg: f32) -> impl Scene {
    bsn! {
        {insert(
            Transform::from_translation(Vec3::new(x, y, SpriteLayers::Car.to_z()))
                .with_rotation(Quat::from_rotation_z(angle_deg.to_radians()))
        )}
        MenuCarSpawner
    }
}

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
    let player_name = local_data.0.name.clone();
    commands.spawn_scene(bsn! {
        {insert(DespawnOnExit(Screen::StartMenu))}
        Node {
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        Children [
            // Full-screen background.
            (
                {insert((
                    Sprite::from_image(handles.menu_background_texture.clone()),
                    Transform::from_translation(Vec3::Z * SpriteLayers::Background.to_z()),
                ))}
            ),
            // Host button + join-by-code row.
            (
                MenuControls
                Node {
                    position_type: PositionType::Absolute,
                    bottom: vh(20),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(30),
                    align_items: AlignItems::Center,
                }
                Children [
                    (
                        animated_button(0, handles.buttons_texture.clone(), handles.buttons_atlas.clone())
                        on(|_: On<Pointer<Press>>, mut start_hosting: MessageWriter<StartHosting>| {
                            start_hosting.write(StartHosting);
                        })
                    ),
                    (
                        Node { row_gap: px(10), align_items: AlignItems::Center }
                        Children [
                            (
                                EditableText { max_characters: {Some(4)}, allow_newlines: false }
                                // Codes are A-Z; the server uppercases on join, so accept any letter.
                                EditableTextFilter::new(|c: char| c.is_ascii_alphabetic())
                                TextFont { font_size: {FontSize::Px(50.)} }
                                Node { height: px(64), width: px(128) }
                                BackgroundColor({AppColors::Dark.color()})
                                CodeInput
                            ),
                            (
                                animated_button(2, handles.buttons_texture.clone(), handles.buttons_atlas.clone())
                                on(|_: On<Pointer<Press>>,
                                    mut join_writer: MessageWriter<JoinWebrtcLobbyByCode>,
                                    code: Single<&EditableText, With<CodeInput>>| {
                                    join_writer.write(JoinWebrtcLobbyByCode(code.value().to_string()));
                                })
                            ),
                        ]
                    ),
                ]
            ),
            // Name label + editable name field.
            (
                MenuControls
                Node {
                    position_type: PositionType::Absolute,
                    bottom: px(15),
                    column_gap: px(10),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                }
                Children [
                    (
                        {insert(ImageNode::from_atlas_image(handles.name_texture.clone(), TextureAtlas::from(name_atlas)))}
                        Button
                        AnimatedButton(0)
                        Node { height: px(8. * 4.), width: px(32. * 4.) }
                    ),
                    (
                        EditableText::new(player_name)
                        EditableText { max_characters: {Some(11)}, allow_newlines: false }
                        TextFont { font_size: {FontSize::Px(28.)} }
                        BackgroundColor({AppColors::Dark.color()})
                        Node { height: px(34), width: px(200) }
                        NameInput
                    ),
                ]
            ),
            // Shown only while a join or a host request is in flight.
            (
                JoiningPanel
                Visibility::Hidden
                Node {
                    position_type: PositionType::Absolute,
                    bottom: vh(20),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(20),
                    align_items: AlignItems::Center,
                }
                Children [
                    (
                        Text::new("Connecting...")
                        TextFont { font_size: {FontSize::Px(30.)} }
                    ),
                    (
                        animated_button(6, handles.buttons_texture.clone(), handles.buttons_atlas.clone())
                        // Despawning the lobby is the whole cancel: its removal tells the
                        // signalling server, rebuilds the socket and releases the role, which is
                        // the same path the leave button takes out of a lobby that did open.
                        on(|_: On<Pointer<Press>>,
                            mut commands: Commands,
                            lobbies: Query<Entity, Or<(With<Lobby>, With<PendingLobby>)>>| {
                            for lobby in lobbies.iter() {
                                commands.entity(lobby).despawn();
                            }
                        })
                    ),
                ]
            ),
            // Why the last attempt ended. Empty until one does.
            (
                JoinErrorText
                Text::new("")
                TextFont { font_size: {FontSize::Px(22.)} }
                Node {
                    position_type: PositionType::Absolute,
                    bottom: vh(12),
                }
            ),
            // Logo.
            (
                {insert(ImageNode::from_atlas_image(handles.logo_texture.clone(), TextureAtlas::from(logo_atlas)))}
                Node {
                    position_type: PositionType::Absolute,
                    top: vh(10),
                    height: px(68. * 4.),
                    width: px(128. * 4.),
                }
                AnimatedButton(0)
            ),
            // Off-screen car spawn points that periodically launch drifting cars.
            (
                // Transform/Visibility so the transform-bearing children propagate
                // cleanly (avoids the B0004 hierarchy warning).
                {insert((Transform::default(), Visibility::default()))}
                Children [
                    menu_car_spawner(-134., -6., -53.),
                    menu_car_spawner(-142., 0., -53.),
                    menu_car_spawner(10., -80., 72.),
                    menu_car_spawner(20., -80., 73.),
                    menu_car_spawner(30., 80., -147.),
                    menu_car_spawner(40., 80., -147.),
                ]
            ),
        ]
    });
}

/// Handle code input submission (join lobby by code).
fn handle_code_submit(
    mut submit_reader: MessageReader<TextSubmit>,
    code_inputs: Query<Entity, With<CodeInput>>,
    mut join_writer: MessageWriter<JoinWebrtcLobbyByCode>,
) {
    for submit in submit_reader.read() {
        // Only act on submits from a CodeInput entity
        if code_inputs.get(submit.entity).is_ok() {
            join_writer.write(JoinWebrtcLobbyByCode(submit.text.clone()));
        }
    }
}

/// Sync name input text to local player data.
fn handle_name_change(
    name_inputs: Query<&EditableText, (With<NameInput>, Changed<EditableText>)>,
    mut local_data: ResMut<LocalPlayerData>,
    mut commands: Commands,
    lobbies: Query<Entity, With<Lobby>>,
) {
    for contents in name_inputs.iter() {
        local_data.0.name = contents.value().to_string();
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

/// Swap the menu's controls for the joining panel while a session is being opened.
fn show_joining_controls(
    pending: Query<(), With<PendingLobby>>,
    mut controls: Query<&mut Visibility, (With<MenuControls>, Without<JoiningPanel>)>,
    mut joining: Query<&mut Visibility, (With<JoiningPanel>, Without<MenuControls>)>,
) {
    let opening = !pending.is_empty();
    for mut vis in controls.iter_mut() {
        *vis = if opening {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    for mut vis in joining.iter_mut() {
        *vis = if opening {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// Say why the last attempt to open a session ended, until another one is made.
///
/// The backend has already torn the lobby down by the time [`LobbyJoinFailed`] arrives — it is
/// telling us why, not asking what to do — so there is nothing to undo here, only something to
/// put on the screen. Cleared when a new attempt starts, because the old reason is then wrong.
fn report_join_failure(
    mut failures: MessageReader<LobbyJoinFailed>,
    started: Query<(), Added<PendingLobby>>,
    mut last: ResMut<LastJoinFailure>,
    mut texts: Query<&mut Text, With<JoinErrorText>>,
) {
    if !started.is_empty() {
        last.0 = None;
    }
    if let Some(failure) = failures.read().last() {
        warn!("session could not be opened: {}", failure.reason);
        last.0 = Some(failure.reason.clone());
    }
    let wanted = last.0.as_deref().unwrap_or("");
    for mut text in texts.iter_mut() {
        if text.0 != wanted {
            *text = Text::new(wanted);
        }
    }
}

fn outside_of(pos: Vec2, zone: Vec2) -> bool {
    pos.x < -zone.x || pos.x > zone.x || pos.y < -zone.y || pos.y > zone.y
}
