use bevy::prelude::*;
use bevy_webrtc::{ConnectionId, WebRtc, WebRtcUpdate};
use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen_futures::spawn_local;

use crate::ClientId;
use crate::transports::api::{EasyP2PTransportIo, EasyP2PTransportRequest, EasyP2PTransportUpdate};

use super::structs::FirestoreInboxMessage;
use super::{
    FIRESTORE_INBOX, FirestoreConfig, FirestoreShared, NetConnection, SignalingState,
    ensure_room_exists, gen_client_id_num, generate_room_code, write_answer, write_offer,
};

fn cleanup_transport(
    commands: &mut Commands,
    webrtc: &mut WebRtc,
    sig: &mut SignalingState,
    shared: &mut FirestoreShared,
    q_conns: &Query<(Entity, &NetConnection)>,
) {
    webrtc.close_all();
    for (e, _) in q_conns.iter() {
        commands.entity(e).despawn();
    }
    sig.room_code.clear();
    sig.is_host = false;
    sig.answered_clients.clear();
    sig.joined_clients.clear();
    sig.client_id = None;
    sig.client_answer_applied = false;
    sig.offer_conn = None;
    sig.client_join_pending = false;
    sig.client_emitted_join = false;
    sig.host_connection_to_client_id.clear();
    shared.in_flight = false;
    shared.next_allowed_fetch_at_ms = 0.0;
    shared.not_found_logged = false;
    shared.room_exists = false;
    FIRESTORE_INBOX.with(|inbox| inbox.borrow_mut().clear());
}

pub(crate) fn handle_transport_requests<Packet>(
    mut commands: Commands,
    mut io: EasyP2PTransportIo<Packet>,
    mut webrtc: WebRtc,
    mut sig: ResMut<SignalingState>,
    mut shared: ResMut<FirestoreShared>,
    cfg: Res<FirestoreConfig>,
    q_conns: Query<(Entity, &NetConnection)>,
) where
    Packet: Send + Sync + 'static + Serialize + DeserializeOwned,
{
    let mut updates = Vec::new();
    for req in io.take_requests() {
        match req {
            EasyP2PTransportRequest::CreateLobby => {
                let room = generate_room_code();
                sig.room_code = room.clone();
                sig.is_host = true;
                sig.answered_clients.clear();
                updates.push(EasyP2PTransportUpdate::LobbyCreated(room.clone()));

                let cfg = cfg.clone();
                spawn_local(async move {
                    ensure_room_exists(&cfg, &room).await;
                    FIRESTORE_INBOX
                        .with(|inbox| inbox.borrow_mut().push(FirestoreInboxMessage::RoomCreated));
                });
            }
            EasyP2PTransportRequest::JoinLobby(room) => {
                cleanup_transport(&mut commands, &mut webrtc, &mut sig, &mut shared, &q_conns);
                sig.room_code = room.clone();
                sig.is_host = false;
                sig.client_id = Some(gen_client_id_num().to_string());
                sig.client_answer_applied = false;
                sig.client_join_pending = true;
            }
            EasyP2PTransportRequest::SendToAll(packet) => {
                if let Ok(text) = serde_json::to_string(packet) {
                    for (_, c) in q_conns.iter() {
                        webrtc.send_text(c.id, text.clone());
                    }
                }
            }
            EasyP2PTransportRequest::SendToHost(packet) => {
                if let Ok(text) = serde_json::to_string(packet) {
                    if let Some(single) = only_connection_ids(&q_conns) {
                        webrtc.send_text(single, text);
                    }
                }
            }
            EasyP2PTransportRequest::SendToClient(client_id, packet) => {
                if !sig.is_host {
                    continue;
                }
                if let Ok(text) = serde_json::to_string(packet) {
                    let target = client_id.to_string();
                    if let Some((&conn_raw, _)) = sig
                        .host_connection_to_client_id
                        .iter()
                        .find(|(_, cid)| *cid == &target)
                    {
                        webrtc.send_text(ConnectionId(conn_raw), text);
                    }
                }
            }
            EasyP2PTransportRequest::SendToAllExcept(except_cid, packet) => {
                if !sig.is_host {
                    continue;
                }
                if let Ok(text) = serde_json::to_string(packet) {
                    let except_str = except_cid.to_string();
                    for (conn_raw, cid_str) in sig.host_connection_to_client_id.iter() {
                        if cid_str == &except_str {
                            continue;
                        }
                        webrtc.send_text(ConnectionId(*conn_raw), text.clone());
                    }
                }
            }
            EasyP2PTransportRequest::ExitLobby => {
                cleanup_transport(&mut commands, &mut webrtc, &mut sig, &mut shared, &q_conns);
                updates.push(EasyP2PTransportUpdate::LobbyExited);
            }
            EasyP2PTransportRequest::Kick(client_id) => {
                if !sig.is_host {
                    continue;
                }
                let target = client_id.to_string();
                let mut to_remove: Option<u64> = None;
                for (cid_conn, cid_str) in sig.host_connection_to_client_id.iter() {
                    if cid_str == &target {
                        to_remove = Some(*cid_conn);
                        break;
                    }
                }
                if let Some(conn_raw) = to_remove {
                    let conn = ConnectionId(conn_raw);
                    webrtc.close(conn);
                    for (e, c) in q_conns.iter() {
                        if c.id == conn {
                            commands.entity(e).despawn();
                        }
                    }
                    sig.host_connection_to_client_id.remove(&conn_raw);
                    sig.answered_clients.remove(&target);
                    sig.joined_clients.remove(&target);

                    let mut list: Vec<ClientId> = Vec::new();
                    for cid_str in sig.joined_clients.iter() {
                        if let Ok(cid) = cid_str.parse::<ClientId>() {
                            list.push(cid);
                        }
                    }
                    updates.push(EasyP2PTransportUpdate::RosterUpdated(list));
                }
            }
        }
    }
    for update in updates {
        io.emit_update(update);
    }
}

pub(crate) fn handle_webrtc_events<Packet>(
    mut commands: Commands,
    mut webrtc: WebRtc,
    mut sig: ResMut<SignalingState>,
    mut shared: ResMut<FirestoreShared>,
    cfg: Res<FirestoreConfig>,
    mut io: EasyP2PTransportIo<Packet>,
    q_conns: Query<(Entity, &NetConnection)>,
) where
    Packet: Send + Sync + 'static + Serialize + DeserializeOwned,
{
    let updates = webrtc.read_updates();

    for update in updates {
        match update {
            WebRtcUpdate::LocalSdp { id, sdp } => {
                if q_conns.iter().all(|(_, c)| c.id != id) {
                    commands.spawn(NetConnection { id });
                }
                if !sig.is_host && sig.offer_conn.is_none() {
                    sig.offer_conn = Some(id);
                }
                if !sig.room_code.is_empty() && !sig.is_host {
                    if let Some(cid) = sig.client_id.clone() {
                        let cfg = cfg.clone();
                        let room = sig.room_code.clone();
                        let sdp_text = sdp.clone();
                        spawn_local(async move {
                            ensure_room_exists(&cfg, &room).await;
                            write_offer(&cfg, &room, &cid, &sdp_text).await;
                        });
                    }
                }
                if sig.is_host {
                    if let Some(client_id) = sig.host_connection_to_client_id.get(&id.0).cloned() {
                        let cfg = cfg.clone();
                        let room = sig.room_code.clone();
                        let sdp_text = sdp.clone();
                        spawn_local(async move {
                            ensure_room_exists(&cfg, &room).await;
                            write_answer(&cfg, &room, &client_id, &sdp_text).await;
                        });
                    }
                }
            }
            WebRtcUpdate::ConnectionOpen(id) => {
                if sig.is_host {
                    if let Some(cid_str) = sig.host_connection_to_client_id.get(&id.0).cloned() {
                        sig.joined_clients.insert(cid_str);
                        let mut list: Vec<ClientId> = Vec::new();
                        for cid_str in sig.joined_clients.iter() {
                            if let Ok(cid) = cid_str.parse::<ClientId>() {
                                list.push(cid);
                            }
                        }
                        io.emit_update(EasyP2PTransportUpdate::RosterUpdated(list));
                    }
                } else if !sig.client_emitted_join {
                    let room = sig.room_code.clone();
                    io.emit_update(EasyP2PTransportUpdate::LobbyJoined(room.clone()));
                    sig.client_emitted_join = true;
                }
            }
            WebRtcUpdate::ConnectionClosed(id) => {
                for (e, c) in q_conns.iter() {
                    if c.id == id {
                        commands.entity(e).despawn();
                    }
                }
                if sig.is_host {
                    if let Some(cid_str) = sig.host_connection_to_client_id.remove(&id.0) {
                        sig.answered_clients.remove(&cid_str);
                        sig.joined_clients.remove(&cid_str);
                        let mut list: Vec<ClientId> = Vec::new();
                        for cid_str in sig.joined_clients.iter() {
                            if let Ok(cid) = cid_str.parse::<ClientId>() {
                                list.push(cid);
                            }
                        }
                        io.emit_update(EasyP2PTransportUpdate::RosterUpdated(list));
                    }
                } else {
                    cleanup_transport(&mut commands, &mut webrtc, &mut sig, &mut shared, &q_conns);
                    io.emit_update(EasyP2PTransportUpdate::LobbyExited);
                }
            }
            WebRtcUpdate::IncomingData { id, data } => {
                if let Ok(packet) = serde_json::from_str::<Packet>(&data) {
                    if sig.is_host {
                        if let Some(cid_str) = sig.host_connection_to_client_id.get(&id.0) {
                            if let Ok(cid) = cid_str.parse::<ClientId>() {
                                io.emit_update(EasyP2PTransportUpdate::MessageReceivedFromClient(
                                    cid, packet,
                                ));
                            }
                        }
                    } else {
                        io.emit_update(EasyP2PTransportUpdate::MessageReceivedFromHost(packet));
                    }
                } else {
                    warn!("Failed to deserialize packet: {}", data);
                }
            }
        }
    }
}

fn only_connection_ids(q: &Query<(Entity, &NetConnection)>) -> Option<ConnectionId> {
    let mut it = q.iter();
    let first = it.next()?.1;
    if it.next().is_none() {
        Some(first.id)
    } else {
        None
    }
}
