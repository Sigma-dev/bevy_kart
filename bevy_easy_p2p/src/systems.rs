use core::any::TypeId;
use std::time::Duration;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::api::{
    HandleInstantiation, OnExitLobbyReq, OnInternalClientData, OnInternalHostData, OnLobbyCreated,
    OnLobbyEntered, OnLobbyExit, OnLobbyJoined, OnRelayToAllExcept, OnRosterUpdate, OnSendToAllReq,
    OnSendToClientReq, OnSendToHostReq, OnTransportIncomingFromClient, OnTransportIncomingFromHost,
    OnTransportRelayToAllExcept, OnTransportRosterChanged, OnTransportSendToAll,
    OnTransportSendToClient, OnTransportSendToHost, PingUpdate,
};
use crate::state::{
    EasyP2PState, InstantiationData, IsHost, NetworkedEntity, NetworkedId, P2PData, P2PLobbyState,
    PlayerInfo, SyncedEventRegister, SyncedStateRegister,
};
use crate::updates::{EasyP2PUpdate, EasyP2PUpdateQueue};

pub(crate) fn on_external_lobby_exit<PlayerData, PlayerInputData, Instantiations>(
    mut state: ResMut<EasyP2PState<PlayerData>>,
    mut r: MessageReader<OnLobbyExit>,
    mut lobby_state: ResMut<NextState<P2PLobbyState>>,
    mut host_flag: ResMut<IsHost>,
    mut updates: ResMut<EasyP2PUpdateQueue<PlayerData, PlayerInputData, Instantiations>>,
) where
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
    let mut exit_reason = None;
    for OnLobbyExit(reason) in r.read() {
        exit_reason = Some(*reason);
    }
    if exit_reason.is_none() {
        return;
    }
    let reason = exit_reason.unwrap();
    state.is_host = false;
    host_flag.0 = false;
    state.lobby_code.clear();
    state.players.clear();
    lobby_state.set(P2PLobbyState::OutOfLobby);
    updates.push(EasyP2PUpdate::LobbyExited { reason });
}

pub(crate) fn broadcast_roster_on_host<PlayerData, PlayerInputData, Instantiations>(
    mut info_r: MessageReader<OnTransportRosterChanged>,
    mut roster_w: MessageWriter<OnRosterUpdate<PlayerData>>,
    mut w_send_all: MessageWriter<OnSendToAllReq<PlayerData, PlayerInputData, Instantiations>>,
    mut state: ResMut<EasyP2PState<PlayerData>>,
    mut updates: ResMut<EasyP2PUpdateQueue<PlayerData, PlayerInputData, Instantiations>>,
) where
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
    if !state.is_host {
        return;
    }
    for OnTransportRosterChanged(list) in info_r.read() {
        state.players.retain(|p| match p.id {
            NetworkedId::ClientId(cid) => list.contains(&cid.to_string()),
            NetworkedId::Host => true,
        });

        let players = state.get_players(state.is_host);
        let _ = roster_w.write(OnRosterUpdate(players.clone()));
        w_send_all.write(OnSendToAllReq(P2PData::HostLobbyInfoUpdate(
            players.clone(),
        )));
        updates.push(EasyP2PUpdate::RosterUpdated { players });
    }
}

pub(crate) fn despawn_on_leave<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
>(
    mut commands: Commands,
    mut on_roster_update: MessageReader<OnRosterUpdate<PlayerData>>,
    network_entities_q: Query<(Entity, &NetworkedEntity)>,
) {
    for OnRosterUpdate(list) in on_roster_update.read() {
        for (entity, networked) in network_entities_q.iter() {
            let should_despawn = match networked.id {
                NetworkedId::ClientId(cid) => {
                    !list.iter().any(|p| p.id == NetworkedId::ClientId(cid))
                }
                NetworkedId::Host => false,
            };
            if should_despawn && networked.despawn_on_leave {
                commands.entity(entity).despawn();
            }
        }
    }
}

pub(crate) fn encode_outgoing<
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
>(
    mut to_host_r: MessageReader<OnSendToHostReq<PlayerData, PlayerInputData, Instantiations>>,
    mut to_all_r: MessageReader<OnSendToAllReq<PlayerData, PlayerInputData, Instantiations>>,
    mut to_client_r: MessageReader<OnSendToClientReq<PlayerData, PlayerInputData, Instantiations>>,
    mut relay_except_r: MessageReader<
        OnRelayToAllExcept<PlayerData, PlayerInputData, Instantiations>,
    >,
    mut w_send_host: MessageWriter<OnTransportSendToHost>,
    mut w_send_all: MessageWriter<OnTransportSendToAll>,
    mut w_send_client: MessageWriter<OnTransportSendToClient>,
    mut w_relay_except: MessageWriter<OnTransportRelayToAllExcept>,
) {
    for OnSendToHostReq(data) in to_host_r.read() {
        if let Ok(text) = serde_json::to_string(&data) {
            w_send_host.write(OnTransportSendToHost(text));
        }
    }
    for OnSendToAllReq(data) in to_all_r.read() {
        if let Ok(text) = serde_json::to_string(&data) {
            w_send_all.write(OnTransportSendToAll(text));
        }
    }
    for OnSendToClientReq(cid, data) in to_client_r.read() {
        if let Ok(text) = serde_json::to_string(&data) {
            w_send_client.write(OnTransportSendToClient(*cid, text));
        }
    }
    for OnRelayToAllExcept(sender, data) in relay_except_r.read() {
        if let Ok(text) = serde_json::to_string(&data) {
            w_relay_except.write(OnTransportRelayToAllExcept(*sender, text));
        }
    }
}

pub(crate) fn decode_incoming<
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
>(
    mut from_client_r: MessageReader<OnTransportIncomingFromClient>,
    mut from_host_r: MessageReader<OnTransportIncomingFromHost>,
    mut ev_client: MessageWriter<OnInternalClientData<PlayerData, PlayerInputData, Instantiations>>,
    mut ev_host: MessageWriter<OnInternalHostData<PlayerData, PlayerInputData, Instantiations>>,
) {
    for OnTransportIncomingFromClient(cid, text) in from_client_r.read() {
        if let Ok(data) =
            serde_json::from_str::<P2PData<PlayerData, PlayerInputData, Instantiations>>(text)
        {
            ev_client.write(OnInternalClientData(*cid, data));
        }
    }
    for OnTransportIncomingFromHost(text) in from_host_r.read() {
        if let Ok(data) =
            serde_json::from_str::<P2PData<PlayerData, PlayerInputData, Instantiations>>(text)
        {
            ev_host.write(OnInternalHostData(data));
        }
    }
}

pub(crate) fn state_update_system<PlayerData, PlayerInputData, Instantiations>(
    mut state: ResMut<EasyP2PState<PlayerData>>,
    mut created_r: MessageReader<OnLobbyCreated>,
    mut joined_r: MessageReader<OnLobbyJoined>,
    mut entered_r: MessageReader<OnLobbyEntered>,
    mut exit_r: MessageReader<OnExitLobbyReq>,
    mut lobby_state: ResMut<NextState<P2PLobbyState>>,
    mut host_flag: ResMut<IsHost>,
    mut updates: ResMut<EasyP2PUpdateQueue<PlayerData, PlayerInputData, Instantiations>>,
) where
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
    for OnLobbyCreated(code) in created_r.read() {
        state.is_host = true;
        state.lobby_code = code.clone();
        host_flag.0 = true;
        updates.push(EasyP2PUpdate::LobbyCreated { code: code.clone() });
    }
    for OnLobbyJoined(code) in joined_r.read() {
        state.is_host = false;
        state.lobby_code = code.clone();
        host_flag.0 = false;
        updates.push(EasyP2PUpdate::LobbyJoined { code: code.clone() });
    }
    for OnLobbyEntered(code) in entered_r.read() {
        state.lobby_code = code.clone();
        lobby_state.set(P2PLobbyState::InLobby);
        updates.push(EasyP2PUpdate::LobbyEntered { code: code.clone() });
    }
    for _ in exit_r.read() {
        state.is_host = false;
        state.lobby_code.clear();
        state.players.clear();
        lobby_state.set(P2PLobbyState::OutOfLobby);
        host_flag.0 = false;
    }
}

pub(crate) fn intercept_data_messages<
    PlayerData: Default + PartialEq,
    PlayerInputData,
    Instantiations,
>(
    time: Res<Time>,
    mut commands: Commands,
    mut internal_client_r: MessageReader<
        OnInternalClientData<PlayerData, PlayerInputData, Instantiations>,
    >,
    mut internal_host_r: MessageReader<
        OnInternalHostData<PlayerData, PlayerInputData, Instantiations>,
    >,
    mut roster_w: MessageWriter<OnRosterUpdate<PlayerData>>,
    mut relay_w: MessageWriter<OnRelayToAllExcept<PlayerData, PlayerInputData, Instantiations>>,
    mut inst_w: MessageWriter<HandleInstantiation<Instantiations>>,
    mut w_send_client: MessageWriter<
        OnSendToClientReq<PlayerData, PlayerInputData, Instantiations>,
    >,
    mut ping_w: MessageWriter<PingUpdate>,
    mut state: ResMut<EasyP2PState<PlayerData>>,
    register: Res<SyncedStateRegister>,
    event_register: Res<SyncedEventRegister>,
    mut updates: ResMut<EasyP2PUpdateQueue<PlayerData, PlayerInputData, Instantiations>>,
) where
    PlayerData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    PlayerInputData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
{
    for OnInternalClientData(cid, data) in internal_client_r.read() {
        match data {
            P2PData::ClientLobbyChatMessage(text, _sender) => {
                updates.push(EasyP2PUpdate::ClientChat {
                    client_id: *cid,
                    text: text.clone(),
                });
                if state.is_host {
                    relay_w.write(OnRelayToAllExcept(
                        *cid,
                        P2PData::ClientLobbyChatMessage(text.clone(), NetworkedId::ClientId(*cid)),
                    ));
                }
            }
            P2PData::HostLobbyInfoUpdate(_) => {}
            P2PData::ClientInput(input) => {
                if state.is_host {
                    updates.push(EasyP2PUpdate::ClientInput {
                        sender: NetworkedId::ClientId(*cid),
                        input: input.clone(),
                    });
                }
            }
            P2PData::ClientDataUpdate(data) => {
                if let Some(entry) = state
                    .players
                    .iter_mut()
                    .find(|player| player.id == NetworkedId::ClientId(*cid))
                {
                    entry.data = data.clone();
                }
            }
            P2PData::StateSync(_, _) => {}
            P2PData::EventSync(_, _) => {}
            P2PData::HostInstantiation(_) => {}
            P2PData::PingRequest(timestamp) => {
                // Host receives ping from client, echo it back
                if state.is_host {
                    w_send_client.write(OnSendToClientReq(*cid, P2PData::PingRequest(*timestamp)));
                }
            }
        }
    }
    for OnInternalHostData(data) in internal_host_r.read() {
        match data {
            P2PData::ClientLobbyChatMessage(text, sender) => match sender {
                NetworkedId::Host => {
                    updates.push(EasyP2PUpdate::HostChat { text: text.clone() });
                }
                NetworkedId::ClientId(cid) => {
                    updates.push(EasyP2PUpdate::ClientChat {
                        client_id: *cid,
                        text: text.clone(),
                    });
                }
            },
            P2PData::HostLobbyInfoUpdate(players_data) => {
                state.players = players_data.clone();
                let _ = roster_w.write(OnRosterUpdate(players_data.clone()));
                updates.push(EasyP2PUpdate::RosterUpdated {
                    players: players_data.clone(),
                });
            }
            P2PData::StateSync(type_index, payload) => {
                let idx = *type_index as usize;
                if idx < register.readers.len() {
                    let reader = register.readers[idx];
                    reader(payload, &mut commands);
                }
            }
            P2PData::EventSync(type_index, payload) => {
                let idx = *type_index as usize;
                if idx < event_register.readers.len() {
                    commands.queue(EmitSyncedEvent {
                        index: *type_index,
                        payload: payload.clone(),
                    });
                }
            }
            P2PData::ClientInput(_) => {}
            P2PData::ClientDataUpdate(_) => {}
            P2PData::HostInstantiation(inst) => {
                let local: InstantiationData<Instantiations> = InstantiationData::from(&*inst);
                inst_w.write(HandleInstantiation(local.clone()));
                updates.push(EasyP2PUpdate::Instantiated { data: local });
            }
            P2PData::PingRequest(timestamp) => {
                let elapsed_secs = time.elapsed_secs() - timestamp;
                ping_w.write(PingUpdate(Duration::from_secs_f32(elapsed_secs)));
            }
        }
    }
}

pub(crate) fn send_local_data_after_enter<
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
>(
    mut entered_r: MessageReader<OnLobbyEntered>,
    state: Res<EasyP2PState<PlayerData>>,
    mut w_send_host: MessageWriter<OnSendToHostReq<PlayerData, PlayerInputData, Instantiations>>,
) {
    for OnLobbyEntered(_code) in entered_r.read() {
        if state.is_host {
            continue;
        }
        w_send_host.write(OnSendToHostReq(P2PData::ClientDataUpdate(
            state.local_player_data.clone(),
        )));
    }
}

pub(crate) fn handle_client_data_update_on_host<PlayerData, PlayerInputData, Instantiations>(
    mut internal_client_r: MessageReader<
        OnInternalClientData<PlayerData, PlayerInputData, Instantiations>,
    >,
    mut state: ResMut<EasyP2PState<PlayerData>>,
    mut w_send_all: MessageWriter<OnSendToAllReq<PlayerData, PlayerInputData, Instantiations>>,
    mut roster_w: MessageWriter<OnRosterUpdate<PlayerData>>,
    mut updates: ResMut<EasyP2PUpdateQueue<PlayerData, PlayerInputData, Instantiations>>,
) where
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
    if !state.is_host {
        return;
    }
    for OnInternalClientData(cid, data) in internal_client_r.read() {
        if let P2PData::ClientDataUpdate(client_info) = data {
            let client_id = *cid;
            let mut found = false;
            for entry in state.players.iter_mut() {
                if entry.id == NetworkedId::ClientId(client_id) {
                    entry.data = client_info.clone();
                    found = true;
                    break;
                }
            }
            if !found {
                state.players.push(PlayerInfo::<PlayerData> {
                    id: NetworkedId::ClientId(client_id),
                    data: client_info.clone(),
                });
            }

            let payload = state.get_players(state.is_host);
            w_send_all.write(OnSendToAllReq(P2PData::HostLobbyInfoUpdate(
                payload.clone(),
            )));
            let players = state.get_players(state.is_host);
            let _ = roster_w.write(OnRosterUpdate(players.clone()));
            updates.push(EasyP2PUpdate::RosterUpdated { players });
        }
    }
}

pub(crate) fn host_broadcast_event<E>(
    host_flag: Res<IsHost>,
    mut events: MessageReader<E>,
    register: Res<SyncedEventRegister>,
    mut w_send_all: MessageWriter<OnTransportSendToAll>,
) where
    E: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Message,
{
    if !host_flag.0 {
        return;
    }
    for e in events.read() {
        if let Some(index) = register.indexes.get(&TypeId::of::<E>()) {
            match serde_json::to_string(e) {
                Ok(text) => {
                    if let Ok(payload) =
                        serde_json::to_string(&P2PData::<(), (), ()>::EventSync(*index, text))
                    {
                        w_send_all.write(OnTransportSendToAll(payload));
                    }
                }
                Err(err) => {
                    warn!("Error serializing event for sync: {:?}", err);
                }
            }
        }
    }
}

pub(crate) fn host_broadcast_state_change<S>(
    host_flag: Res<IsHost>,
    current: Res<State<S>>,
    mut last: Local<Option<S>>,
    register: Res<SyncedStateRegister>,
    mut w_send_all: MessageWriter<OnTransportSendToAll>,
) where
    S: States
        + Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + PartialEq
        + Send
        + Sync
        + core::fmt::Debug
        + 'static,
{
    if !host_flag.0 {
        return;
    }
    let current_value = current.get().clone();
    if last.as_ref().map(|v| v == &current_value).unwrap_or(false) {
        return;
    }
    *last = Some(current_value.clone());
    if let Some(index) = register.indexes.get(&TypeId::of::<S>()) {
        if let Ok(text) = serde_json::to_string(&current_value) {
            if let Ok(payload) =
                serde_json::to_string(&P2PData::<(), (), ()>::StateSync(*index, text))
            {
                w_send_all.write(OnTransportSendToAll(payload));
            }
        }
    }
}

pub(crate) struct EmitSyncedEvent {
    pub(crate) index: u8,
    pub(crate) payload: String,
}

impl bevy::ecs::system::Command for EmitSyncedEvent {
    fn apply(self, world: &mut World) {
        let Some(register) = world.get_resource::<SyncedEventRegister>() else {
            return;
        };
        let idx = self.index as usize;
        if idx < register.readers.len() {
            let reader = register.readers[idx];
            reader(&self.payload, world);
        }
    }
}

pub(crate) fn send_ping<
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
>(
    time: Res<Time>,
    state: Res<EasyP2PState<PlayerData>>,
    mut w_send_host: MessageWriter<OnSendToHostReq<PlayerData, PlayerInputData, Instantiations>>,
) {
    if state.is_host {
        return;
    }

    w_send_host.write(OnSendToHostReq(P2PData::PingRequest(time.elapsed_secs())));
}
