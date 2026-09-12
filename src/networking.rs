use bevy::prelude::*;
#[allow(unused_imports)]
use bevy_ensemble::prelude::*;
use bevy_ticked_networking::prelude::*;
use serde::{Deserialize, Serialize};

use crate::items::ItemType;
use crate::kart::KartColor;

/// Input data sent each tick from each player.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PlayerInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub using_item: bool,
}

/// Networked component: what kind of networked entity this is.
///
/// Whose it is lives in `bevy_ticked_networking::Owner`, which the stack reads
/// to decide what a client predicts and what it draws from the host's history.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub enum EntityKind {
    Kart,
    ItemPickup(ItemType),
    Rocket,
    Mine,
}

/// Whether this peer simulates a tracked entity, or only draws it.
///
/// The host simulates everything. A client simulates what it predicts -- its own
/// kart, and anything else marked `ReplicationMode::Predicted` -- and everything
/// else is put at the host's state, a couple of ticks behind, every tick: a
/// system that moved one of those would be moving it away from where it is
/// about to be put back. Absent marker means interpolated.
pub fn simulates(local_client: Option<&LocalClientPlayer>, mode: Option<&ReplicationMode>) -> bool {
    local_client.is_none() || matches!(mode, Some(ReplicationMode::Predicted))
}

/// Player metadata shared via ensemble messages.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Message)]
pub struct AppPlayerData {
    pub name: String,
    pub kart_color: KartColor,
}

impl Default for AppPlayerData {
    fn default() -> Self {
        Self {
            name: "YOUR_NAME".to_string(),
            kart_color: KartColor::new_random(),
        }
    }
}

/// Ensemble message for chat.
#[derive(Clone, Debug, Serialize, Deserialize, Message)]
pub struct ChatMessage {
    pub sender: u128,
    pub text: String,
}

/// Broadcast message: game state changed (host -> all peers).
#[derive(Clone, Debug, Serialize, Deserialize, Message)]
pub struct GameStateChanged(pub AppState);

/// Local player's data (stored locally, pushed via SetPlayerData when in a lobby).
#[derive(Resource, Default)]
pub struct LocalPlayerData(pub AppPlayerData);

#[derive(States, Default, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    OutOfGame,
    Game,
}

#[derive(States, Default, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LobbyState {
    #[default]
    OutOfLobby,
    InLobby,
}
