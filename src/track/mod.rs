use crate::{
    AppState, AssetHandles, EntityKind, FinishTimes, OwnerPlayer, SpriteLayers,
    car_controller_2d::CarControllerDisabled,
    items::{ItemPickedUp, spawn_spawner},
    kart::{LapsCounter, LocalKart},
    track::position::{RacePosition, RacePositionPlugin, progress_line::ProgressLine},
};
use audio_manager::prelude::*;
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_ensemble::prelude::*;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

pub(crate) mod position;

pub struct TrackPlugin;

impl Plugin for TrackPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RacePositionPlugin);
        app.add_systems(
            Update,
            (
                on_receive_finish_times,
                handle_end_race.run_if(in_state(AppState::Game)),
                end_with_delay,
                start_light,
                update_held_item_icon,
                update_on_item_used,
                update_position_ui,
            ),
        );
    }
}

pub const LAPS_TO_WIN: u32 = 3;

/// Ensemble message: finish times update.
#[derive(Clone, Debug, Serialize, Deserialize, Message)]
pub struct OnFinishTimeUpdate(pub FinishTimes);


#[derive(Resource)]
struct RaceEnded(f32);

#[derive(Component)]
struct StartLight;

#[derive(Resource)]
struct RaceStarted(f32);

#[derive(Component)]
struct HeldItemIcon;

#[derive(Component)]
struct PositionUI;

fn spawn_barriers(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    red_material: Handle<ColorMaterial>,
    white_material: Handle<ColorMaterial>,
    points: Vec<Vec2>,
) {
    for (i, (a, b)) in points.iter().zip(points.iter().cycle().skip(1)).enumerate() {
        let material = if i % 2 == 0 {
            red_material.clone()
        } else {
            white_material.clone()
        };
        let middle = (*a + *b) / 2.;
        let length = (*a - *b).length();
        let angle = (*a - *b).y.atan2((*a - *b).x);
        commands.spawn((
            DespawnOnExit(AppState::Game),
            RigidBody::Static,
            Mesh2d(meshes.add(Rectangle::new(length, 2.))),
            MeshMaterial2d(material.clone()),
            Collider::rectangle(length, 2.),
            Transform::from_translation(middle.extend(SpriteLayers::Car.to_z()))
                .with_rotation(Quat::from_rotation_z(angle)),
        ));
    }
}

fn spawn_item_spawners(commands: &mut Commands, points: Vec<Vec2>) {
    for point in points {
        spawn_spawner(commands, point);
    }
}

pub(crate) fn spawn_track(
    mut finish_times: ResMut<FinishTimes>,
    time: Res<Time>,
    mut commands: Commands,
    mut audio_manager: AudioManager,
    asset_handles: Res<AssetHandles>,
    server_player: Option<Res<LocalServerPlayer>>,
    participants: Query<&LobbyParticipant>,
    mut counter: ResMut<TickTrackedEntityCounter>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    finish_times.times.clear();
    commands.remove_resource::<RaceEnded>();
    commands.spawn((
        DespawnOnExit(AppState::Game),
        Sprite::from_image(asset_handles.track_texture.clone()),
    ));
    let red_material = materials.add(ColorMaterial::from(Color::srgb(0.68, 0.13, 0.20)));
    let white_material = materials.add(ColorMaterial::from(Color::srgb(1., 1., 1.)));
    let outer_ring = vec![
        Vec2::new(-97., -61.5),
        Vec2::new(33., -57.2),
        Vec2::new(48.2, -47.4),
        Vec2::new(55.7, -38.),
        Vec2::new(61.6, -26.),
        Vec2::new(66., -25.6),
        Vec2::new(76.6, -45.8),
        Vec2::new(86., -54.),
        Vec2::new(99.8, -54.2),
        Vec2::new(106.5, -51.4),
        Vec2::new(114.6, -43.4),
        Vec2::new(119.2, -27.),
        Vec2::new(119.6, 4.2),
        Vec2::new(115.4, 48.),
        Vec2::new(110.6, 57.4),
        Vec2::new(100.2, 63.2),
        Vec2::new(87.8, 63.6),
        Vec2::new(69.3, 53.),
        Vec2::new(53.4, 41.6),
        Vec2::new(14., 0.),
        Vec2::new(7., -1.4),
        Vec2::new(0.1, -5.5),
        Vec2::new(-54.2, -10.6),
        Vec2::new(-63., -6.5),
        Vec2::new(-59.6, -0.2),
        Vec2::new(-35.1, 0.2),
        Vec2::new(-9.6, 2.4),
        Vec2::new(9.8, 11.),
        Vec2::new(23.2, 22.6),
        Vec2::new(27., 31.2),
        Vec2::new(27., 39.),
        Vec2::new(13.8, 54.6),
        Vec2::new(-10., 60.),
        Vec2::new(-47.2, 58.2),
        Vec2::new(-90., 57.8),
        Vec2::new(-106.6, 50.),
        Vec2::new(-119.6, 37.2),
        Vec2::new(-124., 26.2),
        Vec2::new(-123.8, -34.8),
        Vec2::new(-120.6, -45.6),
        Vec2::new(-109., -57.8),
    ];
    let inner_ring = vec![
        Vec2::new(-92., -37.8),
        Vec2::new(-50.6, -35.2),
        Vec2::new(15.8, -33.2),
        Vec2::new(31.8, -32.),
        Vec2::new(41.8, -13.8),
        Vec2::new(54.4, -1.4),
        Vec2::new(72.4, -1.6),
        Vec2::new(85.4, -11.8),
        Vec2::new(94.5, -29.),
        Vec2::new(95.4, -1.4),
        Vec2::new(92.4, 20.6),
        Vec2::new(93.6, 36.4),
        Vec2::new(89.4, 40.),
        Vec2::new(83., 33.8),
        Vec2::new(62.6, 17.8),
        Vec2::new(26.2, -21.),
        Vec2::new(5.6, -29.8),
        Vec2::new(-20.2, -31.2),
        Vec2::new(-77.6, -30.8),
        Vec2::new(-84.7, -25.8),
        Vec2::new(-90.2, -18.8),
        Vec2::new(-90.4, 5.),
        Vec2::new(-84.6, 14.8),
        Vec2::new(-69.8, 23.6),
        Vec2::new(-26.1, 25.2),
        Vec2::new(-13., 28.),
        Vec2::new(-0.8, 31.4),
        Vec2::new(-0.2, 34.8),
        Vec2::new(-27.4, 35.6),
        Vec2::new(-87.2, 33.),
        Vec2::new(-98.6, 25.8),
        Vec2::new(-100.6, -22.2),
        Vec2::new(-98.4, -35.2),
    ];
    commands.spawn((
        DespawnOnExit(AppState::Game),
        ProgressLine::new(vec![
            Vec2::new(-16.343735, -46.21621),
            Vec2::new(32.428055, -46.21621),
            Vec2::new(57.332794, -16.378376),
            Vec2::new(75.80382, -19.783785),
            Vec2::new(97.28415, -42.48648),
            Vec2::new(107.86867, 2.4324331),
            Vec2::new(99.35954, 47.837837),
            Vec2::new(89.086334, 52.216213),
            Vec2::new(10.3250885, -15.567566),
            Vec2::new(-72.17187, -17.351353),
            Vec2::new(-74.76611, 6.162163),
            Vec2::new(4.825287, 19.945944),
            Vec2::new(12.919342, 42.64865),
            Vec2::new(-9.079857, 48.486485),
            Vec2::new(-101.2274, 42.486485),
            Vec2::new(-111.29307, 20.756758),
            Vec2::new(-104.75557, -47.35135),
        ]),
    ));
    spawn_barriers(
        &mut commands,
        &mut meshes,
        red_material.clone(),
        white_material.clone(),
        outer_ring,
    );
    spawn_barriers(
        &mut commands,
        &mut meshes,
        red_material.clone(),
        white_material.clone(),
        inner_ring,
    );
    let texture = asset_handles.traffic_light_texture.clone();
    let layout = TextureAtlasLayout::from_grid(UVec2::new(15, 7), 5, 1, None, None);
    let texture_atlas_layout = texture_atlas_layouts.add(layout);
    commands.spawn((
        DespawnOnExit(AppState::Game),
        Transform::from_translation(Vec3::new(-28., -64., SpriteLayers::Car.to_z())),
        Sprite::from_atlas_image(
            texture,
            TextureAtlas {
                layout: texture_atlas_layout,
                index: 1,
            },
        ),
        StartLight,
    ));
    audio_manager.play_sound(PlayAudio2D::new_once("sounds/countdown.wav"));
    commands.insert_resource(RaceStarted(time.elapsed_secs()));

    let texture_handle = asset_handles.items_texture.clone();
    let texture_atlas = TextureAtlasLayout::from_grid(UVec2::splat(8), 2, 1, None, None);
    let texture_atlas_handle = texture_atlas_layouts.add(texture_atlas);

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(5.),
            bottom: Val::Px(5.),
            column_gap: px(8),
            ..default()
        },
        DespawnOnExit(AppState::Game),
        children![
            (PositionUI),
            (
                Node {
                    height: Val::Px(100.),
                    width: Val::Px(100.),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.18039, 0.13333, 0.18431)),
                children![
                    ((
                        ImageNode::from_atlas_image(
                            texture_handle,
                            TextureAtlas::from(texture_atlas_handle)
                        ),
                        Node {
                            height: Val::Px(80.),
                            width: Val::Px(80.),
                            ..default()
                        },
                        Visibility::Hidden,
                        HeldItemIcon,
                    ))
                ],
            )
        ],
    ));
    if server_player.is_none() {
        return;
    }
    let mut player_uuids: Vec<u128> = participants
        .iter()
        .map(|p| p.player_uuid)
        .collect();
    player_uuids.shuffle(&mut rand::rng());
    for (i, uuid) in player_uuids.iter().enumerate() {
        let i = i as i32;
        let position: Vec3 = Vec3::new(
            (-25 + (i / 3) * -10) as f32,
            (-39 + (i % 3) * -7) as f32,
            SpriteLayers::Car.to_z(),
        );
        let tracked_id = counter.next();
        commands.spawn((
            DespawnOnExit(AppState::Game),
            tracked_id,
            EntityKind::Kart,
            OwnerPlayer(*uuid),
            Mass(1.),
            RigidBody::Dynamic,
            Collider::rectangle(4., 8.),
            Transform::from_translation(position)
                .with_rotation(Quat::from_rotation_z(-90_f32.to_radians())),
            CarControllerDisabled,
        ));
    }

    spawn_item_spawners(
        &mut commands,
        vec![
            Vec2::new(0., -52.),
            Vec2::new(-0., -39.8),
            Vec2::new(101., 3.8),
            Vec2::new(110., 3.4),
            Vec2::new(-81., -5.8),
            Vec2::new(-68.8, -5.7),
            Vec2::new(-117.2, 21.1),
            Vec2::new(-106.66667, 20.86859),
        ],
    );
}

fn on_receive_finish_times(
    mut commands: Commands,
    mut reader: MessageReader<ReceivedEnsembleMessage<OnFinishTimeUpdate>>,
) {
    for msg in reader.read() {
        commands.insert_resource(msg.message.0.clone());
    }
}

fn handle_end_race(
    time: Res<Time>,
    mut commands: Commands,
    input: Res<ButtonInput<KeyCode>>,
    server_player: Option<Res<LocalServerPlayer>>,
    cars: Query<&LapsCounter>,
    lobbies: Query<Entity, With<Lobby>>,
    finish_times: Res<FinishTimes>,
    race_ended: Option<Res<RaceEnded>>,
) {
    if server_player.is_none() {
        return;
    }
    let car_counts: Vec<i32> = cars.iter().map(|c| c.count).collect();
    let race_not_over =
        car_counts.is_empty() || car_counts.iter().any(|&c| c < LAPS_TO_WIN as i32);
    let cheat = input.pressed(KeyCode::KeyU) && input.pressed(KeyCode::KeyK);
    if race_not_over && !cheat {
        return;
    }
    if race_ended.is_some() {
        return;
    }
    info!("Race ended! car_counts={:?}, cheat={}", car_counts, cheat);
    if let Some(lobby) = lobbies.iter().next() {
        let msg = OnFinishTimeUpdate(finish_times.clone());
        commands
            .entity(lobby)
            .trigger(move |entity| BroadcastLobbyMessage::new(entity, msg));
    }
    commands.insert_resource(RaceEnded(time.elapsed_secs()));
}

fn end_with_delay(
    mut commands: Commands,
    time: Res<Time>,
    race_ended: Option<Res<RaceEnded>>,
    server_player: Option<Res<LocalServerPlayer>>,
    mut next_state: ResMut<NextState<AppState>>,
    lobbies: Query<Entity, With<Lobby>>,
) {
    if server_player.is_none() {
        return;
    }
    let Some(race_ended) = race_ended else {
        return;
    };
    let elapsed = time.elapsed_secs() - race_ended.0;
    if elapsed < 3. {
        return;
    }
    info!("end_with_delay: transitioning to OutOfGame (waited {:.1}s)", elapsed);
    next_state.set(AppState::OutOfGame);
    if let Some(lobby) = lobbies.iter().next() {
        let msg = crate::GameStateChanged(crate::AppState::OutOfGame);
        commands
            .entity(lobby)
            .trigger(move |e| BroadcastLobbyMessage::new(e, msg));
    }
    commands.remove_resource::<RaceEnded>();
}

fn start_light(
    mut commands: Commands,
    time: Res<Time>,
    mut lights: Query<&mut Sprite, With<StartLight>>,
    race_started: Option<Res<RaceStarted>>,
    disabled_cars: Query<Entity, With<CarControllerDisabled>>,
) {
    let Some(race) = race_started else {
        return;
    };
    let time_since_start = time.elapsed_secs() - race.0;
    for mut light in lights.iter_mut() {
        let Some(texture_atlas) = &mut light.texture_atlas else {
            continue;
        };
        let new_index = time_since_start.floor() as usize + 1;
        if new_index > 4 {
            continue;
        }
        texture_atlas.index = new_index;
    }
    if time_since_start > 3. && time_since_start < 4. {
        for entity in disabled_cars.iter() {
            commands.entity(entity).remove::<CarControllerDisabled>();
        }
    }
}

fn update_held_item_icon(
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    local_server: Option<Res<LocalServerPlayer>>,
    mut held_item_icon: Query<(&mut Visibility, &mut ImageNode), With<HeldItemIcon>>,
    mut pickup_reader: MessageReader<ReceivedEnsembleMessage<ItemPickedUp>>,
) {
    let local_uuid = local_player
        .as_ref()
        .map(|p| p.0)
        .or_else(|| local_server.as_ref().map(|p| p.0));
    for msg in pickup_reader.read() {
        let picked_up = &msg.message;
        if !local_uuid.is_some_and(|uuid| uuid == picked_up.car_uuid) {
            continue;
        }
        for (mut visibility, mut image_node) in held_item_icon.iter_mut() {
            *visibility = Visibility::Visible;
            if let Some(atlas) = image_node.texture_atlas.as_mut() {
                atlas.index = picked_up.item.to_index();
            }
        }
    }
}

fn update_on_item_used(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut held_item_icon: Query<&mut Visibility, With<HeldItemIcon>>,
) {
    if keyboard.just_pressed(KeyCode::Space) {
        for mut visibility in held_item_icon.iter_mut() {
            *visibility = Visibility::Hidden;
        }
    }
}

fn update_position_ui(
    mut commands: Commands,
    asset_handles: Res<AssetHandles>,
    counter: Single<Entity, With<PositionUI>>,
    local_kart: Single<&RacePosition, With<LocalKart>>,
) {
    let position = local_kart.position + 1;
    commands
        .entity(*counter)
        .insert(Node {
            height: px(100),
            ..default()
        })
        .despawn_children();

    let tens_part = position / 10;
    if tens_part > 0 {
        commands
            .entity(*counter)
            .with_child((ImageNode::from_atlas_image(
                asset_handles.numbers_texture.clone(),
                TextureAtlas {
                    layout: asset_handles.numbers_atlas.clone(),
                    index: tens_part as usize,
                },
            ),));
    }
    commands
        .entity(*counter)
        .with_child((ImageNode::from_atlas_image(
            asset_handles.numbers_texture.clone(),
            TextureAtlas {
                layout: asset_handles.numbers_atlas.clone(),
                index: (position % 10) as usize,
            },
        ),));
}
