use bevy::prelude::*;
use bevy_webrtc::{
    ConnectionId, ConnectionOpen, CreateAnswer, CreateOffer, IncomingData, LocalSdpReady, SendData,
    SetRemote, WebRtcPlugin,
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

// ===== Signaling helpers and resources =====

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

// Random room code: six uppercase letters (e.g., AMONGU)
fn generate_room_code() -> String {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut out = String::with_capacity(6);
    for _ in 0..6 {
        let r = (js_sys::Math::random() * (ALPHABET.len() as f64)).floor() as usize;
        out.push(ALPHABET.as_bytes()[r] as char);
    }
    out
}

fn gen_client_id() -> String {
    const ALPHABET: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let len = 12;
    // Try to use window.crypto for better randomness
    if let Some(window) = web_sys::window() {
        if let Some(crypto) = window.crypto().ok() {
            let mut bytes = vec![0u8; len];
            if crypto.get_random_values_with_u8_array(&mut bytes).is_ok() {
                let mut out = String::with_capacity(len);
                for b in bytes {
                    // Map byte to index 0..ALPHABET.len()
                    let idx = (b as usize) % ALPHABET.len();
                    out.push(ALPHABET.as_bytes()[idx] as char);
                }
                return out;
            }
        }
    }
    // Fallback to Math.random
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        let r = (js_sys::Math::random() * (ALPHABET.len() as f64)).floor() as usize;
        out.push(ALPHABET.as_bytes()[r] as char);
    }
    out
}

#[derive(Resource, Clone)]
struct FirestoreConfig {
    project_id: String,
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
    client_id: Option<String>,
    client_answer_applied: bool,
    offer_conn: Option<ConnectionId>,
    host_connection_to_client_id: HashMap<u64, String>,
}

#[derive(Resource, Default)]
struct FirestoreShared {
    in_flight: bool,
    next_allowed_fetch_at_ms: f64,
    not_found_logged: bool,
    room_exists: bool,
}

pub struct FirestoreSignalingPlugin;

impl Plugin for FirestoreSignalingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConnectionIdAllocator>()
            .init_resource::<FirestoreConfig>()
            .init_resource::<SignalingState>()
            .init_resource::<FirestoreShared>()
            .add_systems(Startup, auto_join_from_room_url)
            .add_systems(
                Update,
                (
                    keyboard_shortcuts,
                    log_local_sdp_ready,
                    log_connection_open,
                    log_incoming_data,
                    firestore_pump,
                ),
            );
    }
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

// Same as read_room but distinguishes 404 as a non-error so callers can handle gracefully
async fn read_room_allow_404(
    cfg: &FirestoreConfig,
    room: &str,
) -> Result<Option<serde_json::Value>, u16> {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return Err(0),
    };
    let url = firestore_room_doc_url(cfg, room);
    let init = RequestInit::new();
    init.set_method("GET");
    init.set_mode(RequestMode::Cors);
    let request = match Request::new_with_str_and_init(&url, &init) {
        Ok(r) => r,
        Err(_) => return Err(0),
    };
    let resp_value = match JsFuture::from(window.fetch_with_request(&request)).await {
        Ok(v) => v,
        Err(_) => return Err(0),
    };
    let resp: Response = match resp_value.dyn_into() {
        Ok(r) => r,
        Err(_) => return Err(0),
    };
    let status = resp.status();
    if status == 404 {
        return Ok(None);
    }
    if !resp.ok() {
        return Err(status);
    }
    let json_promise = resp.json().map_err(|_| status)?;
    let js_value = JsFuture::from(json_promise).await.map_err(|_| status)?;
    let val: serde_json::Value = serde_wasm_bindgen::from_value(js_value).map_err(|_| status)?;
    Ok(Some(val))
}

fn prompt(label: &str) -> Option<String> {
    web_sys::window()
        .and_then(|w| w.prompt_with_message(&label).ok())
        .flatten()
}

// Keyboard shortcuts:
// - H: Host a room (auto code)
// - J: Join a room (enter code)
// - T: Send a text message on the data channel (both sides)
fn keyboard_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut w_offer: MessageWriter<CreateOffer>,
    mut w_send: MessageWriter<SendData>,
    mut id_alloc: ResMut<ConnectionIdAllocator>,
    q_conns: Query<&NetConnection>,
    mut sig: ResMut<SignalingState>,
    cfg: Res<FirestoreConfig>,
) {
    if keys.just_pressed(KeyCode::KeyT) {
        let text = prompt("Send text over data channel:").unwrap_or_default();
        if !text.is_empty() {
            // Choose a target connection: if one, use it; otherwise prompt for id or 'all'
            if let Some(single) = only_connection(&q_conns) {
                info!("Sending to {:?}: {}", single, text);
                w_send.write(SendData { id: single, text });
            } else {
                let target = prompt("Target connection id (number) or 'all':").unwrap_or_default();
                if target.trim().eq_ignore_ascii_case("all") {
                    for c in q_conns.iter() {
                        w_send.write(SendData {
                            id: c.id,
                            text: text.clone(),
                        });
                    }
                    info!("Sent to all connections: {}", text);
                } else if let Ok(n) = target.trim().parse::<u64>() {
                    let id = ConnectionId(n);
                    info!("Sending to {:?}: {}", id, text);
                    w_send.write(SendData { id, text });
                } else {
                    info!("Invalid target. Not sending.");
                }
            }
        }
    }
    // Host flow: create room (auto code)
    if keys.just_pressed(KeyCode::KeyH) {
        let room = generate_room_code();
        sig.room_code = room.clone();
        sig.is_host = true;
        sig.answered_clients.clear();
        let cfg = cfg.clone();
        info!("Hosting room: {}", room);
        if let Some(base) = current_base_url() {
            info!("Share link: {}?room={}", base, room);
        }
        // Reset room_exists until confirmed
        // and notify pump post-creation
        // (flag is set when inbox receives __status: created)
        // so we avoid initial 404s
        // Note: keep polling gate in pump
        wasm_bindgen_futures::spawn_local(async move {
            ensure_room_exists(&cfg, &room).await;
            FIRESTORE_INBOX.with(|inbox| {
                inbox
                    .borrow_mut()
                    .push(serde_json::json!({"__status": "created"}))
            });
        });
    }

    // Client flow: join room
    if keys.just_pressed(KeyCode::KeyJ) {
        let room = prompt("Enter room code:").unwrap_or_default();
        if !room.is_empty() {
            // Generate a random client id
            let client_id = gen_client_id();
            sig.room_code = room.clone();
            sig.is_host = false;
            sig.client_id = Some(client_id.clone());
            sig.client_answer_applied = false;
            // Create local offer immediately; we'll publish when LocalSdpReady fires
            let id = id_alloc.allocate();
            sig.offer_conn = Some(id);
            w_offer.write(CreateOffer { id });
            info!("Joining room '{}' as client '{}'", room, client_id);
        }
    }
}

fn only_connection(q: &Query<&NetConnection>) -> Option<ConnectionId> {
    let mut it = q.iter();
    let first = it.next()?;
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
        info!("Local description ready for connection {:?}", id);
        // Ensure an entity exists for this connection id
        commands.spawn(NetConnection { id: *id });
        // Remember the connection id created by our local offer if applicable
        if !sig.is_host && sig.offer_conn.is_none() {
            sig.offer_conn = Some(*id);
        }

        // If client joined a room, publish offer under their id
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

        // Host: when we created a local answer in response to a client's offer, publish it
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

fn log_connection_open(mut r: MessageReader<ConnectionOpen>) {
    for ConnectionOpen(id) in r.read() {
        info!("Connection {:?} data channel open", id);
    }
}

fn log_incoming_data(mut r: MessageReader<IncomingData>) {
    for IncomingData { id, text } in r.read() {
        info!("Message on {:?}: {}", id, text);
    }
}

fn get_url() -> Option<String> {
    let window = web_sys::window()?;

    // Prefer the embedding page's URL when running inside itch.io's iframe domain.
    // On itch, the game runs at html-classic.itch.zone, but the shareable URL
    // should be the project page (the embedding referrer) when available.
    let hostname_is_itch_zone = window
        .location()
        .hostname()
        .ok()
        .map(|h| h.ends_with("itch.zone") || h.ends_with("itch.io"))
        .unwrap_or(false);

    let referrer = window.document().map(|d| d.referrer()).unwrap_or_default();

    if hostname_is_itch_zone && !referrer.is_empty() {
        Some(referrer)
    } else {
        Some(window.location().href().ok()?)
    }
}

// Return current page URL without any query string or hash fragment
fn current_base_url() -> Option<String> {
    let source = get_url()?;
    let no_hash = source.split('#').next().unwrap_or(source.as_str());
    let base = no_hash.split('?').next().unwrap_or(no_hash);
    Some(base.trim_end_matches('/').to_string())
}

// Extract a query parameter from the current URL if present
fn extract_query_param(target: &str) -> Option<String> {
    let href = get_url()?;
    let no_hash = href.split('#').next().unwrap_or(href.as_str());
    let query = no_hash.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        let key = it.next()?;
        if key == target {
            let val = it.next().unwrap_or("");
            return Some(val.to_string());
        }
    }
    None
}

// On page load, if `?code=` is present, treat it as a REMOTE OFFER CODE and create an answer immediately
fn auto_join_from_room_url(
    mut sig: ResMut<SignalingState>,
    mut id_alloc: ResMut<ConnectionIdAllocator>,
    mut w_offer: MessageWriter<CreateOffer>,
) {
    if let Some(room) = extract_query_param("room") {
        if !room.trim().is_empty() {
            sig.room_code = room.clone();
            sig.is_host = false;
            sig.client_id = Some(gen_client_id());
            sig.client_answer_applied = false;
            let id = id_alloc.allocate();
            sig.offer_conn = Some(id);
            w_offer.write(CreateOffer { id });
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
) {
    if sig.room_code.is_empty() {
        return;
    }

    // 1) Drain inbox from prior async fetch
    let mut drained_docs: Vec<serde_json::Value> = Vec::new();
    FIRESTORE_INBOX.with(|inbox| {
        let mut buf = inbox.borrow_mut();
        drained_docs.extend(buf.drain(..));
    });
    if !drained_docs.is_empty() {
        // Mark the previous request (if any) as completed
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
                // Back off a bit more when room is not yet created
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
        }
    }

    // 2) Kick off next async fetch with simple rate limit and in-flight guard
    let now = now_ms();
    if shared.in_flight || now < shared.next_allowed_fetch_at_ms {
        return;
    }
    // Avoid GET 404 spam for hosts until room creation confirmed
    if sig.is_host && !shared.room_exists {
        return;
    }
    shared.in_flight = true;
    // Poll at most twice per second
    shared.next_allowed_fetch_at_ms = now + 500.0;
    let room = sig.room_code.clone();
    let cfg_owned = cfg.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let pushed = match read_room_allow_404(&cfg_owned, &room).await {
            Ok(Some(doc)) => doc,
            Ok(None) => serde_json::json!({"__status": "not_found"}),
            Err(_) => serde_json::Value::Null,
        };
        FIRESTORE_INBOX.with(|inbox| inbox.borrow_mut().push(pushed));
    });
}

fn now_ms() -> f64 {
    // js_sys::Date::now is widely available and sufficient for coarse rate limiting
    js_sys::Date::now()
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
                            // Remember which client id this host-side answer connection is for
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
                            // Apply the answer to our original offer connection if tracked,
                            // otherwise create a new one as fallback
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

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((WebRtcPlugin, FirestoreSignalingPlugin))
        .run();
}
