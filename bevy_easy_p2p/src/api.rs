use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::networked_event::SyncedEventRegister;
use crate::networked_instantiation::{
    HandleInstantiation, InstantiationData, InstantiationDataNet,
};
use crate::networked_state::SyncedStateRegister;
use crate::networked_transform;
use crate::ping::PingPlugin;
use crate::schedules::{EasyHydrate, EasyP2PSchedulePlugin};
use crate::state::{NetworkedEntity, NetworkedId, P2PData, P2PLobbyState, PlayerInfo};
use crate::transports::api::{EasyP2PTransportRequest, EasyP2PTransportUpdate, Reliability};
use crate::updates::EasyP2PUpdate;

pub trait EasyP2PData: 'static + Send + Sync + Clone + core::fmt::Debug {
    type PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq;
    type PlayerInputData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static;
    type Instantiations: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static;
}

#[derive(Message)]
pub(crate) struct DespawnEntity(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Disconnected,
    Kicked,
}

#[derive(Message, Clone)]
pub(crate) struct OnLobbyExit(pub ExitReason);

#[derive(SystemParam)]
pub struct EasyP2P<'w, 's, T: EasyP2PData> {
    requests_w: MessageWriter<'w, EasyP2PTransportRequest<P2PData<T>>>,
    despawn_w: MessageWriter<'w, DespawnEntity>,
    instantiation_set: ParamSet<
        'w,
        's,
        (
            MessageWriter<'w, HandleInstantiation<<T as EasyP2PData>::Instantiations>>,
            MessageReader<'w, 's, HandleInstantiation<<T as EasyP2PData>::Instantiations>>,
        ),
    >,
    state: ResMut<'w, crate::state::EasyP2PState<<T as EasyP2PData>::PlayerData>>,
    updates_set: ParamSet<
        'w,
        's,
        (
            MessageWriter<'w, EasyP2PUpdate<T>>,
            MessageReader<'w, 's, EasyP2PUpdate<T>>,
        ),
    >,
    children_q: Query<'w, 's, &'static ChildOf>,
    network_entities_q: Query<'w, 's, &'static NetworkedEntity>,
}

impl<'w, 's, T: EasyP2PData> EasyP2P<'w, 's, T> {
    pub fn create_lobby(&mut self) {
        self.requests_w.write(EasyP2PTransportRequest::CreateLobby);
    }
    pub fn join_lobby(&mut self, code: &str) {
        info!("joining lobby... : {}", code);
        self.requests_w
            .write(EasyP2PTransportRequest::JoinLobby(code.to_string()));
    }
    pub fn exit_lobby(&mut self) {
        info!("exiting lobby...");
        self.requests_w.write(EasyP2PTransportRequest::ExitLobby);
    }
    pub fn send_message_to_host(&mut self, text: String) {
        let msg = P2PData::ClientLobbyChatMessage(text.clone(), NetworkedId::ClientId(0));
        self.requests_w
            .write(EasyP2PTransportRequest::SendToHost(msg, Reliability::Reliable));
    }
    pub fn send_message_all(&mut self, text: String) {
        let msg: P2PData<T> = P2PData::ClientLobbyChatMessage(text.clone(), NetworkedId::Host);
        self.requests_w
            .write(EasyP2PTransportRequest::SendToAll(msg, Reliability::Reliable));
    }
    pub fn send_inputs(&mut self, input: T::PlayerInputData) {
        let msg = P2PData::ClientInput(input.clone());
        if self.is_host() {
            self.updates_set.p0().write(EasyP2PUpdate::ClientInput {
                sender: NetworkedId::Host,
                input,
            });
        } else {
            self.requests_w
                .write(EasyP2PTransportRequest::SendToHost(msg, Reliability::Unreliable));
        }
    }
    pub fn instantiate(&mut self, instantiation: T::Instantiations, transform: Transform) {
        if !self.state.is_host {
            return;
        }
        let instantiation_data = InstantiationData {
            transform: transform.clone(),
            uuid: self.state.increment_uuid(),
            instantiation: instantiation.clone(),
        };
        self.instantiation_set
            .p0()
            .write(HandleInstantiation(instantiation_data.clone()));
        let net: InstantiationDataNet<T::Instantiations> =
            InstantiationDataNet::from(&instantiation_data);
        self.requests_w.write(EasyP2PTransportRequest::SendToAll(
            P2PData::HostInstantiation(net),
            Reliability::Reliable,
        ));
    }
    pub fn despawn(&mut self, uuid: u64) {
        if !self.state.is_host {
            return;
        }
        self.requests_w
            .write(EasyP2PTransportRequest::SendToAll(P2PData::HostDespawn(
                uuid,
            ), Reliability::Reliable));
        self.despawn_w.write(DespawnEntity(uuid));
    }
    pub fn get_instantiations(&mut self) -> Vec<InstantiationData<T::Instantiations>> {
        self.instantiation_set
            .p1()
            .read()
            .map(|inst| inst.0.clone())
            .collect()
    }
    pub fn kick(&mut self, networked_id: NetworkedId) {
        if !self.state.is_host {
            return;
        }
        if let NetworkedId::ClientId(client_id) = networked_id {
            self.requests_w
                .write(EasyP2PTransportRequest::Kick(client_id));
        }
    }
    pub fn is_host(&self) -> bool {
        self.state.is_host
    }
    pub fn get_local_player_id(&self) -> Option<NetworkedId> {
        if self.state.is_host {
            return Some(NetworkedId::Host);
        }
        self.state.local_networked_id
    }
    pub fn get_players(&self) -> Vec<PlayerInfo<T::PlayerData>> {
        self.state.get_players(self.is_host())
    }
    pub fn get_local_player_data(&self) -> T::PlayerData {
        self.state.local_player_data.clone()
    }
    pub fn set_local_player_data(&mut self, data: T::PlayerData) {
        self.state.local_player_data = data.clone();
        if self.state.is_host {
            let players = self.state.get_players(self.state.is_host);
            self.updates_set.p0().write(EasyP2PUpdate::RosterUpdated {
                players: players.clone(),
            });
            self.requests_w.write(EasyP2PTransportRequest::SendToAll(
                P2PData::HostLobbyInfoUpdate(players),
                Reliability::Reliable,
            ));
        } else {
            self.requests_w.write(EasyP2PTransportRequest::SendToHost(
                P2PData::ClientDataUpdate(data),
                Reliability::Reliable,
            ));
        }
    }
    pub fn read_updates(&mut self) -> Vec<EasyP2PUpdate<T>> {
        self.updates_set
            .p1()
            .read()
            .map(|update| update.clone())
            .collect()
    }
    pub fn get_player_data(&self, id: NetworkedId) -> Option<T::PlayerData> {
        self.get_players()
            .iter()
            .find(|player| player.id == id)
            .map(|player| player.data.clone())
    }
    pub fn get_player_index(&self, id: NetworkedId) -> Option<usize> {
        self.get_players().iter().position(|player| player.id == id)
    }
    pub fn get_closest_networked_id(&self, entity: Entity) -> Option<NetworkedId> {
        if let Ok(net) = self.network_entities_q.get(entity) {
            return Some(net.owner_id());
        }
        let ancestor = self
            .children_q
            .iter_ancestors(entity)
            .find(|a| self.network_entities_q.contains(*a))?;
        self.network_entities_q.get(ancestor).ok().map(|n| n.owner_id())
    }
    pub fn inputs_belong_to_player(&self, entity: Entity, id: &NetworkedId) -> bool {
        let Some(ancestor) = self.get_closest_networked_id(entity) else {
            return false;
        };
        ancestor == *id
    }
}

pub struct EasyP2PPlugin<T: EasyP2PData>(std::marker::PhantomData<T>);

impl<T: EasyP2PData> Default for EasyP2PPlugin<T> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<T: EasyP2PData> Plugin for EasyP2PPlugin<T> {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::state::EasyP2PState<T::PlayerData>>()
            .init_resource::<SyncedStateRegister>()
            .init_resource::<SyncedEventRegister>()
            .init_state::<P2PLobbyState>()
            .add_message::<EasyP2PUpdate<T>>()
            .add_message::<EasyP2PTransportRequest<P2PData<T>>>()
            .add_message::<EasyP2PTransportUpdate<P2PData<T>>>()
            .add_message::<DespawnEntity>()
            .add_message::<OnLobbyExit>()
            .add_message::<HandleInstantiation<T::Instantiations>>()
            .add_systems(EasyHydrate, crate::systems::process_transport_updates::<T>)
            .add_systems(
                Update,
                (
                    crate::systems::on_external_lobby_exit::<T>,
                    crate::systems::send_local_data_after_enter::<T>,
                    crate::systems::despawn_entity,
                ),
            )
            .add_plugins(networked_transform::NetworkedTransformPlugin::<T>::default())
            .add_plugins(PingPlugin::<T>::default())
            .add_plugins(EasyP2PSchedulePlugin);
    }
}
