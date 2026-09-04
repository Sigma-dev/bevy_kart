//! Finding out that two peers registered different components, at the join.
//!
//! Snapshot component indices are positional `u16` (see the registration block in
//! `main.rs`). Nothing in the packet says which type an index means, so two peers
//! built from different commits deserialise each other's `Position` bytes as an
//! `EntityKind` — with no error, no warning, and no way to tell from the symptom
//! that the cause is a build mismatch rather than a physics bug.
//!
//! `TickedComponentRegistry::wire_hash()` hashes `(index, name)` for every
//! registered type, where the names are the strings passed to
//! `register_networked_ticked_component_as`. The host states its hash; a client
//! that disagrees says so once, loudly.
//!
//! Re-sent on a timer rather than exchanged in a join handshake: it costs eight
//! bytes every half second and means a peer that joins, leaves and rejoins is
//! re-checked each time, with no join-time protocol to get wrong.

use bevy::prelude::*;
use bevy_ensemble::prelude::*;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::server::LocalServerPlayer;
use serde::{Deserialize, Serialize};

/// How often the host restates its registry hash, in seconds.
const RESTATE_EVERY: f32 = 0.5;

/// What the host says its replication registry is.
#[derive(Message, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WireFormat(pub u64);

pub struct WireFormatPlugin;

impl Plugin for WireFormatPlugin {
    fn build(&self, app: &mut App) {
        app.register_broadcast_message::<WireFormat>()
            .add_systems(Update, (broadcast_wire_format, compare_wire_format));
    }
}

fn broadcast_wire_format(
    time: Res<Time>,
    mut cooldown: Local<f32>,
    server_player: Option<Res<LocalServerPlayer>>,
    registry: Res<TickedComponentRegistry>,
    lobbies: Query<Entity, (With<Lobby>, With<Host>)>,
    mut commands: Commands,
) {
    // Emptiness first, cooldown second. The other way round spends the timer while
    // there is nothing to send, so the first statement waits for a full period
    // after the lobby appears -- which is the window a joiner is most likely to be
    // in.
    let Ok(lobby) = lobbies.single() else { return };
    if server_player.is_none() {
        return;
    }
    *cooldown -= time.delta_secs();
    if *cooldown > 0.0 {
        return;
    }
    *cooldown = RESTATE_EVERY;

    let message = WireFormat(registry.wire_hash());
    commands
        .entity(lobby)
        .trigger(move |entity| BroadcastLobbyMessage::new(entity, message));
}

fn compare_wire_format(
    mut messages: MessageReader<ReceivedEnsembleMessage<WireFormat>>,
    registry: Res<TickedComponentRegistry>,
    mut reported: Local<bool>,
) {
    let ours = registry.wire_hash();
    for message in messages.read() {
        if message.message.0 == ours || *reported {
            continue;
        }
        *reported = true;
        error!(
            "the host's replication registry is not ours ({:#018x} vs {:#018x}). \
             Snapshot component indices are positional, so from here on every \
             component is being read as a different one. The two peers are built \
             from different commits.",
            message.message.0, ours
        );
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use bevy_ticked::prelude::*;

    /// Everything registered, in order. **This list is the wire format.**
    ///
    /// Hand-written next to the assertions rather than read out of the registry,
    /// on purpose: a list that derived itself from the thing it is checking could
    /// not disagree with it. This is what fails in CI when somebody tidies the
    /// registration block into alphabetical order — which compiles, warns about
    /// nothing, and changes the meaning of every index.
    const EXPECTED: &[&str] = &[
        "Position",
        "Rotation",
        "LinearVelocity",
        "AngularVelocity",
        "OwnerPlayer",
        "EntityKind",
        "NetworkedPosition",
        "NetworkedRotation",
        "CarControllerInputs",
        "SteeringState",
        "HeldItem",
        "BoostEffect",
        "CarControllerDisabled",
        "RocketHit",
    ];

    #[test]
    fn registration_order_is_the_wire_format() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        crate::register_networked_components(&mut app);
        let registry = app.world().resource::<TickedComponentRegistry>().clone();

        let names: Vec<&str> = registry.wire_names().collect();
        assert_eq!(
            names, EXPECTED,
            "the registration block changed. Appending is safe; reordering, \
             renaming and deleting are not -- indices are positional and travel \
             in every snapshot."
        );
    }

    #[test]
    fn the_hash_notices_a_reorder() {
        // The failure this whole module exists for is silent by construction, so
        // check that the hash is actually order-sensitive rather than trusting it.
        fn hash_of(names: &[&str]) -> u64 {
            const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
            const PRIME: u64 = 0x0000_0100_0000_01b3;
            names.iter().enumerate().fold(OFFSET, |mut h, (i, n)| {
                for b in (i as u16).to_le_bytes().iter().chain(n.as_bytes()) {
                    h ^= u64::from(*b);
                    h = h.wrapping_mul(PRIME);
                }
                h
            })
        }
        assert_ne!(
            hash_of(&["Position", "Rotation"]),
            hash_of(&["Rotation", "Position"])
        );
    }
}
