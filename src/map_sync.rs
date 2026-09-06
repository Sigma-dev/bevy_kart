//! Getting the host's chosen map onto every peer.
//!
//! A client may never have seen the map it is about to race -- the host can have
//! authored it five minutes ago -- so the map travels as data, not as a name.
//!
//! # Why the map is inside the start message
//!
//! The obvious design is two messages: "the map is now X", then "start". It does
//! not work. `bevy_ensemble_sockets`' native send is
//! `runtime_handle.spawn(async move { dc.send(&bytes).await })` -- one task per
//! message on a multi-threaded tokio runtime -- so two consecutive sends race to
//! reach the channel. The channel itself is ordered and reliable; the sends do
//! not arrive at it in call order. On wasm the same call is synchronous. So the
//! two-message design would work perfectly in a browser and corrupt
//! intermittently on native, which is the worst failure profile available.
//!
//! [`StartRace`] therefore carries the whole map. One packet, one atomic fact,
//! and no ordering to get wrong.
//!
//! # Why not a networked ticked resource
//!
//! `bevy_ticked_networking` will replicate a resource for you, but
//! `TickedResourceRegistry::serialize_all` runs unconditionally inside
//! `build_snapshot` and `ResourceActions` keeps a per-tick clone history for
//! rollback. A few-KB map at 64 Hz to every client is about a megabyte a second
//! of pure waste, plus a deep clone every tick, for a value that changes a few
//! times per lobby.

use bevy::prelude::*;
use bevy_ensemble::LobbyClientMessage;
use bevy_ensemble::prelude::*;
use bevy_ticked_networking::prelude::*;
use serde::{Deserialize, Serialize};

use crate::track::SelectedMap;
use crate::track::map::MapData;
use crate::{AppState, Screen};

pub struct MapSyncPlugin;

impl Plugin for MapSyncPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                announce_selected_map,
                send_map_to_new_clients,
                receive_map_selected,
                receive_start_race,
            ),
        );
    }
}

/// Host to everyone: the map now selected in the lobby. A preview, so the
/// picker and its clients agree before anyone presses start.
#[derive(Clone, Debug, Serialize, Deserialize, Message)]
pub struct MapSelected(pub MapData);

/// Host to everyone: the race begins, on exactly this map.
///
/// Carries the map rather than referring to one already sent. See the module
/// note: two messages cannot be relied on to arrive in the order they were sent.
#[derive(Clone, Debug, Serialize, Deserialize, Message)]
pub struct StartRace(pub MapData);

/// A cheap, stable identity for a map's contents.
///
/// FNV-1a over the postcard encoding. Every peer logs the hash of the map it is
/// about to race, so "these two peers disagree about the track" is one `grep`
/// rather than an afternoon -- and it costs eight bytes. Postcard rather than
/// the JSON form because it has one representation per value; the coordinates
/// being integers is what makes that true.
pub fn map_hash(map: &MapData) -> u64 {
    let bytes = postcard::to_allocvec(map).unwrap_or_default();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Enter the race, on this map, on this peer.
///
/// The map and the state change happen in one place because they are one fact.
/// Nothing can order them wrongly because there is nothing to order.
pub fn begin_race(
    commands: &mut Commands,
    next_state: &mut NextState<AppState>,
    map: MapData,
) {
    commands.insert_resource(SelectedMap(map));
    next_state.set(AppState::Game);
}

/// Host: tell everyone when the selection changes.
///
/// The broadcast comes back to the host as well, which is deliberate -- host and
/// client then take the same path through [`receive_map_selected`].
fn announce_selected_map(
    mut commands: Commands,
    selected: Res<SelectedMap>,
    server_player: Option<Res<LocalServerPlayer>>,
    lobbies: Query<Entity, (With<Lobby>, With<Host>)>,
) {
    if !selected.is_changed() || server_player.is_none() {
        return;
    }
    let Ok(lobby) = lobbies.single() else { return };
    let message = MapSelected(selected.0.clone());
    commands
        .entity(lobby)
        .trigger(move |entity| BroadcastLobbyMessage::new(entity, message));
}

/// Host: catch a peer up on the way in.
///
/// Targeted rather than broadcast, so a fifth joiner does not re-send the map to
/// the four already holding it. If a race is already running it sends
/// [`StartRace`] instead, which is what puts a mid-race joiner into the same
/// world as everybody else -- until now it sat on the lobby screen taking
/// snapshots of a race it had no track for.
fn send_map_to_new_clients(
    mut commands: Commands,
    selected: Res<SelectedMap>,
    app_state: Res<State<AppState>>,
    server_player: Option<Res<LocalServerPlayer>>,
    new_clients: Query<Entity, (With<LobbyClient>, Added<LobbyClient>)>,
) {
    if server_player.is_none() {
        return;
    }
    let racing = *app_state.get() == AppState::Game;
    for client in new_clients.iter() {
        let map = selected.0.clone();
        if racing {
            commands
                .entity(client)
                .trigger(move |entity| LobbyClientMessage::new(entity, StartRace(map)));
        } else {
            commands
                .entity(client)
                .trigger(move |entity| LobbyClientMessage::new(entity, MapSelected(map)));
        }
    }
}

fn receive_map_selected(
    mut commands: Commands,
    server_player: Option<Res<LocalServerPlayer>>,
    mut reader: MessageReader<ReceivedEnsembleMessage<MapSelected>>,
) {
    for message in reader.read() {
        // The host is the one that said it.
        if server_player.is_some() {
            continue;
        }
        commands.insert_resource(SelectedMap(message.message.0.clone()));
    }
}

fn receive_start_race(
    mut commands: Commands,
    server_player: Option<Res<LocalServerPlayer>>,
    mut next_state: ResMut<NextState<AppState>>,
    mut reader: MessageReader<ReceivedEnsembleMessage<StartRace>>,
) {
    for message in reader.read() {
        if server_player.is_some() {
            continue;
        }
        begin_race(&mut commands, &mut next_state, message.message.0.clone());
    }
}

/// If the map is somehow missing when a race begins, say so and leave rather
/// than sitting in an empty world.
pub(crate) fn bail_out_of_a_race_with_no_track(
    mut commands: Commands,
    built: Option<Res<crate::track::map::BuiltTrack>>,
    screen: Res<State<Screen>>,
    lobbies: Query<Entity, Or<(With<Lobby>, With<PendingLobby>)>>,
    mut frames: Local<u32>,
) {
    if *screen.get() != Screen::Race || built.is_some() {
        *frames = 0;
        return;
    }
    *frames += 1;
    // A few frames of slack, because the resource is inserted by the same
    // `OnEnter` chain that puts us here.
    if *frames < 30 {
        return;
    }
    error!("in a race with no track after 30 frames; leaving the session");
    for lobby in lobbies.iter() {
        commands.entity(lobby).despawn();
    }
    *frames = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::map::builtin::by_slug;

    #[test]
    fn the_hash_follows_the_contents() {
        let classic = by_slug("classic").unwrap();
        let sweeping = by_slug("sweeping").unwrap();
        assert_eq!(map_hash(&classic), map_hash(&classic.clone()));
        assert_ne!(map_hash(&classic), map_hash(&sweeping));

        // One node moved by a single sub-unit is a different map.
        let mut nudged = classic.clone();
        nudged.nodes[3].position.x += 1;
        assert_ne!(map_hash(&classic), map_hash(&nudged));

        // The name is part of the map, so renaming changes it too.
        let mut renamed = classic.clone();
        renamed.name = "Classic II".into();
        assert_ne!(map_hash(&classic), map_hash(&renamed));
    }

    /// The wire form has to survive the round trip, or a client races a track
    /// subtly unlike the host's. Postcard rather than JSON, because postcard is
    /// what actually goes over the socket.
    #[test]
    fn a_map_survives_the_wire_encoding_exactly() {
        for slug in ["classic", "sweeping"] {
            let map = by_slug(slug).unwrap();
            let bytes = postcard::to_allocvec(&map).unwrap();
            let back: MapData = postcard::from_bytes(&bytes).unwrap();
            assert_eq!(map, back, "{slug} did not survive the round trip");
            assert_eq!(map_hash(&map), map_hash(&back));
        }
    }

    /// A map has to fit comfortably in a single reliable message. `webrtc-rs`
    /// advertises a 64 KiB maximum where browsers advertise 256 KiB, and the
    /// wasm send swallows the error if it is exceeded -- so the failure would be
    /// silent, one-directional, and native-to-browser only.
    #[test]
    fn the_built_in_maps_are_small_enough_to_send() {
        const LIMIT: usize = 16 * 1024;
        for slug in ["classic", "sweeping"] {
            let size = postcard::to_allocvec(&by_slug(slug).unwrap()).unwrap().len();
            assert!(size < LIMIT, "{slug} encodes to {size} bytes, over {LIMIT}");
        }
    }
}
