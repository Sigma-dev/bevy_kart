use audio_manager::prelude::*;
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_easy_p2p::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    AppInstantiations, AppState, FinishTimes, KartEasyP2P, LAPS_TO_WIN, LapsCounter, SpriteLayers,
    car_controller_2d::CarControllerDisabled,
};

pub struct TrackPlugin;

impl Plugin for TrackPlugin {
    fn build(&self, app: &mut App) {
        app.init_networked_event::<OnFinishTimeUpdate>();
        app.add_systems(
            Update,
            (
                on_receive_finish_times,
                handle_end_race,
                end_with_delay,
                start_light,
            ),
        );
    }
}

#[derive(Component)]
struct HasPassedPostStart;

#[derive(Component)]
struct CanFinishLap;

#[derive(Message, Clone, Debug, Serialize, Deserialize)]
pub struct OnFinishTimeUpdate(FinishTimes);

#[derive(Resource)]
struct RaceEnded(f32);

#[derive(Component)]
struct StartLight;

#[derive(Resource)]
struct RaceStarted(f32);

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

pub(crate) fn spawn_track(
    mut finish_times: ResMut<FinishTimes>,
    time: Res<Time>,
    mut commands: Commands,
    mut audio_manager: AudioManager,
    asset_server: Res<AssetServer>,
    mut easy: KartEasyP2P,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    finish_times.times.clear();
    commands.spawn((
        DespawnOnExit(AppState::Game),
        Sprite::from_image(asset_server.load("sprites/track.png")),
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
    let texture = asset_server.load("sprites/start_light.png");
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
    if !easy.is_host() {
        return;
    }
    for (i, player) in easy.get_players().iter().enumerate() {
        let i = i as i32;
        let position: Vec3 = Vec3::new(
            (-25 + (i / 3) * -10) as f32,
            (-39 + (i % 3) * -7) as f32,
            SpriteLayers::Car.to_z(),
        );
        easy.instantiate(
            AppInstantiations::Kart(player.id.clone()),
            Transform::from_translation(position)
                .with_rotation(Quat::from_rotation_z(-90_f32.to_radians())),
        );
    }

    commands
        .spawn((
            DespawnOnExit(AppState::Game),
            Transform::from_translation(Vec3::new(-60., -47.5, 100.)),
            Collider::rectangle(10., 30.),
            Sensor,
            CollisionEventsEnabled,
        ))
        .observe(
            |trigger: On<CollisionStart>,
             mut commands: Commands,
             can_finish_lap: Query<Entity, With<HasPassedPostStart>>| {
                if let Ok(entity) = can_finish_lap.get(trigger.collider2) {
                    commands.entity(entity).insert(CanFinishLap);
                }
            },
        );

    commands
        .spawn((
            DespawnOnExit(AppState::Game),
            Transform::from_translation(Vec3::new(-18., -47.5, 0.)),
            Collider::rectangle(10., 30.),
            Sensor,
            CollisionEventsEnabled,
        ))
        .observe(
            |trigger: On<CollisionStart>,
             time: Res<Time>,
             mut car: Query<(Entity, &mut LapsCounter, Option<&CanFinishLap>)>,
             mut commands: Commands,
             mut finish_times: ResMut<FinishTimes>,
             easy: KartEasyP2P| {
                if let Ok((entity, mut lap_counter, maybe_can_finish_lap)) =
                    car.get_mut(trigger.collider2)
                {
                    commands.entity(entity).remove::<HasPassedPostStart>();
                    commands.entity(entity).remove::<CanFinishLap>();
                    if maybe_can_finish_lap.is_none() {
                        return;
                    }
                    lap_counter.0 += 1;
                    if lap_counter.0 == LAPS_TO_WIN {
                        commands.entity(entity).insert(CarControllerDisabled);
                        finish_times.times.insert(
                            easy.get_closest_networked_id(entity).unwrap().clone(),
                            time.elapsed_secs(),
                        );
                    }
                }
            },
        );

    commands
        .spawn((
            DespawnOnExit(AppState::Game),
            Transform::from_translation(Vec3::new(-2., -47.5, 0.)),
            Collider::rectangle(10., 30.),
            Sensor,
            CollisionEventsEnabled,
        ))
        .observe(
            |trigger: On<CollisionStart>,
             car: Query<Entity, With<LapsCounter>>,
             mut commands: Commands| {
                if let Ok(entity) = car.get(trigger.collider2) {
                    commands.entity(entity).insert(HasPassedPostStart);
                } else {
                }
            },
        );
}

fn on_receive_finish_times(mut commands: Commands, mut r: MessageReader<OnFinishTimeUpdate>) {
    for OnFinishTimeUpdate(finish_times) in r.read() {
        commands.insert_resource(finish_times.clone());
    }
}

fn handle_end_race(
    time: Res<Time>,
    mut commands: Commands,
    easy: KartEasyP2P,
    cars: Query<&LapsCounter>,
    mut events_w: MessageWriter<OnFinishTimeUpdate>,
    finish_times: Res<FinishTimes>,
    race_ended: Option<Res<RaceEnded>>,
) {
    if !easy.is_host() {
        return;
    }
    if cars.iter().count() == 0 || cars.iter().any(|car| car.0 != LAPS_TO_WIN) {
        return;
    }
    if race_ended.is_some() {
        return;
    }
    events_w.write(OnFinishTimeUpdate(finish_times.clone()));
    commands.insert_resource(RaceEnded(time.elapsed_secs()));
}

fn end_with_delay(
    mut commands: Commands,
    time: Res<Time>,
    race_ended: Option<Res<RaceEnded>>,
    easy: KartEasyP2P,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if !easy.is_host() {
        return;
    }
    let Some(race_ended) = race_ended else {
        return;
    };
    if time.elapsed_secs() - race_ended.0 < 3. {
        return;
    }
    next_state.set(AppState::OutOfGame);
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
