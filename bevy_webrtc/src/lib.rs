//! Minimal browser WebRTC (WASM) using Bevy events and web-sys.

use bevy::prelude::*;
use js_sys::Uint8Array;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::spawn_local;
use web_sys::RtcDataChannel;
use web_sys::RtcDataChannelEvent;
use web_sys::RtcDataChannelState;
use web_sys::RtcIceGatheringState;
use web_sys::RtcIceServer;
use web_sys::RtcPeerConnection;
use web_sys::RtcSdpType;
use web_sys::RtcSessionDescriptionInit;

// Messages exposed to Bevy application code (now include ConnectionId)
#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub struct ConnectionId(pub u64);

#[derive(Message)]
pub struct CreateOffer {
    pub id: ConnectionId,
}

#[derive(Message)]
pub struct CreateAnswer {
    pub id: ConnectionId,
    pub remote_sdp: String,
}

#[derive(Message)]
pub struct SetRemote {
    pub id: ConnectionId, // Answer SDP for offerer side
    pub sdp: String,
}

#[derive(Message)]
pub struct SendData {
    pub id: ConnectionId,
    pub text: String,
}

#[derive(Message)]
pub struct LocalSdpReady {
    pub id: ConnectionId,
    pub sdp: String,
}

#[derive(Message)]
pub struct IncomingData {
    pub id: ConnectionId,
    pub text: String,
}

#[derive(Message)]
pub struct ConnectionOpen(pub ConnectionId);

#[derive(Message)]
pub struct CloseConnection {
    pub id: ConnectionId,
}

#[derive(Message)]
pub struct CloseAllConnections;

#[derive(Message)]
pub struct ConnectionClosed(pub ConnectionId);

// Per-connection state, stored inside the NonSend resource map
struct ConnState {
    pc_slot: Rc<RefCell<Option<RtcPeerConnection>>>,
    dc_slot: Rc<RefCell<Option<RtcDataChannel>>>,
    // Buffers to bridge JS callbacks back into Bevy's world
    pending_open: Rc<Cell<bool>>,
    pending_closed: Rc<Cell<bool>>,
    pending_messages: Rc<RefCell<Vec<String>>>,
    pending_local_sdp: Rc<RefCell<Vec<String>>>,
}

impl ConnState {
    fn new() -> Self {
        Self {
            pc_slot: Rc::new(RefCell::new(None)),
            dc_slot: Rc::new(RefCell::new(None)),
            pending_open: Rc::new(Cell::new(false)),
            pending_closed: Rc::new(Cell::new(false)),
            pending_messages: Rc::new(RefCell::new(Vec::new())),
            pending_local_sdp: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

// NonSend resource (do not derive Resource; inserted as NonSend)
struct RtcContext {
    conns: HashMap<ConnectionId, ConnState>,
}

impl Default for RtcContext {
    fn default() -> Self {
        Self {
            conns: HashMap::new(),
        }
    }
}

pub struct WebRtcPlugin;

impl Plugin for WebRtcPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send_resource(RtcContext::default())
            .add_message::<CreateOffer>()
            .add_message::<CreateAnswer>()
            .add_message::<SetRemote>()
            .add_message::<SendData>()
            .add_message::<LocalSdpReady>()
            .add_message::<IncomingData>()
            .add_message::<ConnectionOpen>()
            .add_message::<CloseConnection>()
            .add_message::<CloseAllConnections>()
            .add_message::<ConnectionClosed>()
            .add_systems(
                Update,
                (
                    handle_create_offer,
                    handle_create_answer,
                    handle_set_remote,
                    handle_send_data,
                    pump_js_callbacks,
                    handle_close_connection,
                    handle_close_all,
                ),
            );
    }
}

// Build an RTCPeerConnection with Google STUN
fn make_peer_connection() -> Result<RtcPeerConnection, JsValue> {
    let ice = RtcIceServer::new();
    ice.set_urls(&JsValue::from_str("stun:stun.l.google.com:19302"));
    let cfg = web_sys::RtcConfiguration::new();
    let servers = js_sys::Array::new();
    servers.push(&ice.into());
    cfg.set_ice_servers(&servers);
    RtcPeerConnection::new_with_configuration(&cfg)
}

// Await until ICE gathering state becomes Complete (non-trickle)
async fn await_ice_complete(pc: RtcPeerConnection) {
    if pc.ice_gathering_state() == RtcIceGatheringState::Complete {
        return;
    }
    // One-shot using JS callback and a Promise-like resolver
    let (tx, rx) = futures_channel::oneshot::channel::<()>();
    let tx = Rc::new(Cell::new(Some(tx)));
    let pc_clone = pc.clone();
    let closure = Closure::wrap(Box::new(move || {
        if pc_clone.ice_gathering_state() == RtcIceGatheringState::Complete {
            if let Some(sender) = tx.take() {
                let _ = sender.send(());
            }
        }
    }) as Box<dyn FnMut()>);
    pc.set_onicegatheringstatechange(Some(closure.as_ref().unchecked_ref()));
    let _ = rx.await;
    pc.set_onicegatheringstatechange(None);
    closure.forget();
}

// Ensure local tab/window closing proactively closes PC/DC so remote detects it immediately
fn register_unload_close(pc: RtcPeerConnection, dc: Option<RtcDataChannel>) {
    if let Some(window) = web_sys::window() {
        // pagehide
        let pc_ph = pc.clone();
        let dc_ph = dc.clone();
        let on_pagehide = Closure::wrap(Box::new(move |_ev: web_sys::Event| {
            if let Some(ref ch) = dc_ph {
                let _ = ch.close();
            }
            pc_ph.close();
        }) as Box<dyn FnMut(web_sys::Event)>);
        let _ = window
            .add_event_listener_with_callback("pagehide", on_pagehide.as_ref().unchecked_ref());
        on_pagehide.forget();

        // beforeunload
        let pc_bu = pc.clone();
        let dc_bu = dc.clone();
        let on_beforeunload = Closure::wrap(Box::new(move |_ev: web_sys::Event| {
            if let Some(ref ch) = dc_bu {
                let _ = ch.close();
            }
            pc_bu.close();
        }) as Box<dyn FnMut(web_sys::Event)>);
        let _ = window.add_event_listener_with_callback(
            "beforeunload",
            on_beforeunload.as_ref().unchecked_ref(),
        );
        on_beforeunload.forget();
    }
}

// Hook RTCPeerConnection state changes to detect disconnects in addition to DataChannel onclose
fn hook_peer_connection(pending_closed: Rc<Cell<bool>>, pc: &RtcPeerConnection) {
    // Fallback to string-based state reads for broader web-sys compatibility
    let flag_conn = pending_closed.clone();
    let pc_conn = pc.clone();
    let conn_state_closure = Closure::wrap(Box::new(move || {
        let target: JsValue = JsValue::from(pc_conn.clone());
        if let Ok(state_val) = js_sys::Reflect::get(&target, &JsValue::from_str("connectionState"))
        {
            if let Some(state) = state_val.as_string() {
                info!("pc.connectionState={}", state);
                if state == "disconnected" || state == "failed" || state == "closed" {
                    info!("pc.connectionState indicates closed/disconnected/failed -> mark closed");
                    flag_conn.set(true);
                }
            }
        }
    }) as Box<dyn FnMut()>);
    pc.set_onconnectionstatechange(Some(conn_state_closure.as_ref().unchecked_ref()));
    conn_state_closure.forget();

    let flag_ice = pending_closed.clone();
    let pc_ice = pc.clone();
    let ice_state_closure = Closure::wrap(Box::new(move || {
        let target: JsValue = JsValue::from(pc_ice.clone());
        if let Ok(state_val) =
            js_sys::Reflect::get(&target, &JsValue::from_str("iceConnectionState"))
        {
            if let Some(state) = state_val.as_string() {
                if state == "disconnected" || state == "failed" || state == "closed" {
                    flag_ice.set(true);
                }
            }
        }
    }) as Box<dyn FnMut()>);
    pc.set_oniceconnectionstatechange(Some(ice_state_closure.as_ref().unchecked_ref()));
    ice_state_closure.forget();
}

// Setup data channel callbacks and store channel
fn hook_data_channel(
    pending_open: Rc<Cell<bool>>,
    pending_closed: Rc<Cell<bool>>,
    pending_messages: Rc<RefCell<Vec<String>>>,
    dc: &RtcDataChannel,
) {
    let on_open_flag = pending_open.clone();
    let on_close_flag = pending_closed.clone();
    let on_msg_buf = pending_messages.clone();

    // onopen -> mark flag
    let open_closure = Closure::wrap(Box::new(move || {
        on_open_flag.set(true);
    }) as Box<dyn FnMut()>);
    dc.set_onopen(Some(open_closure.as_ref().unchecked_ref()));
    open_closure.forget();

    // onclose -> mark closed flag
    let close_closure = Closure::wrap(Box::new(move || {
        on_close_flag.set(true);
    }) as Box<dyn FnMut()>);
    dc.set_onclose(Some(close_closure.as_ref().unchecked_ref()));
    close_closure.forget();

    // onmessage -> push string messages
    let msg_closure = Closure::wrap(Box::new(move |ev: web_sys::MessageEvent| {
        let data = ev.data();
        // Prefer string; if ArrayBuffer or Blob, try to convert to string for minimal impl
        if let Some(s) = data.as_string() {
            on_msg_buf.borrow_mut().push(s);
        } else if let Ok(ab) = data.clone().dyn_into::<js_sys::ArrayBuffer>() {
            let u8 = Uint8Array::new(&ab);
            // Attempt UTF-8 decode
            if let Ok(text) = std::str::from_utf8(&u8.to_vec()) {
                on_msg_buf.borrow_mut().push(text.to_string());
            }
        } else if let Ok(js_str) = data.clone().dyn_into::<js_sys::JsString>() {
            on_msg_buf.borrow_mut().push(String::from(js_str));
        }
    }) as Box<dyn FnMut(web_sys::MessageEvent)>);
    dc.set_onmessage(Some(msg_closure.as_ref().unchecked_ref()));
    msg_closure.forget();
}

fn handle_create_offer(mut ctx: NonSendMut<RtcContext>, mut ev: MessageReader<CreateOffer>) {
    for CreateOffer { id } in ev.read() {
        // Fresh per-connection state
        let state = ctx
            .conns
            .entry(ConnectionId(id.0))
            .or_insert_with(ConnState::new);

        // Fresh peer connection
        let pc = match make_peer_connection() {
            Ok(pc) => pc,
            Err(err) => {
                info!("Failed to create RTCPeerConnection: {:?}", err);
                continue;
            }
        };

        // Detect disconnects via peer connection state changes
        hook_peer_connection(state.pending_closed.clone(), &pc);

        // Create data channel immediately (offerer)
        let dc = pc.create_data_channel("data");
        hook_data_channel(
            state.pending_open.clone(),
            state.pending_closed.clone(),
            state.pending_messages.clone(),
            &dc,
        );

        // Proactively close on page unload (offerer side has DC now)
        register_unload_close(pc.clone(), Some(dc.clone()));

        // Prepare to emit local SDP after ICE completes
        let sdp_buf = state.pending_local_sdp.clone();
        let pc_clone = pc.clone();
        let this_id = id;

        spawn_local(async move {
            // Create offer
            let offer_promise = pc_clone.create_offer();
            let offer_val = match wasm_bindgen_futures::JsFuture::from(offer_promise).await {
                Ok(v) => v,
                Err(e) => {
                    info!("createOffer failed: {:?}", e);
                    return;
                }
            };
            let offer: RtcSessionDescriptionInit = offer_val.unchecked_into();

            // Set local description
            if wasm_bindgen_futures::JsFuture::from(pc_clone.set_local_description(&offer))
                .await
                .is_err()
            {
                return;
            }
            await_ice_complete(pc_clone.clone()).await;
            if let Some(local) = pc_clone.local_description() {
                sdp_buf.borrow_mut().push(local.sdp());
            }
            // sdp will be pumped with id later
            let _ = this_id; // silence unused warning in some builds
        });

        state.dc_slot.borrow_mut().replace(dc);
        state.pc_slot.borrow_mut().replace(pc);
    }
    ev.clear();
}

fn handle_create_answer(mut ctx: NonSendMut<RtcContext>, mut ev: MessageReader<CreateAnswer>) {
    for CreateAnswer { id, remote_sdp } in ev.read() {
        // Fresh per-connection state
        let state = ctx
            .conns
            .entry(ConnectionId(id.0))
            .or_insert_with(ConnState::new);

        let pc = match make_peer_connection() {
            Ok(pc) => pc,
            Err(err) => {
                info!("Failed to create RTCPeerConnection: {:?}", err);
                continue;
            }
        };

        // Listen for datachannel from offerer
        // Also detect disconnects via peer connection state changes
        hook_peer_connection(state.pending_closed.clone(), &pc);
        let pc_for_dc = pc.clone();
        // Proactively close on page unload (answerer may not have DC yet)
        register_unload_close(pc.clone(), None);
        let on_dc_ctx_open = state.pending_open.clone();
        let on_dc_ctx_closed = state.pending_closed.clone();
        let on_dc_ctx_msgs = state.pending_messages.clone();
        let dc_slot = state.dc_slot.clone();
        let on_dc = Closure::wrap(Box::new(move |ev: RtcDataChannelEvent| {
            let channel = ev.channel();
            hook_data_channel(
                on_dc_ctx_open.clone(),
                on_dc_ctx_closed.clone(),
                on_dc_ctx_msgs.clone(),
                &channel,
            );
            dc_slot.borrow_mut().replace(channel);
            // Register unload close with concrete channel on answerer side too
            if let Some(pc_ref) = Some(pc_for_dc.clone()) {
                register_unload_close(pc_ref, dc_slot.borrow().as_ref().cloned());
            }
        }) as Box<dyn FnMut(RtcDataChannelEvent)>);
        pc.set_ondatachannel(Some(on_dc.as_ref().unchecked_ref()));
        on_dc.forget();

        let sdp_text = remote_sdp.clone();
        let sdp_buf = state.pending_local_sdp.clone();
        let pc_clone = pc.clone();
        let _this_id = id;
        spawn_local(async move {
            // Apply remote offer
            let remote = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
            remote.set_sdp(&sdp_text);
            if wasm_bindgen_futures::JsFuture::from(pc_clone.set_remote_description(&remote))
                .await
                .is_err()
            {
                return;
            }
            // Create and set local answer
            let answer_val =
                match wasm_bindgen_futures::JsFuture::from(pc_clone.create_answer()).await {
                    Ok(v) => v,
                    Err(_) => return,
                };
            let answer: RtcSessionDescriptionInit = answer_val.unchecked_into();
            if wasm_bindgen_futures::JsFuture::from(pc_clone.set_local_description(&answer))
                .await
                .is_err()
            {
                return;
            }
            await_ice_complete(pc_clone.clone()).await;
            if let Some(local) = pc_clone.local_description() {
                sdp_buf.borrow_mut().push(local.sdp());
            }
        });

        state.pc_slot.borrow_mut().replace(pc);
    }
    ev.clear();
}

fn handle_set_remote(ctx: NonSend<RtcContext>, mut ev: MessageReader<SetRemote>) {
    for SetRemote { id, sdp } in ev.read() {
        if let Some(state) = ctx.conns.get(&id) {
            if let Some(pc) = state.pc_slot.borrow().clone() {
                let pc_clone = pc.clone();
                let sdp_text = sdp.clone();
                spawn_local(async move {
                    let desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
                    desc.set_sdp(&sdp_text);
                    let _ = wasm_bindgen_futures::JsFuture::from(
                        pc_clone.set_remote_description(&desc),
                    )
                    .await;
                });
            }
        }
    }
    ev.clear();
}

fn handle_send_data(ctx: NonSend<RtcContext>, mut ev: MessageReader<SendData>) {
    for SendData { id, text } in ev.read() {
        if let Some(state) = ctx.conns.get(&id) {
            if let Some(dc) = state.dc_slot.borrow().as_ref() {
                if dc.ready_state() == RtcDataChannelState::Open {
                    let _ = dc.send_with_str(text.as_str());
                }
            }
        }
    }
    ev.clear();
}

// Periodically pump events produced by JS callbacks and async tasks back into Bevy
fn pump_js_callbacks(
    ctx: NonSendMut<RtcContext>,
    mut sdp_writer: MessageWriter<LocalSdpReady>,
    mut msg_writer: MessageWriter<IncomingData>,
    mut open_writer: MessageWriter<ConnectionOpen>,
    mut closed_writer: MessageWriter<ConnectionClosed>,
) {
    // Iterate over connections and flush their pending buffers with ids
    let ids: Vec<ConnectionId> = ctx.conns.keys().cloned().collect();
    for id in ids {
        if let Some(state) = ctx.conns.get(&id) {
            if state.pending_open.replace(false) {
                open_writer.write(ConnectionOpen(id));
            }
            if state.pending_closed.replace(false) {
                closed_writer.write(ConnectionClosed(id));
            }
            let mut msgs = state.pending_messages.borrow_mut();
            for s in msgs.drain(..) {
                msg_writer.write(IncomingData { id, text: s });
            }
            drop(msgs);
            let mut sdp = state.pending_local_sdp.borrow_mut();
            for s in sdp.drain(..) {
                sdp_writer.write(LocalSdpReady { id, sdp: s });
            }
        }
    }
}

fn handle_close_connection(
    mut ctx: NonSendMut<RtcContext>,
    mut ev: MessageReader<CloseConnection>,
) {
    for CloseConnection { id } in ev.read() {
        if let Some(state) = ctx.conns.remove(&id) {
            if let Some(dc) = state.dc_slot.borrow().as_ref() {
                let _ = dc.close();
            }
            if let Some(pc) = state.pc_slot.borrow().as_ref() {
                pc.close();
            }
        }
    }
    ev.clear();
}

fn handle_close_all(mut ctx: NonSendMut<RtcContext>, mut ev: MessageReader<CloseAllConnections>) {
    let mut any = false;
    for _ in ev.read() {
        any = true;
    }
    if !any {
        return;
    }
    let ids: Vec<ConnectionId> = ctx.conns.keys().cloned().collect();
    for id in ids {
        if let Some(state) = ctx.conns.remove(&id) {
            if let Some(dc) = state.dc_slot.borrow().as_ref() {
                let _ = dc.close();
            }
            if let Some(pc) = state.pc_slot.borrow().as_ref() {
                pc.close();
            }
        }
    }
    ev.clear();
}
