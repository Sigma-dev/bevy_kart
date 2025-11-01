use bevy::prelude::*;
use bevy_easy_p2p::{ClientId, EasyP2PSystemSet, P2PTransport};
use bevy_webrtc::{
    ConnectionId, CreateAnswer, CreateOffer, LocalSdpReady, SetRemote, WebRtcPlugin,
};
use serde_json::json;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, RequestMode, Response};

mod systems;

thread_local! {
    pub(crate) static FIRESTORE_INBOX: RefCell<Vec<serde_json::Value>> = RefCell::new(Vec::new());
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

pub struct FirestoreP2PPlugin<PlayerData, PlayerInputData, Instantiations>(
    std::marker::PhantomData<(PlayerData, PlayerInputData, Instantiations)>,
);

impl<PlayerData, PlayerInputData, Instantiations> Default
    for FirestoreP2PPlugin<PlayerData, PlayerInputData, Instantiations>
{
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<PlayerData, PlayerInputData, Instantiations> Plugin
    for FirestoreP2PPlugin<PlayerData, PlayerInputData, Instantiations>
where
    PlayerData: Send + Sync + 'static,
    PlayerInputData: Send + Sync + 'static,
    Instantiations: Send + Sync + 'static,
{
    fn build(&self, app: &mut App) {
        app.init_resource::<ConnectionIdAllocator>()
            .init_resource::<FirestoreConfig>()
            .init_resource::<SignalingState>()
            .init_resource::<FirestoreShared>()
            .add_plugins(WebRtcPlugin)
            .add_systems(
                PreUpdate,
                // Receiving: log incoming data from WebRTC (runs right after pump_js_callbacks to process incoming messages early)
                systems::log_incoming_data::<
                    PlayerData,
                    PlayerInputData,
                    Instantiations,
                >,
            )
            .add_systems(
                Update,
                (
                    // Sending: handle transport send requests (runs after encode_outgoing in Core)
                    systems::handle_send_requests::<
                        PlayerData,
                        PlayerInputData,
                        Instantiations,
                    >,
                    systems::handle_create_join_requests::<
                        PlayerData,
                        PlayerInputData,
                        Instantiations,
                    >,
                    systems::handle_exit_requests::<
                        PlayerData,
                        PlayerInputData,
                        Instantiations,
                    >,
                    systems::handle_kick_requests::<
                        PlayerData,
                        PlayerInputData,
                        Instantiations,
                    >,
                    systems::log_connection_open::<
                        PlayerData,
                        PlayerInputData,
                        Instantiations,
                    >,
                    systems::handle_connection_closed::<
                        PlayerData,
                        PlayerInputData,
                        Instantiations,
                    >,
                    systems::on_lobby_exit_cleanup::<
                        PlayerData,
                        PlayerInputData,
                        Instantiations,
                    >,
                )
                    .in_set(EasyP2PSystemSet::Transport),
            )
            .add_systems(Update, log_local_sdp_ready)
            .add_systems(Update, firestore_pump);
    }
}

pub(crate) fn generate_room_code() -> String {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let code_length = 4;
    let mut out = String::with_capacity(code_length);
    for _ in 0..code_length {
        let r = (js_sys::Math::random() * (ALPHABET.len() as f64)).floor() as usize;
        out.push(ALPHABET.as_bytes()[r] as char);
    }
    out
}

pub(crate) fn gen_client_id_num() -> u64 {
    // 53 bits of randomness via Math.random chunks
    let a = (js_sys::Math::random() * (1u64 << 26) as f64).floor() as u64;
    let b = (js_sys::Math::random() * (1u64 << 27) as f64).floor() as u64;
    (a << 27) | b
}

pub(crate) fn firestore_room_doc_url(cfg: &FirestoreConfig, room: &str) -> String {
    format!(
        "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents/rooms/{}",
        cfg.project_id, room
    )
}

pub(crate) fn firestore_patch_url(cfg: &FirestoreConfig, room: &str, mask: &str) -> String {
    format!(
        "{}?updateMask.fieldPaths={}",
        firestore_room_doc_url(cfg, room),
        mask
    )
}

pub(crate) async fn http_fetch_json(
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

pub(crate) async fn ensure_room_exists(cfg: &FirestoreConfig, room: &str) {
    let url = firestore_room_doc_url(cfg, room);
    let body = json!({
        "fields": {
            "offers": {"mapValue": {"fields": {}}},
            "answers": {"mapValue": {"fields": {}}}
        }
    });
    let _ = http_fetch_json("PATCH", &url, Some(body)).await;
}

pub(crate) async fn write_offer(cfg: &FirestoreConfig, room: &str, client_id: &str, sdp: &str) {
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

pub(crate) async fn write_answer(cfg: &FirestoreConfig, room: &str, client_id: &str, sdp: &str) {
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

pub(crate) async fn read_room(cfg: &FirestoreConfig, room: &str) -> Option<serde_json::Value> {
    http_fetch_json("GET", &firestore_room_doc_url(cfg, room), None).await
}

pub(crate) fn now_ms() -> f64 {
    js_sys::Date::now()
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
