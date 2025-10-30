use bevy::prelude::{FromWorld, Resource, World};

use crate::state::{InstantiationData, PlayerInfo};
use crate::{ClientId, ExitReason, NetworkedId};

#[derive(Clone, Debug)]
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

#[derive(Resource)]
pub struct EasyP2PUpdateQueue<PlayerData, PlayerInputData, Instantiations>
where
    PlayerData: Clone + Send + Sync + core::fmt::Debug + 'static,
    PlayerInputData: Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations: Clone + Send + Sync + core::fmt::Debug + 'static,
{
    queue: Vec<EasyP2PUpdate<PlayerData, PlayerInputData, Instantiations>>,
}

impl<PlayerData, PlayerInputData, Instantiations>
    EasyP2PUpdateQueue<PlayerData, PlayerInputData, Instantiations>
where
    PlayerData: Clone + Send + Sync + core::fmt::Debug + 'static,
    PlayerInputData: Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations: Clone + Send + Sync + core::fmt::Debug + 'static,
{
    pub fn push(&mut self, update: EasyP2PUpdate<PlayerData, PlayerInputData, Instantiations>) {
        self.queue.push(update);
    }

    pub fn drain(
        &mut self,
    ) -> impl Iterator<Item = EasyP2PUpdate<PlayerData, PlayerInputData, Instantiations>> {
        self.queue.drain(..)
    }
}

impl<PlayerData, PlayerInputData, Instantiations> FromWorld
    for EasyP2PUpdateQueue<PlayerData, PlayerInputData, Instantiations>
where
    PlayerData: Clone + Send + Sync + core::fmt::Debug + 'static,
    PlayerInputData: Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations: Clone + Send + Sync + core::fmt::Debug + 'static,
{
    fn from_world(_: &mut World) -> Self {
        Self { queue: Vec::new() }
    }
}
