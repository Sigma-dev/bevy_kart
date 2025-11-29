use crate::api::EasyP2PData;
use crate::networked_instantiation::InstantiationData;
use crate::state::PlayerInfo;
use crate::{ClientId, ExitReason, NetworkedId};
use bevy::prelude::*;

#[derive(Message, Clone, Debug)]
pub enum EasyP2PUpdate<T: EasyP2PData> {
    LobbyCreated {
        code: String,
    },
    LobbyJoined {
        code: String,
    },
    LobbyEntered {
        code: String,
    },
    LobbyExited {
        reason: ExitReason,
    },
    HostChat {
        text: String,
    },
    ClientChat {
        client_id: ClientId,
        text: String,
    },
    RosterUpdated {
        players: Vec<PlayerInfo<T::PlayerData>>,
    },
    ClientInput {
        sender: NetworkedId,
        input: T::PlayerInputData,
    },
    Instantiated {
        data: InstantiationData<T::Instantiations>,
    },
}
