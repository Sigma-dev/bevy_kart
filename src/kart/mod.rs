use crate::car_controller_2d::{
    BoostEffect, CarController2d, CarControllerDisabled, CarControllerInputs, SteeringState,
};
use crate::menu::lobby::{LobbyCar, LobbyCarName};
use crate::scene_util::insert;
use crate::track::LAPS_TO_WIN;
use crate::track::position::TrackPosition;
use crate::{
    AppPlayerData, AppState, AssetHandles, LocalPlayerData, Screen, SpriteLayers,
    car_controller_2d::CarController2dWheel, track::FinishTimes,
};
use audio_manager::prelude::*;
use avian2d::prelude::*;
use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy_ensemble::LobbyClientPlayerUuid;
use bevy_ensemble::prelude::*;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};
pub struct KartPlugin;

impl Plugin for KartPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_boost_flame).add_systems(
            PostUpdate,
            follow_transform
                // After the tick blend has written the kart's drawn pose.
                .after(TickedInterpolationSet)
                .before(TransformSystems::Propagate),
        );
    }
}

pub const KART_SIZE: UVec2 = UVec2::new(4, 8);
pub const KART_COLORS_COUNT: u32 = 10;

/// One frame of `sprites/boost.png`. The flame is drawn hanging from the top
/// edge of its cell, so the cell is anchored there and the art trails downwards.
pub const BOOST_FLAME_SIZE: UVec2 = UVec2::splat(8);
pub const BOOST_FLAME_FRAMES: u32 = 2;
/// Frames per second the two flame frames alternate at.
const BOOST_FLAME_FPS: f32 = 12.;
/// `sounds/boost.wav` is recorded far quieter than the rest of the set: about
/// 22 dB under `rocket.wav` by RMS, which the global 0.3 multiplier and the
/// spatial falloff then take the rest of the way to inaudible. This brings it
/// level with the rocket; normalising the file instead would let it go.
const BOOST_SOUND_GAIN: f32 = 12.;

#[derive(Component, Debug, Default)]
pub struct LapsCounter {
    pub count: i32,
    pub last_frame_progress: f32,
}

impl LapsCounter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, progress: f32) {
        if self.last_frame_progress > 0.95 && progress < 0.05 {
            self.count += 1;
        }
        if self.last_frame_progress < 0.05 && progress > 0.95 {
            self.count -= 1;
        }
        self.last_frame_progress = progress;
    }
}

#[derive(EntityEvent)]
pub struct LapUpdate {
    pub count: i32,
    pub entity: Entity,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct KartColor(pub u32);

impl KartColor {
    pub fn new() -> Self {
        Self(0)
    }

    pub fn right(&self) -> KartColor {
        Self((self.0 + 1) % KART_COLORS_COUNT)
    }

    pub fn left(&self) -> KartColor {
        if self.0 == 0 {
            return Self(KART_COLORS_COUNT - 1);
        }
        Self(self.0 - 1)
    }

    pub fn to_u32(&self) -> u32 {
        self.0
    }

    pub fn new_random() -> Self {
        Self(rand::rng().random_range(0..KART_COLORS_COUNT))
    }
}

/// Spawn the 4 wheel child entities for a kart.
pub fn spawn_kart_wheels(parent: &mut ChildSpawnerCommands, wheel_texture: Handle<Image>) {
    let half_car_width = 2.5;
    let half_car_length = 3.0;
    let positions = [
        (half_car_width, half_car_length - 1., true, true),
        (-half_car_width, half_car_length - 1., true, true),
        (half_car_width, -half_car_length, false, false),
        (-half_car_width, -half_car_length, false, false),
    ];
    for (x, y, powered, steerable) in positions {
        parent.spawn((
            Transform::from_xyz(x, y, SpriteLayers::Wheels.to_z()),
            CarController2dWheel::new(powered, steerable),
            Sprite::from_image(wheel_texture.clone()),
        ));
    }
}

/// The exhaust flame behind a kart. Spawned hidden with the kart and shown by
/// [`update_boost_flame`] for as long as the kart carries a [`BoostEffect`],
/// rather than spawned and despawned with the boost: the effect is a replicated
/// component that rollback adds and removes again on a client, and a visual
/// that lives across that churn cannot flicker with it.
#[derive(Component)]
pub struct BoostFlame;

/// Spawn the boost flame child of a kart, hidden until the kart boosts.
pub fn spawn_boost_flame(
    parent: &mut ChildSpawnerCommands,
    boost_flame_texture: Handle<Image>,
    boost_flame_atlas: Handle<TextureAtlasLayout>,
) {
    parent.spawn((
        BoostFlame,
        // At the rear bumper, with the sprite's top edge pinned there, so the
        // flame starts where the kart ends however tall the art is.
        Transform::from_xyz(0., -(KART_SIZE.y as f32) / 2., SpriteLayers::Wheels.to_z()),
        Anchor::TOP_CENTER,
        Sprite::from_atlas_image(
            boost_flame_texture,
            TextureAtlas {
                layout: boost_flame_atlas,
                index: 0,
            },
        ),
        Visibility::Hidden,
    ));
}

/// Show the flame while its kart is boosting, run its frames, and fire the
/// boost sound on the frame it lights up. Frame-time driven, like the rocket's:
/// it is a visual only, and nothing in the simulation reads it.
///
/// The sound rides the flame's own hidden -> shown edge rather than an
/// `Added<BoostEffect>` observer: `BoostEffect` is replicated, so rollback can
/// add it several times over for one boost, while this runs once a frame on
/// whatever state the tick loop settled on.
fn update_boost_flame(
    time: Res<Time>,
    mut audio_manager: AudioManager,
    boosting_karts: Query<(), With<BoostEffect>>,
    mut flames: Query<(&ChildOf, &mut Sprite, &mut Visibility), With<BoostFlame>>,
) {
    let frame = (time.elapsed_secs() * BOOST_FLAME_FPS) as usize % BOOST_FLAME_FRAMES as usize;
    for (child_of, mut sprite, mut visibility) in flames.iter_mut() {
        let boosting = boosting_karts.contains(child_of.parent());
        if boosting && *visibility == Visibility::Hidden {
            // On the kart, so every peer hears the boost from where it happens
            // and the local player's own is the loudest.
            audio_manager.play_sound(
                PlayAudio2D::new_once("sounds/boost.wav")
                    .with_volume(BOOST_SOUND_GAIN)
                    .with_spatial(SpatialSettings2D::Entity(child_of.parent())),
            );
        }
        *visibility = if boosting {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if let Some(atlas) = sprite.texture_atlas.as_mut() {
            atlas.index = frame;
        }
    }
}

/// Observer for LapUpdate: disable car and record finish time when lap target reached.
pub fn on_lap_update(
    trigger: On<LapUpdate>,
    tick: Res<CurrentTick>,
    karts: Query<&CarControllerDisabled>,
    mut commands: Commands,
    owners: Query<&Owner>,
    mut finish_times: ResMut<FinishTimes>,
) {
    if trigger.event().count == LAPS_TO_WIN as i32 {
        if karts.get(trigger.event_target()).is_ok() {
            return;
        }
        if let Ok(owner) = owners.get(trigger.event_target()) {
            commands
                .entity(trigger.event_target())
                .insert(CarControllerDisabled);
            finish_times.times.insert(owner.0, tick.0);
        }
    }
}

#[derive(Component)]
pub struct AutoCar;

pub enum KartControlType {
    Player(Owner),
    AutoCar,
    LobbyCar(u128, Option<usize>),
}

#[derive(Component)]
pub struct LocalKart;

/// Physics + control components for karts that actually drive around (race karts
/// and the animated menu `AutoCar`s). Lobby cars deliberately omit these: they are
/// pure visual puppets positioned directly via their `Transform` (see
/// [`update_lobby_cars`](crate::menu::lobby)), so giving them a `RigidBody` would
/// pull in avian's transform interpolation and fight the puppet positioning.
///
/// Takes the pose explicitly: `TickedAvianPlugin` keeps avian from reading a
/// `Transform` back into the body, and only places one from its `Transform`
/// when its `Position` is the default.
fn kart_physics(transform: &Transform) -> impl Bundle {
    (
        Mass(1.),
        RigidBody::Dynamic,
        Collider::rectangle(4., 8.),
        CarController2d::new(1.),
        SteeringState::default(),
        Position(transform.translation.xy()),
        Rotation::from(transform.rotation),
    )
}

pub(crate) fn spawn_kart(
    In((control_type, transform)): In<(KartControlType, Transform)>,
    mut commands: Commands,
    participants_with_data: Query<(&LobbyParticipant, Option<&PlayerData<AppPlayerData>>)>,
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    server_player: Option<Res<LocalServerPlayer>>,
    asset_handles: Res<AssetHandles>,
) {
    let wheel_tex = asset_handles.wheel_texture.clone();
    let id = commands
        .spawn((
            DespawnOnExit(AppState::Game),
            transform,
            Visibility::Inherited,
        ))
        .with_children(|parent| spawn_kart_wheels(parent, wheel_tex))
        .id();
    match control_type {
        KartControlType::Player(owner) => {
            let Some(player) = participants_with_data
                .iter()
                .find(|(p, _)| p.player_uuid == owner.0)
                .and_then(|(_, data)| data.map(|d| &d.0))
            else {
                commands.entity(id).despawn();
                return;
            };
            let is_local = local_player.as_ref().is_some_and(|p| p.0 == owner.0);
            commands.entity(id).insert((
                kart_physics(&transform),
                owner,
                CarControllerDisabled,
                LapsCounter::new(),
                Sprite::from_atlas_image(
                    asset_handles.karts_texture.clone(),
                    TextureAtlas {
                        layout: asset_handles.karts_atlas.clone(),
                        index: player.kart_color.to_u32() as usize,
                    },
                ),
                TrackPosition,
            ));
            commands.entity(id).observe(on_lap_update);

            let (flame_tex, flame_atlas) = (
                asset_handles.boost_flame_texture.clone(),
                asset_handles.boost_flame_atlas.clone(),
            );
            commands
                .entity(id)
                .with_children(|parent| spawn_boost_flame(parent, flame_tex, flame_atlas));

            if is_local {
                commands.entity(id).insert(LocalKart);
            }

            commands.spawn((
                DespawnOnExit(AppState::Game),
                FollowTransform(id),
                children![(
                    Text2d::new(player.name.clone()),
                    Transform::from_xyz(0., 5., SpriteLayers::AboveCar.to_z())
                        .with_scale(Vec3::splat(0.1)),
                )],
            ));
        }
        KartControlType::AutoCar => {
            commands.entity(id).insert((
                kart_physics(&transform),
                Sprite::from_atlas_image(
                    asset_handles.karts_texture.clone(),
                    TextureAtlas {
                        layout: asset_handles.karts_atlas.clone(),
                        index: rand::rng().random_range(0..KART_COLORS_COUNT) as usize,
                    },
                ),
                AutoCar,
                CarControllerInputs {
                    forward: true,
                    ..default()
                },
                DespawnOnExit(Screen::StartMenu),
            ));
        }
        KartControlType::LobbyCar(player_uuid, rank) => {
            let is_local = local_player.as_ref().is_some_and(|p| p.0 == player_uuid);
            let is_host = server_player.is_some();
            let player_data = participants_with_data
                .iter()
                .find(|(p, _)| p.player_uuid == player_uuid)
                .and_then(|(_, data)| data.map(|d| &d.0));
            let player_name = player_data.map(|d| d.name.as_str()).unwrap_or("...");
            let player_color = player_data
                .map(|d| d.kart_color.to_u32() as usize)
                .unwrap_or(0);
            let name = if is_local { "(YOU)\n" } else { "" }.to_string() + player_name;
            let name = rank
                .map(|r| format!("({})\n", r))
                .unwrap_or_default()
                .to_string()
                + &name;

            commands.entity(id).insert((
                Sprite::from_atlas_image(
                    asset_handles.karts_texture.clone(),
                    TextureAtlas {
                        layout: asset_handles.karts_atlas.clone(),
                        index: player_color,
                    },
                ),
                LobbyCar(player_uuid),
                DespawnOnExit(Screen::Lobby),
            ));

            let ui = commands
                .spawn_scene(bsn! {
                    {insert(FollowTransform(id))}
                    Visibility::Inherited
                    Children [
                        (
                            {insert((
                                Text2d::new(name),
                                Transform::from_xyz(0., 5., SpriteLayers::AboveCar.to_z())
                                    .with_scale(Vec3::splat(0.1)),
                            ))}
                            LobbyCarName({player_uuid})
                            TextLayout::justify(Justify::Center)
                        )
                    ]
                })
                .id();
            if is_local {
                let arrow = asset_handles.arrow_texture.clone();
                commands
                    .spawn_scene(bsn! {
                        {insert((
                            Transform::from_xyz(-6., 0., SpriteLayers::Car.to_z()),
                            Sprite { image: {arrow}, flip_x: true, ..default() },
                        ))}
                        Button
                        Pickable
                        on(|_: On<Pointer<Press>>,
                            mut local_data: ResMut<LocalPlayerData>,
                            mut commands: Commands,
                            lobbies: Query<Entity, With<Lobby>>| {
                            local_data.0.kart_color = local_data.0.kart_color.left();
                            if let Some(lobby) = lobbies.iter().next() {
                                let data = local_data.0.clone();
                                commands.entity(lobby).trigger(move |entity| SetPlayerData::new(entity, data));
                            }
                        })
                    })
                    .insert(ChildOf(ui));
                commands
                    .spawn_scene(bsn! {
                        {insert((
                            Transform::from_xyz(6., 0., SpriteLayers::Car.to_z()),
                            Sprite::from_image(asset_handles.arrow_texture.clone()),
                        ))}
                        Button
                        Pickable
                        on(|_: On<Pointer<Press>>,
                            mut local_data: ResMut<LocalPlayerData>,
                            mut commands: Commands,
                            lobbies: Query<Entity, With<Lobby>>| {
                            local_data.0.kart_color = local_data.0.kart_color.right();
                            if let Some(lobby) = lobbies.iter().next() {
                                let data = local_data.0.clone();
                                commands.entity(lobby).trigger(move |entity| SetPlayerData::new(entity, data));
                            }
                        })
                    })
                    .insert(ChildOf(ui));
            } else if is_host {
                let kick_uuid = player_uuid;
                commands
                    .spawn_scene(bsn! {
                        {insert((
                            Transform::from_xyz(0., -6., SpriteLayers::Car.to_z()),
                            Sprite::from_image(asset_handles.kick_texture.clone()),
                        ))}
                        Button
                        Pickable
                        on(move |_: On<Pointer<Press>>,
                            mut commands: Commands,
                            lobby_clients: Query<(Entity, &LobbyClientPlayerUuid)>| {
                            if let Some((entity, _)) = lobby_clients.iter().find(|(_, uuid)| uuid.0 == kick_uuid) {
                                commands.entity(entity).try_despawn();
                            }
                        })
                    })
                    .insert(ChildOf(ui));
            }
        }
    }
}

#[derive(Component, Clone)]
#[require(Transform)]
pub struct FollowTransform(pub Entity);

fn follow_transform(
    mut commands: Commands,
    transforms: Query<&Transform, Without<FollowTransform>>,
    mut follow_transforms: Query<(Entity, &mut Transform, &FollowTransform)>,
) {
    for (entity, mut transform, follow_transform) in follow_transforms.iter_mut() {
        if let Ok(target_transform) = transforms.get(follow_transform.0) {
            transform.translation = target_transform.translation;
        } else {
            commands.entity(entity).try_despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    //! The boost flame, headless: no window, no audio device, no network.
    use super::*;
    use audio_manager::AudioManagerResource;
    use bevy::asset::AssetPlugin;
    use bevy::audio::{AudioPlayer, AudioSource};

    /// A kart with a flame child, and just enough app to run
    /// [`update_boost_flame`]: assets for the sound to be loaded from, and the
    /// audio manager's resource for it to be played through.
    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<AudioSource>()
            .insert_resource(AudioManagerResource::new(1.))
            .add_systems(Update, update_boost_flame);
        let kart = app.world_mut().spawn(Transform::default()).id();
        app.world_mut().spawn((
            BoostFlame,
            Sprite::from_atlas_image(
                Handle::default(),
                TextureAtlas {
                    layout: Handle::default(),
                    index: 0,
                },
            ),
            Visibility::Hidden,
            ChildOf(kart),
        ));
        (app, kart)
    }

    fn sounds_playing(app: &mut App) -> usize {
        app.world_mut()
            .query_filtered::<Entity, With<AudioPlayer>>()
            .iter(app.world())
            .count()
    }

    fn flame_visibility(app: &mut App) -> Visibility {
        *app.world_mut()
            .query_filtered::<&Visibility, With<BoostFlame>>()
            .single(app.world())
            .unwrap()
    }

    #[test]
    fn the_flame_lights_and_the_sound_fires_once_for_one_boost() {
        let (mut app, kart) = app();
        app.update();
        assert_eq!(flame_visibility(&mut app), Visibility::Hidden);
        assert_eq!(sounds_playing(&mut app), 0);

        app.world_mut().entity_mut(kart).insert(BoostEffect {
            multiplier: 3.,
            remaining_ticks: 64,
        });
        app.update();
        assert_eq!(flame_visibility(&mut app), Visibility::Inherited);
        assert_eq!(sounds_playing(&mut app), 1);

        // Every later frame of the same boost is the flame already lit, not a
        // second one starting.
        app.update();
        app.update();
        assert_eq!(sounds_playing(&mut app), 1);

        app.world_mut().entity_mut(kart).remove::<BoostEffect>();
        app.update();
        assert_eq!(flame_visibility(&mut app), Visibility::Hidden);
    }
}
