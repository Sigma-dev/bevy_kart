//! Putting the players on the starting grid.
//!
//! Host-only, because karts are `TickTrackedEntity`s and every other peer
//! receives them through snapshots. The grid itself is derived on every peer --
//! it comes out of the same builder as the walls -- but only the host reads it.

use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_ensemble::prelude::*;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;
use rand::seq::SliceRandom;

use crate::car_controller_2d::CarControllerDisabled;
use crate::track::map::build::BuiltTrack;
use crate::track::spawn::grid_slot;
use crate::{AppState, EntityKind, SpriteLayers};

pub(crate) fn spawn_starting_grid(
    mut spawner: TrackedSpawner,
    built: Res<BuiltTrack>,
    server_player: Option<Res<LocalServerPlayer>>,
    participants: Query<&LobbyParticipant>,
) {
    if server_player.is_none() {
        return;
    }
    let mut player_uuids: Vec<u128> = participants.iter().map(|p| p.player_uuid).collect();
    // Who gets pole is not the host's to decide.
    player_uuids.shuffle(&mut rand::rng());

    for (index, uuid) in player_uuids.iter().enumerate() {
        let (position, rotation) = grid_slot(&built, index);
        spawner.spawn((
            DespawnOnExit(AppState::Game),
            EntityKind::Kart,
            Owner(*uuid),
            Mass(1.),
            RigidBody::Dynamic,
            Collider::rectangle(4., 8.),
            // The pose goes to physics explicitly; `Transform` is only the view.
            // `TickedAvianPlugin` keeps avian from reading it back, and places a
            // body from its `Transform` only when its `Position` is the default.
            Position(position),
            rotation,
            Transform::from_translation(position.extend(SpriteLayers::Car.to_z()))
                .with_rotation(Quat::from_rotation_z(rotation.as_radians())),
            CarControllerDisabled,
        ));
    }
}
