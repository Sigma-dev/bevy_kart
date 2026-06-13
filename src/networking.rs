use bevy::prelude::*;
#[allow(unused_imports)]
use bevy_ensemble::prelude::*;
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

/// Networked component: identifies which player owns an entity.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct OwnerPlayer(pub u128);

/// Networked position proxy for non-physics tracked entities (items, rockets).
#[derive(Component, Clone, Debug, Serialize, Deserialize, Default)]
pub struct NetworkedPosition(pub Vec2);

/// Networked rotation proxy for non-physics tracked entities (rockets).
#[derive(Component, Clone, Debug, Serialize, Deserialize, Default)]
pub struct NetworkedRotation(pub f32);

/// Networked component: what kind of networked entity this is.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub enum EntityKind {
    Kart,
    ItemPickup(ItemType),
    Rocket,
    Explosion,
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
#[derive(Resource)]
pub struct LocalPlayerData(pub AppPlayerData);

impl Default for LocalPlayerData {
    fn default() -> Self {
        Self(AppPlayerData::default())
    }
}

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
