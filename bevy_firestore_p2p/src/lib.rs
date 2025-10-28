use bevy::prelude::*;
use bevy_easy_p2p::{
    ClientId, ExitReason, OnCreateLobbyReq, OnExitLobbyReq, OnJoinLobbyReq, OnKickReq,
    OnLobbyCreated, OnLobbyEntered, OnLobbyExit, OnLobbyJoined, OnTransportIncomingFromClient,
    OnTransportIncomingFromHost, OnTransportRelayToAllExcept, OnTransportRosterChanged,
    OnTransportSendToAll, OnTransportSendToClient, OnTransportSendToHost, P2PTransport,
};
use bevy_webrtc::{
    CloseAllConnections, CloseConnection, ConnectionClosed, ConnectionId, ConnectionOpen,
    CreateAnswer, CreateOffer, IncomingData, LocalSdpReady, SendData, SetRemote, WebRtcPlugin,
};
use serde_json::json;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, RequestMode, Response};

thread_local! {
    static FIRESTORE_INBOX: RefCell<Vec<serde_json::Value>> = RefCell::new(Vec::new());
}

#[derive(Resource, Default)]
struct ConnectionIdAllocator(u64);

impl ConnectionIdAllocator {
    fn allocate(&mut self) -> ConnectionId {
        self.0 += 1;
        ConnectionId(self.0)
    }
}

#[derive(Component, Copy, Clone)]
struct NetConnection {
    id: ConnectionId,
}

#[derive(Resource, Clone)]
pub struct FirestoreConfig {
    pub project_id: String,
}

impl Default for FirestoreConfig {
    fn default() -> Self {
        Self {
            project_id: "p2p-relay".to_string(),
        }
    }
}

#[derive(Resource, Default)]
struct SignalingState {
    room_code: String,
    is_host: bool,
    answered_clients: HashSet<String>,
    joined_clients: HashSet<String>,
    client_id: Option<String>,
    client_answer_applied: bool,
    offer_conn: Option<ConnectionId>,
    host_connection_to_client_id: HashMap<u64, String>,
    client_join_pending: bool,
    // Track if client has emitted OnLobbyJoined/OnLobbyEntered
    client_emitted_join: bool,
}

#[derive(Resource, Default)]
struct FirestoreShared {
    in_flight: bool,
    next_allowed_fetch_at_ms: f64,
    not_found_logged: bool,
    room_exists: bool,
}

pub struct FirestoreP2PPlugin;

impl Plugin for FirestoreP2PPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConnectionIdAllocator>()
            .init_resource::<FirestoreConfig>()
            .init_resource::<SignalingState>()
            .init_resource::<FirestoreShared>()
            .add_plugins(WebRtcPlugin)
            .add_systems(Update, handle_create_join_requests)
            .add_systems(Update, handle_send_requests)
            .add_systems(Update, handle_exit_requests)
            .add_systems(Update, handle_kick_requests)
            .add_systems(Update, log_local_sdp_ready)
            .add_systems(Update, log_connection_open)
            .add_systems(Update, log_incoming_data)
            .add_systems(Update, handle_connection_closed)
            .add_systems(Update, firestore_pump)
            .add_systems(Update, on_lobby_exit_cleanup);
    }
}

fn generate_room_code() -> String {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut out = String::with_capacity(6);
    for _ in 0..6 {
        let r = (js_sys::Math::random() * (ALPHABET.len() as f64)).floor() as usize;
        out.push(ALPHABET.as_bytes()[r] as char);
    }
    out
}

fn gen_client_id_num() -> u64 {
    // 53 bits of randomness via Math.random chunks
    let a = (js_sys::Math::random() * (1u64 << 26) as f64).floor() as u64;
    let b = (js_sys::Math::random() * (1u64 << 27) as f64).floor() as u64;
    (a << 27) | b
}

fn firestore_room_doc_url(cfg: &FirestoreConfig, room: &str) -> String {
    format!(
        "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents/rooms/{}",
        cfg.project_id, room
    )
}

fn firestore_patch_url(cfg: &FirestoreConfig, room: &str, mask: &str) -> String {
    format!(
        "{}?updateMask.fieldPaths={}",
        firestore_room_doc_url(cfg, room),
        mask
    )
}

async fn http_fetch_json(
    method: &str,
    url: &str,
    body: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    let window = web_sys::window()?;
    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(RequestMode::Cors);
    if let Some(b) = body {
        let headers = Headers::new().ok()?;
        headers.set("Content-Type", "application/json").ok()?;
        init.set_headers(&headers);
        init.set_body(&JsValue::from_str(&b.to_string()));
    }
    let request = Request::new_with_str_and_init(url, &init).ok()?;
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .ok()?;
    let resp: Response = resp_value.dyn_into().ok()?;
    if !resp.ok() {
        return None;
    }
    let json = JsFuture::from(resp.json().ok()?).await.ok()?;
    let val: serde_json::Value = serde_wasm_bindgen::from_value(json).ok()?;
    Some(val)
}

async fn ensure_room_exists(cfg: &FirestoreConfig, room: &str) {
    let url = firestore_room_doc_url(cfg, room);
    let body = json!({
        "fields": {
            "offers": {"mapValue": {"fields": {}}},
            "answers": {"mapValue": {"fields": {}}}
        }
    });
    let _ = http_fetch_json("PATCH", &url, Some(body)).await;
}

async fn write_offer(cfg: &FirestoreConfig, room: &str, client_id: &str, sdp: &str) {
    let url = firestore_patch_url(cfg, room, "offers");
    let body = json!({
        "fields": {
            "offers": {"mapValue": {"fields": {
                client_id: {"stringValue": sdp}
            }}}
        }
    });
    let _ = http_fetch_json("PATCH", &url, Some(body)).await;
}

async fn write_answer(cfg: &FirestoreConfig, room: &str, client_id: &str, sdp: &str) {
    let url = firestore_patch_url(cfg, room, "answers");
    let body = json!({
        "fields": {
            "answers": {"mapValue": {"fields": {
                client_id: {"stringValue": sdp}
            }}}
        }
    });
    let _ = http_fetch_json("PATCH", &url, Some(body)).await;
}

async fn read_room(cfg: &FirestoreConfig, room: &str) -> Option<serde_json::Value> {
    http_fetch_json("GET", &firestore_room_doc_url(cfg, room), None).await
}

fn now_ms() -> f64 {
    js_sys::Date::now()
}

fn handle_create_join_requests(
    mut create_r: MessageReader<OnCreateLobbyReq>,
    mut join_r: MessageReader<OnJoinLobbyReq>,
    mut _w_offer: MessageWriter<CreateOffer>,
    mut _id_alloc: ResMut<ConnectionIdAllocator>,
    mut sig: ResMut<SignalingState>,
    cfg: Res<FirestoreConfig>,
    mut lobby_created: MessageWriter<OnLobbyCreated>,
    mut lobby_entered: MessageWriter<OnLobbyEntered>,
) {
    for _ in create_r.read() {
        let room = generate_room_code();
        sig.room_code = room.clone();
        sig.is_host = true;
        sig.answered_clients.clear();
        lobby_created.write(OnLobbyCreated(room.clone()));
        lobby_entered.write(OnLobbyEntered(room.clone()));
        let cfg = cfg.clone();
        wasm_bindgen_futures::spawn_local(async move {
            ensure_room_exists(&cfg, &room).await;
            FIRESTORE_INBOX.with(|inbox| {
                inbox
                    .borrow_mut()
                    .push(serde_json::json!({"__status":"created"}))
            });
        });
    }

    for req in join_r.read() {
        let room = req.0.clone();
        sig.room_code = room.clone();
        sig.is_host = false;
        sig.client_id = Some(gen_client_id_num().to_string());
        sig.client_answer_applied = false;
        // Delay entering lobby until we verify the room exists
        sig.client_join_pending = true;
        // Do not emit OnLobbyJoined/OnLobbyEntered or create an offer yet
        // The offer will be created in firestore_pump once the room is confirmed to exist
    }
}

fn handle_send_requests(
    mut send_host_r: MessageReader<OnTransportSendToHost>,
    mut send_all_r: MessageReader<OnTransportSendToAll>,
    mut send_client_r: MessageReader<OnTransportSendToClient>,
    mut relay_except_r: MessageReader<OnTransportRelayToAllExcept>,
    mut w_send: MessageWriter<SendData>,
    q_conns: Query<(Entity, &NetConnection)>,
    sig: Res<SignalingState>,
) {
    for OnTransportSendToAll(text) in send_all_r.read() {
        for (_, c) in q_conns.iter() {
            w_send.write(SendData {
                id: c.id,
                text: text.clone(),
            });
        }
    }

    for OnTransportSendToHost(text) in send_host_r.read() {
        if let Some(single) = only_connection_ids(&q_conns) {
            w_send.write(SendData {
                id: single,
                text: text.clone(),
            });
        }
    }

    // Send to specific client (host only)
    for OnTransportSendToClient(client_id, text) in send_client_r.read() {
        if !sig.is_host {
            continue;
        }
        let target = client_id.to_string();
        // Find the connection mapped to this client id
        if let Some((&conn_raw, _)) = sig
            .host_connection_to_client_id
            .iter()
            .find(|(_, cid)| *cid == &target)
        {
            let id = ConnectionId(conn_raw);
            w_send.write(SendData {
                id,
                text: text.clone(),
            });
        }
    }

    // Relay to all except the sender (host only)
    for OnTransportRelayToAllExcept(sender, text) in relay_except_r.read() {
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

fn handle_exit_requests(
    mut commands: Commands,
    mut exit_r: MessageReader<OnExitLobbyReq>,
    mut w_close_all: MessageWriter<CloseAllConnections>,
    mut sig: ResMut<SignalingState>,
    q_conns: Query<(Entity, &NetConnection)>,
) {
    for OnExitLobbyReq in exit_r.read() {
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
}

fn handle_kick_requests(
    mut commands: Commands,
    mut kick_r: MessageReader<OnKickReq>,
    mut w_close_one: MessageWriter<CloseConnection>,
    mut sig: ResMut<SignalingState>,
    mut lobby_info_w: MessageWriter<OnTransportRosterChanged>,
    q_conns: Query<(Entity, &NetConnection)>,
) {
    for OnKickReq(client_id) in kick_r.read() {
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
        let conn_raw = to_remove.unwrap();
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
        lobby_info_w.write(OnTransportRosterChanged(list));
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

fn log_local_sdp_ready(
    mut r: MessageReader<LocalSdpReady>,
    mut commands: Commands,
    mut sig: ResMut<SignalingState>,
    cfg: Res<FirestoreConfig>,
) {
    for LocalSdpReady { id, sdp } in r.read() {
        commands.spawn(NetConnection { id: *id });
        if !sig.is_host && sig.offer_conn.is_none() {
            sig.offer_conn = Some(*id);
        }
        if !sig.room_code.is_empty() && !sig.is_host {
            if let Some(cid) = sig.client_id.clone() {
                let cfg = cfg.clone();
                let room = sig.room_code.clone();
                let sdp_text = sdp.clone();
                wasm_bindgen_futures::spawn_local(async move {
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
                wasm_bindgen_futures::spawn_local(async move {
                    ensure_room_exists(&cfg, &room).await;
                    write_answer(&cfg, &room, &client_id, &sdp_text).await;
                });
            }
        }
    }
}

fn log_connection_open(
    mut r: MessageReader<ConnectionOpen>,
    mut sig: ResMut<SignalingState>,
    mut lobby_info_w: MessageWriter<OnTransportRosterChanged>,
    mut lobby_joined: MessageWriter<OnLobbyJoined>,
    mut lobby_entered: MessageWriter<OnLobbyEntered>,
) {
    for ConnectionOpen(id) in r.read() {
        info!("Connection {:?} data channel open", id);
        if sig.is_host {
            if let Some(cid_str) = sig.host_connection_to_client_id.get(&id.0).cloned() {
                sig.joined_clients.insert(cid_str);
                let list: Vec<String> = sig.joined_clients.iter().cloned().collect();
                lobby_info_w.write(OnTransportRosterChanged(list));
            }
        } else if !sig.client_emitted_join {
            // Client: emit lobby joined/entered only once when channel becomes ready
            let room = sig.room_code.clone();
            lobby_joined.write(OnLobbyJoined(room.clone()));
            lobby_entered.write(OnLobbyEntered(room));
            sig.client_emitted_join = true;
        }
    }
}

fn handle_connection_closed(
    mut commands: Commands,
    mut r: MessageReader<ConnectionClosed>,
    mut sig: ResMut<SignalingState>,
    mut lobby_info_w: MessageWriter<OnTransportRosterChanged>,
    mut lobby_exit_w: MessageWriter<OnLobbyExit>,
    q_conns: Query<(Entity, &NetConnection)>,
) {
    for ConnectionClosed(id) in r.read() {
        // Despawn matching NetConnection entity
        for (e, c) in q_conns.iter() {
            if c.id == *id {
                commands.entity(e).despawn();
            }
        }
        if sig.is_host {
            // Remove client from host maps and emit updated lobby info
            if let Some(cid_str) = sig.host_connection_to_client_id.remove(&id.0) {
                sig.answered_clients.remove(&cid_str);
                sig.joined_clients.remove(&cid_str);
                let list: Vec<String> = sig.joined_clients.iter().cloned().collect();
                lobby_info_w.write(OnTransportRosterChanged(list));
            }
        } else {
            // If client's only connection closed, exit lobby (disconnected)
            // Rely on EasyP2P timeout removal: emit OnLobbyExit here for immediate UX
            // Keep transport state cleanup to OnLobbyExit handler
            // Note: We avoid double-cleaning Firestore state here.
            lobby_exit_w.write(OnLobbyExit(ExitReason::Disconnected));
        }
    }
}

fn log_incoming_data(
    mut r: MessageReader<IncomingData>,
    sig: Res<SignalingState>,
    mut ev_client: MessageWriter<OnTransportIncomingFromClient>,
    mut ev_host: MessageWriter<OnTransportIncomingFromHost>,
) {
    for IncomingData { id, text } in r.read() {
        if sig.is_host {
            if let Some(cid_str) = sig.host_connection_to_client_id.get(&id.0) {
                if let Ok(cid) = cid_str.parse::<u64>() {
                    ev_client.write(OnTransportIncomingFromClient(cid, text.clone()));
                }
            }
        } else {
            ev_host.write(OnTransportIncomingFromHost(text.clone()));
        }
    }
}

fn firestore_pump(
    mut sig: ResMut<SignalingState>,
    cfg: Res<FirestoreConfig>,
    mut w_answer: MessageWriter<CreateAnswer>,
    mut w_set: MessageWriter<SetRemote>,
    mut id_alloc: ResMut<ConnectionIdAllocator>,
    mut shared: ResMut<FirestoreShared>,
    mut w_offer: MessageWriter<CreateOffer>,
) {
    if sig.room_code.is_empty() {
        return;
    }

    let mut drained_docs: Vec<serde_json::Value> = Vec::new();
    FIRESTORE_INBOX.with(|inbox| {
        let mut buf = inbox.borrow_mut();
        drained_docs.extend(buf.drain(..));
    });
    if !drained_docs.is_empty() {
        shared.in_flight = false;
    }
    for doc in drained_docs {
        if let Some(status) = doc.get("__status").and_then(|v| v.as_str()) {
            if status == "created" {
                shared.room_exists = true;
                continue;
            }
        }
        if let Some(status) = doc.get("__status").and_then(|v| v.as_str()) {
            if status == "not_found" {
                let now = now_ms();
                if shared.next_allowed_fetch_at_ms < now + 1500.0 {
                    shared.next_allowed_fetch_at_ms = now + 1500.0;
                }
                if !shared.not_found_logged {
                    info!(
                        "Room '{}' not found yet. Waiting for host...",
                        sig.room_code
                    );
                    shared.not_found_logged = true;
                }
                continue;
            }
        }
        if !doc.is_null() {
            apply_firestore_doc(&doc, &mut sig, &mut w_answer, &mut w_set, &mut id_alloc);
            // If we are a client waiting to join and the room exists, create offer only.
            // Delay emitting OnLobbyJoined/OnLobbyEntered until data channel opens.
            if sig.client_join_pending && !sig.is_host {
                sig.client_join_pending = false;
                sig.client_emitted_join = false;
                let id = id_alloc.allocate();
                sig.offer_conn = Some(id);
                w_offer.write(CreateOffer { id });
            }
        }
    }

    let now = now_ms();
    if shared.in_flight || now < shared.next_allowed_fetch_at_ms {
        return;
    }
    if sig.is_host && !shared.room_exists {
        return;
    }
    shared.in_flight = true;
    shared.next_allowed_fetch_at_ms = now + 500.0;
    let room = sig.room_code.clone();
    let cfg_owned = cfg.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let result = read_room(&cfg_owned, &room).await;
        let pushed = result.unwrap_or_else(|| serde_json::Value::Null);
        FIRESTORE_INBOX.with(|inbox| inbox.borrow_mut().push(pushed));
    });
}

fn apply_firestore_doc(
    doc: &serde_json::Value,
    sig: &mut SignalingState,
    w_answer: &mut MessageWriter<CreateAnswer>,
    w_set: &mut MessageWriter<SetRemote>,
    id_alloc: &mut ResMut<ConnectionIdAllocator>,
) {
    let fields = doc.get("fields");
    if let Some(fields) = fields {
        if sig.is_host {
            if let Some(offers) = fields
                .get("offers")
                .and_then(|m| m.get("mapValue"))
                .and_then(|m| m.get("fields"))
            {
                if let Some(map) = offers.as_object() {
                    for (cid, val) in map.iter() {
                        if sig.answered_clients.contains(cid) {
                            continue;
                        }
                        if let Some(sdp) = val.get("stringValue").and_then(|v| v.as_str()) {
                            let id = id_alloc.allocate();
                            w_answer.write(CreateAnswer {
                                id,
                                remote_sdp: sdp.to_string(),
                            });
                            sig.answered_clients.insert(cid.clone());
                            sig.host_connection_to_client_id.insert(id.0, cid.clone());
                        }
                    }
                }
            }
        } else if let Some(cid) = sig.client_id.clone() {
            if !sig.client_answer_applied {
                if let Some(answers) = fields
                    .get("answers")
                    .and_then(|m| m.get("mapValue"))
                    .and_then(|m| m.get("fields"))
                {
                    if let Some(val) = answers.get(&cid) {
                        if let Some(sdp) = val.get("stringValue").and_then(|v| v.as_str()) {
                            let target = sig.offer_conn.unwrap_or_else(|| id_alloc.allocate());
                            w_set.write(SetRemote {
                                id: target,
                                sdp: sdp.to_string(),
                            });
                            sig.client_answer_applied = true;
                        }
                    }
                }
            }
        }
    }
}

fn on_lobby_exit_cleanup(
    mut commands: Commands,
    mut r: MessageReader<OnLobbyExit>,
    mut sig: ResMut<SignalingState>,
    mut shared: ResMut<FirestoreShared>,
    mut w_close_all: MessageWriter<CloseAllConnections>,
    q_conns: Query<(Entity, &NetConnection)>,
) {
    let mut any = false;
    for _ in r.read() {
        any = true;
    }
    if !any {
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

// Minimal P2PTransport impl (no-ops; actual work driven by systems above)
pub struct FirestoreWebRtcTransport;

impl P2PTransport for FirestoreWebRtcTransport {
    type Error = ();
    fn create_lobby(_world: &mut World) -> Result<String, Self::Error> {
        Ok(String::new())
    }
    fn join_lobby(_world: &mut World, _code: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn exit_lobby(_world: &mut World) {}
    fn send_to_host(_world: &mut World, _text: String) {}
    fn send_to_all(_world: &mut World, _text: String) {}
    fn kick(_world: &mut World, _client_id: ClientId) {}
    fn poll_transport(_world: &mut World) {}
}

// Roster broadcasting is now handled in bevy_easy_p2p; transport only emits OnTransportRosterChanged
