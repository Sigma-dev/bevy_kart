use crate::api::{DespawnEntity, EasyP2PData, HandleInstantiation, OnLobbyExit, PingUpdate};
use crate::networked_event::EmitSyncedEvent;
use crate::state::{
    EasyP2PState, InstantiationData, IsHost, NetworkedEntity, NetworkedId, P2PData, P2PLobbyState,
    PlayerInfo, SyncedEventRegister, SyncedStateRegister,
};
use crate::transports::api::{EasyP2PTransportRequest, EasyP2PTransportUpdate};
use crate::updates::EasyP2PUpdate;
use bevy::prelude::*;
use std::time::Duration;

pub(crate) fn on_external_lobby_exit<T: EasyP2PData>(
    mut state: ResMut<EasyP2PState<T::PlayerData>>,
    mut r: MessageReader<OnLobbyExit>,
    mut lobby_state: ResMut<NextState<P2PLobbyState>>,
    mut host_flag: ResMut<IsHost>,
    mut updates: MessageWriter<EasyP2PUpdate<T>>,
) {
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
    state.local_networked_id = None;
    lobby_state.set(P2PLobbyState::OutOfLobby);
    updates.write(EasyP2PUpdate::LobbyExited { reason });
}

pub(crate) fn process_transport_updates<T: EasyP2PData>(
    mut reader: MessageReader<EasyP2PTransportUpdate<P2PData<T>>>,
    mut commands: Commands,
    mut state: ResMut<EasyP2PState<T::PlayerData>>,
    mut lobby_state: ResMut<NextState<P2PLobbyState>>,
    mut host_flag: ResMut<IsHost>,
    mut updates: MessageWriter<EasyP2PUpdate<T>>,
    mut inst_w: MessageWriter<HandleInstantiation<T::Instantiations>>,
    mut despawn_w: MessageWriter<DespawnEntity>,
    mut ping_w: MessageWriter<PingUpdate>,
    mut w_requests: MessageWriter<EasyP2PTransportRequest<P2PData<T>>>,
    register: Res<SyncedStateRegister>,
    event_register: Res<SyncedEventRegister>,
    time: Res<Time>,
) {
    for update in reader.read() {
        match update {
            EasyP2PTransportUpdate::LobbyCreated(code) => {
                state.is_host = true;
                state.lobby_code = code.clone();
                state.local_networked_id = Some(NetworkedId::Host);
                host_flag.0 = true;
                updates.write(EasyP2PUpdate::LobbyCreated { code: code.clone() });
                updates.write(EasyP2PUpdate::LobbyEntered { code: code.clone() });
                lobby_state.set(P2PLobbyState::InLobby);
            }
            EasyP2PTransportUpdate::LobbyJoined(code) => {
                state.is_host = false;
                state.lobby_code = code.clone();
                host_flag.0 = false;
                updates.write(EasyP2PUpdate::LobbyJoined { code: code.clone() });
                updates.write(EasyP2PUpdate::LobbyEntered { code: code.clone() });
                lobby_state.set(P2PLobbyState::InLobby);
            }
            EasyP2PTransportUpdate::LobbyExited => {
                state.is_host = false;
                state.lobby_code.clear();
                state.players.clear();
                state.local_networked_id = None;
                lobby_state.set(P2PLobbyState::OutOfLobby);
                host_flag.0 = false;
                updates.write(EasyP2PUpdate::LobbyExited {
                    reason: crate::api::ExitReason::Disconnected,
                });
            }
            EasyP2PTransportUpdate::RosterUpdated(client_ids) => {
                if state.is_host {
                    state.players.retain(|p| match p.id {
                        NetworkedId::ClientId(cid) => client_ids.contains(&cid),
                        NetworkedId::Host => true,
                    });

                    for player in &state.players {
                        if let NetworkedId::ClientId(cid) = player.id {
                            w_requests.write(EasyP2PTransportRequest::SendToClient(
                                cid,
                                P2PData::ClientIdAssignment(player.id),
                            ));
                        }
                    }

                    let players = state.get_players(state.is_host);
                    w_requests.write(EasyP2PTransportRequest::SendToAll(
                        P2PData::HostLobbyInfoUpdate(players.clone()),
                    ));
                    updates.write(EasyP2PUpdate::RosterUpdated { players });
                }
            }
            EasyP2PTransportUpdate::MessageReceivedFromHost(packet) => {
                handle_packet::<T>(
                    packet,
                    &mut commands,
                    &mut state,
                    &mut updates,
                    &mut inst_w,
                    &mut despawn_w,
                    &mut ping_w,
                    &mut w_requests,
                    &register,
                    &event_register,
                    &time,
                    None,
                );
            }
            EasyP2PTransportUpdate::MessageReceivedFromClient(cid, packet) => {
                handle_packet::<T>(
                    packet,
                    &mut commands,
                    &mut state,
                    &mut updates,
                    &mut inst_w,
                    &mut despawn_w,
                    &mut ping_w,
                    &mut w_requests,
                    &register,
                    &event_register,
                    &time,
                    Some(*cid),
                );
            }
        }
    }
}

fn handle_packet<T: EasyP2PData>(
    packet: &P2PData<T>,
    commands: &mut Commands,
    state: &mut ResMut<EasyP2PState<T::PlayerData>>,
    updates: &mut MessageWriter<EasyP2PUpdate<T>>,
    inst_w: &mut MessageWriter<HandleInstantiation<T::Instantiations>>,
    despawn_w: &mut MessageWriter<DespawnEntity>,
    ping_w: &mut MessageWriter<PingUpdate>,
    w_requests: &mut MessageWriter<EasyP2PTransportRequest<P2PData<T>>>,
    register: &SyncedStateRegister,
    event_register: &SyncedEventRegister,
    time: &Res<Time>,
    sender_id: Option<u64>,
) {
    match packet {
        P2PData::ClientLobbyChatMessage(text, sender) => match sender {
            NetworkedId::Host => {
                updates.write(EasyP2PUpdate::HostChat { text: text.clone() });
            }
            NetworkedId::ClientId(cid) => {
                updates.write(EasyP2PUpdate::ClientChat {
                    client_id: *cid,
                    text: text.clone(),
                });
                if state.is_host {
                    w_requests.write(EasyP2PTransportRequest::SendToAllExcept(
                        *cid,
                        P2PData::ClientLobbyChatMessage(text.clone(), NetworkedId::ClientId(*cid)),
                    ));
                }
            }
        },
        P2PData::HostLobbyInfoUpdate(players_data) => {
            state.players = players_data.clone();
            updates.write(EasyP2PUpdate::RosterUpdated {
                players: players_data.clone(),
            });
        }
        P2PData::ClientIdAssignment(networked_id) => {
            state.local_networked_id = Some(*networked_id);
        }
        P2PData::ClientInput(input) => {
            if state.is_host {
                if let Some(cid) = sender_id {
                    updates.write(EasyP2PUpdate::ClientInput {
                        sender: NetworkedId::ClientId(cid),
                        input: input.clone(),
                    });
                }
            }
        }
        P2PData::ClientDataUpdate(data) => {
            if state.is_host {
                if let Some(cid) = sender_id {
                    let mut found = false;
                    for entry in state.players.iter_mut() {
                        if entry.id == NetworkedId::ClientId(cid) {
                            entry.data = data.clone();
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        state.players.push(PlayerInfo::<T::PlayerData> {
                            id: NetworkedId::ClientId(cid),
                            data: data.clone(),
                        });
                    }

                    w_requests.write(EasyP2PTransportRequest::SendToClient(
                        cid,
                        P2PData::ClientIdAssignment(NetworkedId::ClientId(cid)),
                    ));

                    let payload = state.get_players(state.is_host);
                    w_requests.write(EasyP2PTransportRequest::SendToAll(
                        P2PData::HostLobbyInfoUpdate(payload.clone()),
                    ));
                    let players = state.get_players(state.is_host);
                    updates.write(EasyP2PUpdate::RosterUpdated { players });
                }
            }
        }
        P2PData::StateSync(type_index, payload) => {
            let idx = *type_index as usize;
            if idx < register.readers.len() {
                let reader = register.readers[idx];
                reader(payload, commands);
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
        P2PData::HostInstantiation(inst) => {
            let local: InstantiationData<T::Instantiations> = InstantiationData::from(&*inst);
            inst_w.write(HandleInstantiation(local.clone()));
            updates.write(EasyP2PUpdate::Instantiated { data: local });
        }
        P2PData::HostDespawn(uuid) => {
            despawn_w.write(DespawnEntity(*uuid));
        }
        P2PData::PingRequest(timestamp) => {
            if state.is_host {
                if let Some(cid) = sender_id {
                    w_requests.write(EasyP2PTransportRequest::SendToClient(
                        cid,
                        P2PData::PingRequest(*timestamp),
                    ));
                }
            } else {
                let elapsed_secs = time.elapsed_secs() - timestamp;
                ping_w.write(PingUpdate(Duration::from_secs_f32(elapsed_secs)));
            }
        }
    }
}

pub(crate) fn send_local_data_after_enter<T: EasyP2PData>(
    mut updates_r: MessageReader<EasyP2PUpdate<T>>,
    state: Res<EasyP2PState<T::PlayerData>>,
    mut w_send_host: MessageWriter<EasyP2PTransportRequest<P2PData<T>>>,
) {
    for update in updates_r.read() {
        if let EasyP2PUpdate::LobbyEntered { .. } = update {
            if state.is_host {
                continue;
            }
            w_send_host.write(EasyP2PTransportRequest::SendToHost(
                P2PData::ClientDataUpdate(state.local_player_data.clone()),
            ));
        }
    }
}

pub(crate) fn send_ping<T: EasyP2PData>(
    time: Res<Time>,
    state: Res<EasyP2PState<T::PlayerData>>,
    mut w_send_host: MessageWriter<EasyP2PTransportRequest<P2PData<T>>>,
) {
    if state.is_host {
        return;
    }

    w_send_host.write(EasyP2PTransportRequest::SendToHost(P2PData::PingRequest(
        time.elapsed_secs(),
    )));
}

pub(crate) fn despawn_entity(
    mut commands: Commands,
    mut despawn_w: MessageReader<DespawnEntity>,
    network_entities_q: Query<(Entity, &NetworkedEntity)>,
) {
    for DespawnEntity(uuid) in despawn_w.read() {
        for (entity, networked) in network_entities_q.iter() {
            if networked.uuid == *uuid {
                commands.entity(entity).despawn();
            }
        }
    }
}
