use crate::{api::EasyP2PData, networked_instantiation::InstantiationDataNet};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, Eq, PartialEq, Debug, Hash, States)]
pub enum P2PLobbyState {
    #[default]
    OutOfLobby,
    JoiningLobby,
    InLobby,
}

#[derive(Component, Serialize, Deserialize, Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum NetworkedId {
    Host,
    ClientId(u64),
}

#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct NetworkedEntity {
    pub(crate) owner_id: NetworkedId,
    pub(crate) uuid: u64,
    pub(crate) despawn_on_leave: bool,
}

impl NetworkedEntity {
    pub fn new(owner_id: NetworkedId, uuid: u64) -> Self {
        Self {
            owner_id,
            uuid,
            despawn_on_leave: true,
        }
    }

    pub fn owner_id(&self) -> NetworkedId {
        self.owner_id
    }

    pub fn uuid(&self) -> u64 {
        self.uuid
    }

    pub fn despawn_on_leave(&self) -> bool {
        self.despawn_on_leave
    }

    pub fn set_despawn_on_leave(&mut self, value: bool) {
        self.despawn_on_leave = value;
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerInfo<PlayerData> {
    pub id: NetworkedId,
    pub data: PlayerData,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum P2PData<T: EasyP2PData> {
    ClientLobbyChatMessage(String, NetworkedId),
    ClientInput(T::PlayerInputData),
    ClientDataUpdate(T::PlayerData),
    HostLobbyInfoUpdate(Vec<PlayerInfo<T::PlayerData>>),
    ClientIdAssignment(NetworkedId),
    StateSync(u8, String),
    EventSync(u8, String),
    HostInstantiation(InstantiationDataNet<T::Instantiations>),
    HostDespawn(u64),
    PingRequest(f32),
}

#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct EasyP2PState<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
> {
    pub local_player_data: PlayerData,
    pub is_host: bool,
    pub lobby_code: String,
    pub players: Vec<PlayerInfo<PlayerData>>,
    pub instantiation_uuid_counter: u64,
    pub local_networked_id: Option<NetworkedId>,
}

impl<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
> EasyP2PState<PlayerData>
{
    pub fn get_players(&self, add_host: bool) -> Vec<PlayerInfo<PlayerData>> {
        let mut players = if add_host {
            vec![PlayerInfo {
                id: NetworkedId::Host,
                data: self.local_player_data.clone(),
            }]
        } else {
            vec![]
        };
        players.extend(self.players.clone());
        players
    }

    pub fn increment_uuid(&mut self) -> u64 {
        let current = self.instantiation_uuid_counter;
        self.instantiation_uuid_counter = current + 1;
        current
    }
}
