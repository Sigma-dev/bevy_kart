use bevy::prelude::*;
use bevy_easy_p2p::{ClientId, EasyP2PTransportIo, ExitReason};
use bevy_webrtc::{CloseAllConnections, CloseConnection, ConnectionClosed, ConnectionId, ConnectionOpen, IncomingData, SendData};
use wasm_bindgen_futures::spawn_local;

use crate::{
    ensure_room_exists,
    generate_room_code,
    gen_client_id_num,
    FirestoreConfig,
    FirestoreShared,
    NetConnection,
    SignalingState,
    FIRESTORE_INBOX,
};

pub(crate) fn handle_create_join_requests<PlayerData, PlayerInputData, Instantiations>(
    mut io: EasyP2PTransportIo<PlayerData, PlayerInputData, Instantiations>,
    mut sig: ResMut<SignalingState>,
    cfg: Res<FirestoreConfig>,
) {
    if io.take_create_requests() > 0 {
        let room = generate_room_code();
        sig.room_code = room.clone();
        sig.is_host = true;
        sig.answered_clients.clear();
        io.emit_lobby_created(room.clone());
        io.emit_lobby_entered(room.clone());
        let cfg = cfg.clone();
        spawn_local(async move {
            ensure_room_exists(&cfg, &room).await;
            FIRESTORE_INBOX.with(|inbox| {
                inbox
                    .borrow_mut()
                    .push(serde_json::json!({"__status":"created"}))
            });
        });
    }

    for room in io.take_join_requests() {
        sig.room_code = room.clone();
        sig.is_host = false;
        sig.client_id = Some(gen_client_id_num().to_string());
        sig.client_answer_applied = false;
        sig.client_join_pending = true;
    }
}

pub(crate) fn handle_send_requests<PlayerData, PlayerInputData, Instantiations>(
    mut io: EasyP2PTransportIo<PlayerData, PlayerInputData, Instantiations>,
    mut w_send: MessageWriter<SendData>,
    q_conns: Query<(Entity, &NetConnection)>,
    sig: Res<SignalingState>,
) {
    for text in io.take_send_to_all() {
        for (_, c) in q_conns.iter() {
            w_send.write(SendData {
                id: c.id,
                text: text.clone(),
            });
        }
    }

    let host_payloads = io.take_send_to_host();
    if let Some(single) = only_connection_ids(&q_conns) {
        for text in host_payloads {
            w_send.write(SendData { id: single, text });
        }
    }

    for (client_id, text) in io.take_send_to_client() {
        if !sig.is_host {
            continue;
        }
        let target = client_id.to_string();
        if let Some((&conn_raw, _)) = sig
            .host_connection_to_client_id
            .iter()
            .find(|(_, cid)| *cid == &target)
        {
            let id = ConnectionId(conn_raw);
            w_send.write(SendData { id, text });
        }
    }

    for (sender, text) in io.take_relay_to_all_except() {
        if !sig.is_host {
            continue;
        }
        let sender_str = sender.to_string();
        for (conn_raw, cid_str) in sig.host_connection_to_client_id.iter() {
            if cid_str == &sender_str {
                continue;
            }
            w_send.write(SendData {
                id: ConnectionId(*conn_raw),
                text: text.clone(),
            });
        }
    }
}

pub(crate) fn handle_exit_requests<PlayerData, PlayerInputData, Instantiations>(
    mut commands: Commands,
    mut io: EasyP2PTransportIo<PlayerData, PlayerInputData, Instantiations>,
    mut w_close_all: MessageWriter<CloseAllConnections>,
    mut sig: ResMut<SignalingState>,
    q_conns: Query<(Entity, &NetConnection)>,
) {
    if io.take_exit_requests() == 0 {
        return;
    }
    w_close_all.write(CloseAllConnections);
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
    FIRESTORE_INBOX.with(|inbox| inbox.borrow_mut().clear());
}

pub(crate) fn handle_kick_requests<PlayerData, PlayerInputData, Instantiations>(
    mut commands: Commands,
    mut io: EasyP2PTransportIo<PlayerData, PlayerInputData, Instantiations>,
    mut w_close_one: MessageWriter<CloseConnection>,
    mut sig: ResMut<SignalingState>,
    q_conns: Query<(Entity, &NetConnection)>,
) {
    for client_id in io.take_kick_requests() {
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
        let Some(conn_raw) = to_remove else {
            continue;
        };
        let conn = ConnectionId(conn_raw);
        w_close_one.write(CloseConnection { id: conn });
        for (e, c) in q_conns.iter() {
            if c.id == conn {
                commands.entity(e).despawn();
            }
        }
        sig.host_connection_to_client_id.remove(&conn_raw);
        sig.answered_clients.remove(&target);
        sig.joined_clients.remove(&target);
        let list: Vec<String> = sig.joined_clients.iter().cloned().collect();
        io.emit_roster_changed(list);
    }
}

pub(crate) fn log_connection_open<PlayerData, PlayerInputData, Instantiations>(
    mut r: MessageReader<ConnectionOpen>,
    mut sig: ResMut<SignalingState>,
    mut io: EasyP2PTransportIo<PlayerData, PlayerInputData, Instantiations>,
) {
    for ConnectionOpen(id) in r.read() {
        if sig.is_host {
            if let Some(cid_str) = sig.host_connection_to_client_id.get(&id.0).cloned() {
                sig.joined_clients.insert(cid_str);
                let list: Vec<String> = sig.joined_clients.iter().cloned().collect();
                io.emit_roster_changed(list);
            }
        } else if !sig.client_emitted_join {
            let room = sig.room_code.clone();
            io.emit_lobby_joined(room.clone());
            io.emit_lobby_entered(room);
            sig.client_emitted_join = true;
        }
    }
}

pub(crate) fn handle_connection_closed<PlayerData, PlayerInputData, Instantiations>(
    mut commands: Commands,
    mut r: MessageReader<ConnectionClosed>,
    mut sig: ResMut<SignalingState>,
    mut io: EasyP2PTransportIo<PlayerData, PlayerInputData, Instantiations>,
    q_conns: Query<(Entity, &NetConnection)>,
) {
    for ConnectionClosed(id) in r.read() {
        for (e, c) in q_conns.iter() {
            if c.id == *id {
                commands.entity(e).despawn();
            }
        }
        if sig.is_host {
            if let Some(cid_str) = sig.host_connection_to_client_id.remove(&id.0) {
                sig.answered_clients.remove(&cid_str);
                sig.joined_clients.remove(&cid_str);
                let list: Vec<String> = sig.joined_clients.iter().cloned().collect();
                io.emit_roster_changed(list);
            }
        } else {
            io.emit_lobby_exit(ExitReason::Disconnected);
        }
    }
}

pub(crate) fn log_incoming_data<PlayerData, PlayerInputData, Instantiations>(
    mut r: MessageReader<IncomingData>,
    sig: Res<SignalingState>,
    mut io: EasyP2PTransportIo<PlayerData, PlayerInputData, Instantiations>,
) {
    for IncomingData { id, text } in r.read() {
        if sig.is_host {
            if let Some(cid_str) = sig.host_connection_to_client_id.get(&id.0) {
                if let Ok(cid) = cid_str.parse::<ClientId>() {
                    io.emit_incoming_from_client(cid, text.clone());
                }
            }
        } else {
            io.emit_incoming_from_host(text.clone());
        }
    }
}

pub(crate) fn on_lobby_exit_cleanup<PlayerData, PlayerInputData, Instantiations>(
    mut commands: Commands,
    mut io: EasyP2PTransportIo<PlayerData, PlayerInputData, Instantiations>,
    mut sig: ResMut<SignalingState>,
    mut shared: ResMut<FirestoreShared>,
    mut w_close_all: MessageWriter<CloseAllConnections>,
    q_conns: Query<(Entity, &NetConnection)>,
) {
    if io.take_lobby_exit_events().is_empty() {
        return;
    }
    w_close_all.write(CloseAllConnections);
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

fn only_connection_ids(q: &Query<(Entity, &NetConnection)>) -> Option<ConnectionId> {
    let mut it = q.iter();
    let first = it.next()?.1;
    if it.next().is_none() {
        Some(first.id)
    } else {
        None
    }
}

