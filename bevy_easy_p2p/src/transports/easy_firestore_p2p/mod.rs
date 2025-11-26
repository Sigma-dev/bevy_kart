use crate::schedules::{TransportHydrate, TransportProcess};
use bevy::prelude::*;
use bevy_webrtc::{ConnectionId, WebRtc, WebRtcPlugin};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, RequestMode, Response};

mod structs;
mod systems;
use structs::*;

thread_local! {
    pub(crate) static FIRESTORE_INBOX: RefCell<Vec<FirestoreInboxMessage>> = RefCell::new(Vec::new());
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

pub struct FirestoreP2PPlugin<Packet>(std::marker::PhantomData<Packet>);

impl<Packet> Default for FirestoreP2PPlugin<Packet> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<Packet> Plugin for FirestoreP2PPlugin<Packet>
where
    Packet: Send + Sync + 'static + Serialize + DeserializeOwned,
{
    fn build(&self, app: &mut App) {
        app.init_resource::<FirestoreConfig>()
            .init_resource::<SignalingState>()
            .init_resource::<FirestoreShared>()
            .add_plugins(WebRtcPlugin)
            .add_systems(TransportHydrate, systems::handle_webrtc_events::<Packet>)
            .add_systems(
                TransportProcess,
                systems::handle_transport_requests::<Packet>,
            )
            .add_systems(TransportHydrate, firestore_pump);
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

pub(crate) async fn http_fetch_json<B: Serialize, R: DeserializeOwned>(
    method: &str,
    url: &str,
    body: Option<&B>,
) -> Option<R> {
    let window = web_sys::window()?;
    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(RequestMode::Cors);
    if let Some(b) = body {
        let headers = Headers::new().ok()?;
        headers.set("Content-Type", "application/json").ok()?;
        init.set_headers(&headers);
        let body_str = serde_json::to_string(b).ok()?;
        init.set_body(&JsValue::from_str(&body_str));
    }
    let request = Request::new_with_str_and_init(url, &init).ok()?;
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .ok()?;
    let resp: Response = resp_value.dyn_into().ok()?;
    if !resp.ok() {
        warn!("Fetch failed: {} {}", resp.status(), resp.status_text());
        return None;
    }
    let json = JsFuture::from(resp.json().ok()?).await.ok()?;
    match serde_wasm_bindgen::from_value(json) {
        Ok(val) => Some(val),
        Err(e) => {
            warn!("Deserialization failed: {:?}", e);
            None
        }
    }
}

pub(crate) async fn ensure_room_exists(cfg: &FirestoreConfig, room: &str) {
    let url = firestore_room_doc_url(cfg, room);
    let body = FirestoreRoomDoc {
        fields: FirestoreRoomFields::default(),
    };
    // We just use generic Value for the response here since we don't care about it
    let _ = http_fetch_json::<_, serde_json::Value>("PATCH", &url, Some(&body)).await;
}

pub(crate) async fn write_offer(cfg: &FirestoreConfig, room: &str, client_id: &str, sdp: &str) {
    let url = firestore_patch_url(cfg, room, "offers");
    let mut map = HashMap::new();
    map.insert(
        client_id.to_string(),
        FirestoreStringValue {
            string_value: sdp.to_string(),
        },
    );

    let body = FirestorePatchOffers {
        fields: FirestoreOffersField {
            offers: FirestoreMapValue {
                map_value: FirestoreMapFields { fields: map },
            },
        },
    };
    let _ = http_fetch_json::<_, serde_json::Value>("PATCH", &url, Some(&body)).await;
}

pub(crate) async fn write_answer(cfg: &FirestoreConfig, room: &str, client_id: &str, sdp: &str) {
    let url = firestore_patch_url(cfg, room, "answers");
    let mut map = HashMap::new();
    map.insert(
        client_id.to_string(),
        FirestoreStringValue {
            string_value: sdp.to_string(),
        },
    );

    let body = FirestorePatchAnswers {
        fields: FirestoreAnswersField {
            answers: FirestoreMapValue {
                map_value: FirestoreMapFields { fields: map },
            },
        },
    };
    let _ = http_fetch_json::<_, serde_json::Value>("PATCH", &url, Some(&body)).await;
}

pub(crate) async fn read_room(cfg: &FirestoreConfig, room: &str) -> Option<FirestoreInboxMessage> {
    match http_fetch_json::<(), FirestoreRoomDoc>("GET", &firestore_room_doc_url(cfg, room), None)
        .await
    {
        Some(doc) => Some(FirestoreInboxMessage::Doc(doc)),
        None => Some(FirestoreInboxMessage::RoomNotFound),
    }
}

pub(crate) fn now_ms() -> f64 {
    js_sys::Date::now()
}

fn firestore_pump(
    mut sig: ResMut<SignalingState>,
    cfg: Res<FirestoreConfig>,
    mut webrtc: WebRtc,
    mut shared: ResMut<FirestoreShared>,
) {
    if sig.room_code.is_empty() {
        return;
    }

    let mut drained_msgs: Vec<FirestoreInboxMessage> = Vec::new();
    FIRESTORE_INBOX.with(|inbox| {
        let mut buf = inbox.borrow_mut();
        drained_msgs.extend(buf.drain(..));
    });
    if !drained_msgs.is_empty() {
        shared.in_flight = false;
    }
    for msg in drained_msgs {
        match msg {
            FirestoreInboxMessage::RoomCreated => {
                shared.room_exists = true;
            }
            FirestoreInboxMessage::RoomNotFound => {
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
            }
            FirestoreInboxMessage::Doc(doc) => {
                apply_firestore_doc(&doc, &mut sig, &mut webrtc);
                // If we are a client waiting to join and the room exists, create offer only.
                // Delay emitting OnLobbyJoined/OnLobbyEntered until data channel opens.
                if sig.client_join_pending && !sig.is_host {
                    sig.client_join_pending = false;
                    sig.client_emitted_join = false;
                    let id = webrtc.create_offer();
                    sig.offer_conn = Some(id);
                }
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
        if let Some(msg) = read_room(&cfg_owned, &room).await {
            FIRESTORE_INBOX.with(|inbox| inbox.borrow_mut().push(msg));
        } else {
            // network error or something, maybe treat as not found or ignore
            // currently ignoring
            FIRESTORE_INBOX
                .with(|inbox| inbox.borrow_mut().push(FirestoreInboxMessage::RoomNotFound));
        }
    });
}

fn apply_firestore_doc(doc: &FirestoreRoomDoc, sig: &mut SignalingState, webrtc: &mut WebRtc) {
    if sig.is_host {
        for (cid, val) in doc.fields.offers.map_value.fields.iter() {
            if sig.answered_clients.contains(cid) {
                continue;
            }
            let sdp = &val.string_value;
            let id = webrtc.create_answer(sdp.to_string());
            sig.answered_clients.insert(cid.clone());
            sig.host_connection_to_client_id.insert(id.0, cid.clone());
        }
    } else if let Some(cid) = sig.client_id.clone() {
        if !sig.client_answer_applied {
            if let Some(val) = doc.fields.answers.map_value.fields.get(&cid) {
                let sdp = &val.string_value;
                if let Some(target) = sig.offer_conn {
                    webrtc.accept_answer(target, sdp.to_string());
                    sig.client_answer_applied = true;
                } else {
                    warn!("Received Firestore answer but no pending offer connection exists");
                }
            }
        }
    }
}
