//! Minimal browser WebRTC (WASM) using Bevy events and web-sys.

use bevy::prelude::*;
use js_sys::Uint8Array;
use std::cell::{Cell, RefCell};
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

// Messages exposed to Bevy application code
#[derive(Message, Default)]
pub struct CreateOffer;

#[derive(Message)]
pub struct CreateAnswer {
    pub remote_sdp: String,
}

#[derive(Message)]
pub struct SetRemote {
    pub sdp: String, // Answer SDP for offerer side
}

#[derive(Message)]
pub struct SendData {
    pub text: String,
}

#[derive(Message)]
pub struct LocalSdpReady(pub String);

#[derive(Message)]
pub struct IncomingData(pub String);

#[derive(Message)]
pub struct ConnectionOpen;

// NonSend resource (do not derive Resource; inserted as NonSend)
struct RtcContext {
    pc_slot: Rc<RefCell<Option<RtcPeerConnection>>>,
    dc_slot: Rc<RefCell<Option<RtcDataChannel>>>,
    // Buffers to bridge JS callbacks back into Bevy's world
    pending_open: Rc<Cell<bool>>,
    pending_messages: Rc<RefCell<Vec<String>>>,
    pending_local_sdp: Rc<RefCell<Vec<String>>>,
}

impl Default for RtcContext {
    fn default() -> Self {
        Self {
            pc_slot: Rc::new(RefCell::new(None)),
            dc_slot: Rc::new(RefCell::new(None)),
            pending_open: Rc::new(Cell::new(false)),
            pending_messages: Rc::new(RefCell::new(Vec::new())),
            pending_local_sdp: Rc::new(RefCell::new(Vec::new())),
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
            .add_systems(
                Update,
                (
                    handle_create_offer,
                    handle_create_answer,
                    handle_set_remote,
                    handle_send_data,
                    pump_js_callbacks,
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

// Setup data channel callbacks and store channel
fn hook_data_channel(
    pending_open: Rc<Cell<bool>>,
    pending_messages: Rc<RefCell<Vec<String>>>,
    dc: &RtcDataChannel,
) {
    let on_open_flag = pending_open.clone();
    let on_msg_buf = pending_messages.clone();

    // onopen -> mark flag
    let open_closure = Closure::wrap(Box::new(move || {
        on_open_flag.set(true);
    }) as Box<dyn FnMut()>);
    dc.set_onopen(Some(open_closure.as_ref().unchecked_ref()));
    open_closure.forget();

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

fn handle_create_offer(ctx: NonSendMut<RtcContext>, mut ev: MessageReader<CreateOffer>) {
    if ev.is_empty() {
        return;
    }
    ev.clear();

    // Fresh peer connection
    let pc = match make_peer_connection() {
        Ok(pc) => pc,
        Err(err) => {
            info!("Failed to create RTCPeerConnection: {:?}", err);
            return;
        }
    };

    // Create data channel immediately (offerer)
    let dc = pc.create_data_channel("data");
    hook_data_channel(ctx.pending_open.clone(), ctx.pending_messages.clone(), &dc);

    // Prepare to emit local SDP after ICE completes
    let sdp_buf = ctx.pending_local_sdp.clone();
    let pc_clone = pc.clone();

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
    });

    ctx.dc_slot.borrow_mut().replace(dc);
    ctx.pc_slot.borrow_mut().replace(pc);
}

fn handle_create_answer(ctx: NonSendMut<RtcContext>, mut ev: MessageReader<CreateAnswer>) {
    for CreateAnswer { remote_sdp } in ev.read() {
        let pc = match make_peer_connection() {
            Ok(pc) => pc,
            Err(err) => {
                info!("Failed to create RTCPeerConnection: {:?}", err);
                continue;
            }
        };

        // Listen for datachannel from offerer
        let on_dc_ctx_open = ctx.pending_open.clone();
        let on_dc_ctx_msgs = ctx.pending_messages.clone();
        let dc_slot = ctx.dc_slot.clone();
        let on_dc = Closure::wrap(Box::new(move |ev: RtcDataChannelEvent| {
            let channel = ev.channel();
            hook_data_channel(on_dc_ctx_open.clone(), on_dc_ctx_msgs.clone(), &channel);
            dc_slot.borrow_mut().replace(channel);
        }) as Box<dyn FnMut(RtcDataChannelEvent)>);
        pc.set_ondatachannel(Some(on_dc.as_ref().unchecked_ref()));
        on_dc.forget();

        let sdp_text = remote_sdp.clone();
        let sdp_buf = ctx.pending_local_sdp.clone();
        let pc_clone = pc.clone();
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

        ctx.pc_slot.borrow_mut().replace(pc);
    }
    ev.clear();
}

fn handle_set_remote(ctx: NonSend<RtcContext>, mut ev: MessageReader<SetRemote>) {
    if let Some(pc) = ctx.pc_slot.borrow().clone() {
        for SetRemote { sdp } in ev.read() {
            let pc_clone = pc.clone();
            let sdp_text = sdp.clone();
            spawn_local(async move {
                let desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
                desc.set_sdp(&sdp_text);
                let _ =
                    wasm_bindgen_futures::JsFuture::from(pc_clone.set_remote_description(&desc))
                        .await;
            });
        }
    }
    ev.clear();
}

fn handle_send_data(ctx: NonSend<RtcContext>, mut ev: MessageReader<SendData>) {
    if let Some(dc) = ctx.dc_slot.borrow().as_ref() {
        if dc.ready_state() == RtcDataChannelState::Open {
            for SendData { text } in ev.read() {
                let _ = dc.send_with_str(text.as_str());
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
) {
    if ctx.pending_open.replace(false) {
        open_writer.write(ConnectionOpen);
    }
    let mut msgs = ctx.pending_messages.borrow_mut();
    for s in msgs.drain(..) {
        msg_writer.write(IncomingData(s));
    }
    drop(msgs);
    let mut sdp = ctx.pending_local_sdp.borrow_mut();
    for s in sdp.drain(..) {
        sdp_writer.write(LocalSdpReady(s));
    }
}
