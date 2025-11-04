use crate::state::{InstantiationData, PlayerInfo};
use crate::{ClientId, ExitReason, NetworkedId};
use bevy::prelude::*;

#[derive(Message, Clone, Debug)]
pub enum EasyP2PUpdate<PlayerData, PlayerInputData, Instantiations>
where
    PlayerData: Clone + Send + Sync + core::fmt::Debug + 'static,
    PlayerInputData: Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations: Clone + Send + Sync + core::fmt::Debug + 'static,
{
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
        players: Vec<PlayerInfo<PlayerData>>,
    },
    ClientInput {
        sender: NetworkedId,
        input: PlayerInputData,
    },
    Instantiated {
        data: InstantiationData<Instantiations>,
    },
}
