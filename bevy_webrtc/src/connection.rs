use bevy::prelude::*;
use js_sys::Uint8Array;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::spawn_local;
use web_sys::RtcPeerConnection;
use web_sys::{RtcDataChannel, RtcSessionDescriptionInit};
use web_sys::{RtcDataChannelEvent, RtcIceGatheringState};
use web_sys::{RtcIceServer, RtcSdpType};

use crate::{Data, Sdp};

const STUN_SERVER_URL: &str = "stun:stun.l.google.com:19302";

// Per-connection state, stored inside the NonSend resource map
#[derive(Debug, Clone)]
pub(crate) struct RtcConnection {
    // Peer connection slot
    pub(crate) pc_slot: Rc<RefCell<Option<RtcPeerConnection>>>,
    // Data channel slot
    pub(crate) dc_reliable_slot: Rc<RefCell<Option<RtcDataChannel>>>,
    pub(crate) dc_unreliable_slot: Rc<RefCell<Option<RtcDataChannel>>>,
    // Buffers to bridge JS callbacks back into Bevy's world
    pub(crate) pending_open: Rc<Cell<bool>>,
    pub(crate) pending_closed: Rc<Cell<bool>>,
    pub(crate) pending_messages: Rc<RefCell<Vec<Vec<u8>>>>,
    pub(crate) pending_local_sdp: Rc<RefCell<Vec<String>>>,
}

impl RtcConnection {
    pub(crate) fn new() -> Self {
        Self {
            pc_slot: Rc::new(RefCell::new(None)),
            dc_reliable_slot: Rc::new(RefCell::new(None)),
            dc_unreliable_slot: Rc::new(RefCell::new(None)),
            pending_open: Rc::new(Cell::new(false)),
            pending_closed: Rc::new(Cell::new(false)),
            pending_messages: Rc::new(RefCell::new(Vec::new())),
            pending_local_sdp: Rc::new(RefCell::new(Vec::new())),
        }
    }
    pub(crate) fn new_offer() -> Result<Self, String> {
        let connection = Self::new();
        // Fresh peer connection
        let pc = make_and_hook_peer_connection(&connection)?;

        // Create reliable data channel (ordered, reliable)
        let dc_rel = pc.create_data_channel("reliable");
        hook_data_channel(
            connection.pending_open.clone(),
            connection.pending_closed.clone(),
            connection.pending_messages.clone(),
            &dc_rel,
        );

        // Create unreliable data channel (unordered, unreliable)
        let init = web_sys::RtcDataChannelInit::new();
        init.set_ordered(false);
        init.set_max_retransmits(0);
        let dc_unrel = pc.create_data_channel_with_data_channel_dict("unreliable", &init);
        hook_data_channel(
            connection.pending_open.clone(),
            connection.pending_closed.clone(),
            connection.pending_messages.clone(),
            &dc_unrel,
        );

        // Proactively close on page unload (offerer side has DC now)
        register_unload_close(pc.clone());

        // Prepare to emit local SDP after ICE completes
        let sdp_buf = connection.pending_local_sdp.clone();
        let pc_clone = pc.clone();

        spawn_local(async move {
            // Create offer
            let offer_promise = pc_clone.create_offer();
            let Ok(offer_val) = wasm_bindgen_futures::JsFuture::from(offer_promise).await else {
                error!("createOffer failed");
                return;
            };
            if let Err(err) =
                update_description(&sdp_buf, &pc_clone, offer_val.unchecked_into()).await
            {
                error!("Failed to update description: {:?}", err);
                return;
            }
        });

        connection.dc_reliable_slot.borrow_mut().replace(dc_rel);
        connection.dc_unreliable_slot.borrow_mut().replace(dc_unrel);
        connection.pc_slot.borrow_mut().replace(pc);
        Ok(connection)
    }
    pub(crate) fn new_answer(remote_sdp: &Sdp) -> Result<Self, String> {
        let connection = Self::new();

        let pc = make_and_hook_peer_connection(&connection)?;
        // Proactively close on page unload (answerer may not have DC yet)
        register_unload_close(pc.clone());
        let on_dc_ctx_open = connection.pending_open.clone();
        let on_dc_ctx_closed = connection.pending_closed.clone();
        let on_dc_ctx_msgs = connection.pending_messages.clone();
        let dc_reliable_slot = connection.dc_reliable_slot.clone();
        let dc_unreliable_slot = connection.dc_unreliable_slot.clone();

        let on_dc = Closure::wrap(Box::new(move |ev: RtcDataChannelEvent| {
            let channel = ev.channel();
            let label = channel.label();
            hook_data_channel(
                on_dc_ctx_open.clone(),
                on_dc_ctx_closed.clone(),
                on_dc_ctx_msgs.clone(),
                &channel,
            );
            if label == "unreliable" {
                dc_unreliable_slot.borrow_mut().replace(channel);
            } else {
                // Default to reliable if label matches "reliable" or others
                dc_reliable_slot.borrow_mut().replace(channel);
            }
        }) as Box<dyn FnMut(RtcDataChannelEvent)>);
        pc.set_ondatachannel(Some(on_dc.as_ref().unchecked_ref()));
        on_dc.forget();

        let sdp_text = remote_sdp.clone();
        let sdp_buf = connection.pending_local_sdp.clone();
        let pc_clone = pc.clone();
        spawn_local(async move {
            // Apply remote offer
            let remote = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
            remote.set_sdp(&sdp_text);
            if let Err(err) =
                wasm_bindgen_futures::JsFuture::from(pc_clone.set_remote_description(&remote)).await
            {
                error!("set_remote_description failed: {:?}", err);
                return;
            }
            let Ok(answer_val) =
                wasm_bindgen_futures::JsFuture::from(pc_clone.create_answer()).await
            else {
                error!("create_answer failed");
                return;
            };

            if let Err(err) =
                update_description(&sdp_buf, &pc_clone, answer_val.unchecked_into()).await
            {
                error!("Failed to update description: {:?}", err);
                return;
            }
        });

        connection.pc_slot.borrow_mut().replace(pc);
        Ok(connection)
    }
    pub(crate) fn accept_answer(&self, sdp: &Sdp) -> Result<(), String> {
        let pc = self
            .get_peer_connection()
            .ok_or_else(|| format!("Peer connection not found"))?;
        let sdp_text = sdp.clone();
        spawn_local(async move {
            let desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
            desc.set_sdp(&sdp_text);
            let _ = wasm_bindgen_futures::JsFuture::from(pc.set_remote_description(&desc)).await;
        });
        Ok(())
    }
    pub(crate) fn get_peer_connection(&self) -> Option<RtcPeerConnection> {
        self.pc_slot.borrow().clone()
    }
    pub(crate) fn get_data_channel(&self, reliable: bool) -> Option<RtcDataChannel> {
        if reliable {
            self.dc_reliable_slot.borrow().clone()
        } else {
            self.dc_unreliable_slot.borrow().clone()
        }
    }
    pub(crate) fn send_data(&self, data: &Data, reliable: bool) -> Result<(), String> {
        self.get_data_channel(reliable)
            .ok_or_else(|| format!("Data channel not found (reliable={})", reliable))?
            .send_with_u8_array(data)
            .map_err(|err| format!("Failed to send data: {:?}", err))
    }
    pub(crate) fn close(&self) -> Result<(), String> {
        if let Some(dc) = self.get_data_channel(true) {
            dc.close();
        }
        if let Some(dc) = self.get_data_channel(false) {
            dc.close();
        }
        self.get_peer_connection()
            .ok_or_else(|| format!("Peer connection not found"))?
            .close();
        Ok(())
    }
}

// Ensure local tab/window closing proactively closes PC so remote detects it immediately
fn register_unload_close(pc: RtcPeerConnection) {
    if let Some(window) = web_sys::window() {
        // pagehide
        let pc_ph = pc.clone();
        let on_pagehide = Closure::wrap(Box::new(move |_ev: web_sys::Event| {
            pc_ph.close();
        }) as Box<dyn FnMut(web_sys::Event)>);
        let _ = window
            .add_event_listener_with_callback("pagehide", on_pagehide.as_ref().unchecked_ref());
        on_pagehide.forget();
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
                if state == "failed" || state == "closed" {
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
                if state == "failed" || state == "closed" {
                    flag_ice.set(true);
                }
            }
        }
    }) as Box<dyn FnMut()>);
    pc.set_oniceconnectionstatechange(Some(ice_state_closure.as_ref().unchecked_ref()));
    ice_state_closure.forget();
}

// Build an RTCPeerConnection with Google STUN
fn make_peer_connection() -> Result<RtcPeerConnection, JsValue> {
    let ice = RtcIceServer::new();
    ice.set_urls(&JsValue::from_str(STUN_SERVER_URL));
    let cfg = web_sys::RtcConfiguration::new();
    let servers = js_sys::Array::new();
    servers.push(&ice.into());
    cfg.set_ice_servers(&servers);
    RtcPeerConnection::new_with_configuration(&cfg)
}

fn make_and_hook_peer_connection(connection: &RtcConnection) -> Result<RtcPeerConnection, String> {
    let pc = make_peer_connection()
        .map_err(|err| format!("Failed to create RTCPeerConnection: {:?}", err))?;
    hook_peer_connection(connection.pending_closed.clone(), &pc);
    Ok(pc)
}

// Setup data channel callbacks and store channel
fn hook_data_channel(
    pending_open: Rc<Cell<bool>>,
    pending_closed: Rc<Cell<bool>>,
    pending_messages: Rc<RefCell<Vec<Vec<u8>>>>,
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

    // onmessage -> push binary messages
    let msg_closure = Closure::wrap(Box::new(move |ev: web_sys::MessageEvent| {
        let data = ev.data();
        if let Ok(ab) = data.clone().dyn_into::<js_sys::ArrayBuffer>() {
            let u8 = Uint8Array::new(&ab);
            on_msg_buf.borrow_mut().push(u8.to_vec());
        } else if let Ok(blob) = data.clone().dyn_into::<web_sys::Blob>() {
            // Handle blob if necessary, for now assume ArrayBuffer (binaryType = "arraybuffer")
            // Ideally we set binaryType on the channel
            let _ = blob;
        }
    }) as Box<dyn FnMut(web_sys::MessageEvent)>);
    dc.set_binary_type(web_sys::RtcDataChannelType::Arraybuffer);
    dc.set_onmessage(Some(msg_closure.as_ref().unchecked_ref()));
    msg_closure.forget();
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

async fn update_description(
    sdp_buf: &Rc<RefCell<Vec<String>>>,
    pc: &RtcPeerConnection,
    description: RtcSessionDescriptionInit,
) -> Result<(), String> {
    wasm_bindgen_futures::JsFuture::from(pc.set_local_description(&description))
        .await
        .map_err(|err| format!("Failed to set local description: {:?}", err))?;

    await_ice_complete(pc.clone()).await;
    if let Some(local) = pc.local_description() {
        sdp_buf.borrow_mut().push(local.sdp());
    }
    Ok(())
}
