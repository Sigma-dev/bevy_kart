use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::time::common_conditions::on_timer;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::state::{
    InstantiationData, InstantiationDataNet, IsHost, NetworkedEntity, NetworkedId, P2PData,
    P2PLobbyState, PlayerInfo, SyncedEventRegister, SyncedStateRegister,
};
use crate::updates::{EasyP2PUpdate, EasyP2PUpdateQueue};
use crate::{ClientId, networked_transform};

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum EasyP2PSystemSet {
    Transport,
    Core,
    Emit,
}

#[derive(Message, Clone)]
pub(crate) struct OnLobbyCreated(pub String);
#[derive(Message, Clone)]
pub(crate) struct OnLobbyJoined(pub String);
#[derive(Message, Clone)]
pub(crate) struct OnLobbyEntered(pub String);
#[derive(Message)]
pub struct OnApplyState<S>(pub S)
where
    S: States + Clone + Send + Sync + 'static;

#[derive(Message)]
pub(crate) struct OnRosterUpdate<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
>(pub Vec<PlayerInfo<PlayerData>>);
#[derive(Message, Clone)]
pub(crate) struct OnRelayToAllExcept<PlayerData, PlayerInputData, Instantiations>(
    pub ClientId,
    pub P2PData<PlayerData, PlayerInputData, Instantiations>,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Disconnected,
    Kicked,
}

#[derive(Message, Clone)]
pub(crate) struct OnLobbyExit(pub ExitReason);
#[derive(Message, Clone)]
pub(crate) struct OnTransportRosterChanged(pub Vec<String>);
#[derive(Message, Clone)]
pub(crate) struct OnTransportSendToHost(pub String);
#[derive(Message, Clone)]
pub(crate) struct OnTransportSendToAll(pub String);
#[derive(Message, Clone)]
pub(crate) struct OnTransportSendToClient(pub ClientId, pub String);
#[derive(Message, Clone)]
pub(crate) struct OnTransportRelayToAllExcept(pub ClientId, pub String);
#[derive(Message, Clone)]
pub(crate) struct OnTransportIncomingFromClient(pub ClientId, pub String);
#[derive(Message, Clone)]
pub(crate) struct OnTransportIncomingFromHost(pub String);
#[derive(Message, Clone)]
pub(crate) struct HandleInstantiation<Instantiations>(pub InstantiationData<Instantiations>);
#[derive(Message, Clone)]
pub(crate) struct OnInternalClientData<PlayerData, PlayerInputData, Instantiations>(
    pub ClientId,
    pub P2PData<PlayerData, PlayerInputData, Instantiations>,
);
#[derive(Message, Clone)]
pub(crate) struct OnInternalHostData<PlayerData, PlayerInputData, Instantiations>(
    pub P2PData<PlayerData, PlayerInputData, Instantiations>,
);

#[derive(Message, Clone)]
pub(crate) struct OnCreateLobbyReq;
#[derive(Message, Clone)]
pub(crate) struct OnJoinLobbyReq(pub String);
#[derive(Message, Clone)]
pub(crate) struct OnSendToHostReq<PlayerData, PlayerInputData, Instantiations>(
    pub P2PData<PlayerData, PlayerInputData, Instantiations>,
);
#[derive(Message, Clone)]
pub(crate) struct OnSendToAllReq<PlayerData, PlayerInputData, Instantiations>(
    pub P2PData<PlayerData, PlayerInputData, Instantiations>,
);
#[derive(Message, Clone)]
pub(crate) struct OnSendToClientReq<PlayerData, PlayerInputData, Instantiations>(
    pub ClientId,
    pub P2PData<PlayerData, PlayerInputData, Instantiations>,
);
#[derive(Message, Clone)]
pub(crate) struct OnExitLobbyReq;
#[derive(Message, Clone)]
pub(crate) struct OnKickReq(pub ClientId);

#[derive(Message)]
pub struct PingUpdate(pub std::time::Duration);

pub trait P2PTransport: Send + Sync + 'static {
    type Error;

    fn create_lobby(world: &mut World) -> Result<String, Self::Error>;
    fn join_lobby(world: &mut World, code: &str) -> Result<(), Self::Error>;
    fn exit_lobby(world: &mut World);
    fn send_to_host(world: &mut World, text: String);
    fn send_to_all(world: &mut World, text: String);
    fn kick(world: &mut World, client_id: ClientId);
    fn poll_transport(world: &mut World);
}

#[derive(SystemParam)]
pub struct EasyP2P<
    'w,
    's,
    T: P2PTransport,
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
> {
    create_w: MessageWriter<'w, OnCreateLobbyReq>,
    join_w: MessageWriter<'w, OnJoinLobbyReq>,
    exit_w: MessageWriter<'w, OnExitLobbyReq>,
    send_host_w: MessageWriter<'w, OnSendToHostReq<PlayerData, PlayerInputData, Instantiations>>,
    send_all_w: MessageWriter<'w, OnSendToAllReq<PlayerData, PlayerInputData, Instantiations>>,
    kick_w: MessageWriter<'w, OnKickReq>,
    instantiation_set: ParamSet<
        'w,
        's,
        (
            MessageWriter<'w, HandleInstantiation<Instantiations>>,
            MessageReader<'w, 's, HandleInstantiation<Instantiations>>,
        ),
    >,
    state: ResMut<'w, crate::state::EasyP2PState<PlayerData>>,
    updates: ResMut<'w, EasyP2PUpdateQueue<PlayerData, PlayerInputData, Instantiations>>,
    children_q: Query<'w, 's, &'static ChildOf>,
    network_entities_q: Query<'w, 's, &'static NetworkedEntity>,
    _marker: std::marker::PhantomData<&'s T>,
    roster_w: MessageWriter<'w, OnRosterUpdate<PlayerData>>,
}

impl<'w, 's, T, PlayerData: Default + PartialEq, PlayerInputData, Instantiations>
    EasyP2P<'w, 's, T, PlayerData, PlayerInputData, Instantiations>
where
    T: P2PTransport,
    PlayerData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    PlayerInputData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
{
    pub fn create_lobby(&mut self) {
        self.create_w.write(OnCreateLobbyReq);
    }
    pub fn join_lobby(&mut self, code: &str) {
        info!("joining lobby... : {}", code);
        self.join_w.write(OnJoinLobbyReq(code.to_string()));
    }
    pub fn exit_lobby(&mut self) {
        info!("exiting lobby...");
        self.exit_w.write(OnExitLobbyReq);
    }
    pub fn send_message_to_host(&mut self, text: String) {
        let msg = P2PData::ClientLobbyChatMessage(text.clone(), NetworkedId::ClientId(0));
        info!("sending message to host: {:?}", &msg);
        self.send_host_w.write(OnSendToHostReq(msg));
    }
    pub fn send_message_all(&mut self, text: String) {
        let msg = P2PData::ClientLobbyChatMessage(text.clone(), NetworkedId::Host);
        info!("sending message to all: {:?}", &msg);
        self.send_all_w.write(OnSendToAllReq(msg));
    }
    pub fn send_inputs(&mut self, input: PlayerInputData) {
        let msg = P2PData::ClientInput(input.clone());
        if self.is_host() {
            self.updates.push(EasyP2PUpdate::ClientInput {
                sender: NetworkedId::Host,
                input,
            });
        } else {
            self.send_host_w.write(OnSendToHostReq(msg));
        }
    }
    pub fn instantiate(&mut self, instantiation: Instantiations, transform: Transform) {
        self.instantiation_set
            .p0()
            .write(HandleInstantiation(InstantiationData {
                transform: transform.clone(),
                instantiation: instantiation.clone(),
            }));
        if self.state.is_host {
            let net: InstantiationDataNet<Instantiations> =
                InstantiationDataNet::from(&InstantiationData {
                    transform,
                    instantiation,
                });
            self.send_all_w
                .write(OnSendToAllReq(P2PData::HostInstantiation(net)));
        }
    }
    pub fn get_instantiations(&mut self) -> Vec<InstantiationData<Instantiations>> {
        self.instantiation_set
            .p1()
            .read()
            .map(|inst| inst.0.clone())
            .collect()
    }
    pub fn kick(&mut self, client_id: ClientId) {
        self.kick_w.write(OnKickReq(client_id));
    }
    pub fn is_host(&self) -> bool {
        self.state.is_host
    }
    pub fn get_players(&self) -> Vec<PlayerInfo<PlayerData>> {
        self.state.get_players(self.is_host())
    }
    pub fn get_local_player_data(&self) -> PlayerData {
        self.state.local_player_data.clone()
    }
    pub fn set_local_player_data(&mut self, data: PlayerData) {
        self.state.local_player_data = data.clone();
        if self.state.is_host {
            let players = self.state.get_players(self.state.is_host);
            let _ = self.roster_w.write(OnRosterUpdate(players.clone()));
            self.updates.push(EasyP2PUpdate::RosterUpdated {
                players: players.clone(),
            });
            self.send_all_w
                .write(OnSendToAllReq(P2PData::HostLobbyInfoUpdate(players)));
        } else {
            self.send_host_w
                .write(OnSendToHostReq(P2PData::ClientDataUpdate(data)));
        }
    }
    pub fn read_updates(
        &mut self,
    ) -> impl Iterator<Item = EasyP2PUpdate<PlayerData, PlayerInputData, Instantiations>> {
        self.updates.drain()
    }
    pub fn get_player_data(&self, id: NetworkedId) -> PlayerData {
        self.get_players()
            .iter()
            .find(|player| player.id == id)
            .unwrap()
            .data
            .clone()
    }
    pub fn get_closest_networked_id(&self, entity: Entity) -> Option<NetworkedId> {
        if self.network_entities_q.contains(entity) {
            return Some(self.network_entities_q.get(entity).unwrap().id());
        }
        let ancestor = self
            .children_q
            .iter_ancestors(entity)
            .find(|a| self.network_entities_q.contains(*a))?;
        Some(self.network_entities_q.get(ancestor).unwrap().id())
    }
    pub fn inputs_belong_to_player(&self, entity: Entity, id: &NetworkedId) -> bool {
        let Some(ancestor) = self.get_closest_networked_id(entity) else {
            return false;
        };
        ancestor == *id
    }
}

#[derive(SystemParam)]
pub struct EasyP2PTransportIo<
    'w,
    's,
    PlayerData: 'static,
    PlayerInputData: 'static,
    Instantiations: 'static,
> {
    create_r: MessageReader<'w, 's, OnCreateLobbyReq>,
    join_r: MessageReader<'w, 's, OnJoinLobbyReq>,
    exit_r: MessageReader<'w, 's, OnExitLobbyReq>,
    kick_r: MessageReader<'w, 's, OnKickReq>,
    send_host_r: MessageReader<'w, 's, OnTransportSendToHost>,
    send_all_r: MessageReader<'w, 's, OnTransportSendToAll>,
    send_client_r: MessageReader<'w, 's, OnTransportSendToClient>,
    relay_except_r: MessageReader<'w, 's, OnTransportRelayToAllExcept>,
    lobby_created_w: MessageWriter<'w, OnLobbyCreated>,
    lobby_joined_w: MessageWriter<'w, OnLobbyJoined>,
    lobby_entered_w: MessageWriter<'w, OnLobbyEntered>,
    lobby_exit_rw: ParamSet<
        'w,
        's,
        (
            MessageWriter<'w, OnLobbyExit>,
            MessageReader<'w, 's, OnLobbyExit>,
        ),
    >,
    roster_changed_w: MessageWriter<'w, OnTransportRosterChanged>,
    incoming_client_w: MessageWriter<'w, OnTransportIncomingFromClient>,
    incoming_host_w: MessageWriter<'w, OnTransportIncomingFromHost>,
    _marker: std::marker::PhantomData<(PlayerData, PlayerInputData, Instantiations)>,
}

impl<'w, 's, PlayerData: 'static, PlayerInputData: 'static, Instantiations: 'static>
    EasyP2PTransportIo<'w, 's, PlayerData, PlayerInputData, Instantiations>
{
    pub fn take_create_requests(&mut self) -> usize {
        self.create_r.read().count()
    }

    pub fn take_join_requests(&mut self) -> Vec<String> {
        self.join_r.read().map(|req| req.0.clone()).collect()
    }

    pub fn take_exit_requests(&mut self) -> usize {
        self.exit_r.read().count()
    }

    pub fn take_kick_requests(&mut self) -> Vec<ClientId> {
        self.kick_r.read().map(|req| req.0).collect()
    }

    pub fn take_lobby_exit_events(&mut self) -> Vec<ExitReason> {
        self.lobby_exit_rw
            .p1()
            .read()
            .map(|OnLobbyExit(reason)| *reason)
            .collect()
    }

    pub fn take_send_to_host(&mut self) -> Vec<String> {
        self.send_host_r
            .read()
            .map(|OnTransportSendToHost(text)| text.clone())
            .collect()
    }

    pub fn take_send_to_all(&mut self) -> Vec<String> {
        self.send_all_r
            .read()
            .map(|OnTransportSendToAll(text)| text.clone())
            .collect()
    }

    pub fn take_send_to_client(&mut self) -> Vec<(ClientId, String)> {
        self.send_client_r
            .read()
            .map(|OnTransportSendToClient(client_id, text)| (*client_id, text.clone()))
            .collect()
    }

    pub fn take_relay_to_all_except(&mut self) -> Vec<(ClientId, String)> {
        self.relay_except_r
            .read()
            .map(|OnTransportRelayToAllExcept(client_id, text)| (*client_id, text.clone()))
            .collect()
    }

    pub fn emit_lobby_created(&mut self, code: impl Into<String>) {
        self.lobby_created_w.write(OnLobbyCreated(code.into()));
    }

    pub fn emit_lobby_joined(&mut self, code: impl Into<String>) {
        self.lobby_joined_w.write(OnLobbyJoined(code.into()));
    }

    pub fn emit_lobby_entered(&mut self, code: impl Into<String>) {
        self.lobby_entered_w.write(OnLobbyEntered(code.into()));
    }

    pub fn emit_lobby_exit(&mut self, reason: ExitReason) {
        self.lobby_exit_rw.p0().write(OnLobbyExit(reason));
    }

    pub fn emit_roster_changed(&mut self, roster: Vec<String>) {
        self.roster_changed_w
            .write(OnTransportRosterChanged(roster));
    }

    pub fn emit_incoming_from_client(&mut self, client_id: ClientId, payload: impl Into<String>) {
        self.incoming_client_w
            .write(OnTransportIncomingFromClient(client_id, payload.into()));
    }

    pub fn emit_incoming_from_host(&mut self, payload: impl Into<String>) {
        self.incoming_host_w
            .write(OnTransportIncomingFromHost(payload.into()));
    }
}

pub struct EasyP2PPlugin<T: P2PTransport, PlayerData, PlayerInputData, Instantiations>(
    std::marker::PhantomData<(T, PlayerData, PlayerInputData, Instantiations)>,
);

impl<T: P2PTransport, PlayerData, PlayerInputData, Instantiations> Default
    for EasyP2PPlugin<T, PlayerData, PlayerInputData, Instantiations>
{
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<T, PlayerData, PlayerInputData, Instantiations> Plugin
    for EasyP2PPlugin<T, PlayerData, PlayerInputData, Instantiations>
where
    T: P2PTransport,
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
{
    fn build(&self, app: &mut App) {
        app.configure_sets(
            Update,
            (
                EasyP2PSystemSet::Transport,
                EasyP2PSystemSet::Core,
                EasyP2PSystemSet::Emit,
            )
                .chain(),
        )
        .init_resource::<crate::state::EasyP2PState<PlayerData>>()
        .init_resource::<IsHost>()
        .init_resource::<SyncedStateRegister>()
        .init_resource::<SyncedEventRegister>()
        .init_resource::<EasyP2PUpdateQueue<PlayerData, PlayerInputData, Instantiations>>()
        .init_state::<P2PLobbyState>()
        .add_message::<OnCreateLobbyReq>()
        .add_message::<OnJoinLobbyReq>()
        .add_message::<OnSendToHostReq<PlayerData, PlayerInputData, Instantiations>>()
        .add_message::<OnSendToAllReq<PlayerData, PlayerInputData, Instantiations>>()
        .add_message::<OnSendToClientReq<PlayerData, PlayerInputData, Instantiations>>()
        .add_message::<OnExitLobbyReq>()
        .add_message::<OnKickReq>()
        .add_message::<OnLobbyCreated>()
        .add_message::<OnLobbyJoined>()
        .add_message::<OnLobbyEntered>()
        .add_message::<OnInternalClientData<PlayerData, PlayerInputData, Instantiations>>()
        .add_message::<OnInternalHostData<PlayerData, PlayerInputData, Instantiations>>()
        .add_message::<OnLobbyExit>()
        .add_message::<OnTransportRosterChanged>()
        .add_message::<OnTransportSendToHost>()
        .add_message::<OnTransportSendToAll>()
        .add_message::<OnTransportSendToClient>()
        .add_message::<OnTransportRelayToAllExcept>()
        .add_message::<OnTransportIncomingFromClient>()
        .add_message::<OnTransportIncomingFromHost>()
        .add_message::<OnRosterUpdate<PlayerData>>()
        .add_message::<OnRelayToAllExcept<PlayerData, PlayerInputData, Instantiations>>()
        .add_message::<HandleInstantiation<Instantiations>>()
        .add_message::<PingUpdate>()
        .add_systems(
            Update,
            (
                crate::systems::state_update_system::<PlayerData, PlayerInputData, Instantiations>,
                (
                    crate::systems::on_external_lobby_exit::<
                        PlayerData,
                        PlayerInputData,
                        Instantiations,
                    >,
                    crate::systems::intercept_data_messages::<
                        PlayerData,
                        PlayerInputData,
                        Instantiations,
                    >,
                    crate::systems::send_local_data_after_enter::<
                        PlayerData,
                        PlayerInputData,
                        Instantiations,
                    >,
                    crate::systems::handle_client_data_update_on_host::<
                        PlayerData,
                        PlayerInputData,
                        Instantiations,
                    >,
                    crate::systems::broadcast_roster_on_host::<
                        PlayerData,
                        PlayerInputData,
                        Instantiations,
                    >,
                    crate::systems::encode_outgoing::<PlayerData, PlayerInputData, Instantiations>,
                    crate::systems::decode_incoming::<PlayerData, PlayerInputData, Instantiations>,
                    crate::systems::despawn_on_leave::<PlayerData>,
                    crate::systems::send_ping::<PlayerData, PlayerInputData, Instantiations>
                        .run_if(on_timer(Duration::from_secs(1))),
                ),
            )
                .chain()
                .in_set(EasyP2PSystemSet::Core),
        )
        .add_plugins(networked_transform::NetworkedTransformPlugin::<
            T,
            PlayerData,
            PlayerInputData,
            Instantiations,
        >::default());
    }
}
