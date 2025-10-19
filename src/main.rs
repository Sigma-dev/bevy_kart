use bevy::prelude::*;
use bevy_webrtc::{
    ConnectionId, ConnectionOpen, CreateAnswer, CreateOffer, IncomingData, LocalSdpReady, SendData,
    SetRemote, WebRtcPlugin,
};

const BASE62_ALPHABET: &str = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

fn sdp_to_code(sdp: &str) -> String {
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(sdp.as_bytes(), 7);
    base_x::encode(BASE62_ALPHABET.as_bytes(), &compressed)
}

fn code_to_sdp(code: &str) -> Option<String> {
    let cleaned = code.trim();
    let bytes = base_x::decode(BASE62_ALPHABET.as_bytes(), cleaned).ok()?;
    let decompressed = miniz_oxide::inflate::decompress_to_vec_zlib(&bytes).ok()?;
    String::from_utf8(decompressed).ok()
}

struct TestSignalingPlugin;

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct NetConnection {
    id: ConnectionId,
}

#[derive(Resource, Default)]
struct ConnectionIdAllocator {
    next: u64,
}

impl ConnectionIdAllocator {
    fn allocate(&mut self) -> ConnectionId {
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        ConnectionId(id)
    }
}

impl Plugin for TestSignalingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConnectionIdAllocator>()
            .add_systems(Startup, auto_answer_from_url)
            .add_systems(
                Update,
                (
                    keyboard_shortcuts,
                    log_local_sdp_ready,
                    log_connection_open,
                    log_incoming_data,
                ),
            );
    }
}

fn prompt(label: &str) -> Option<String> {
    web_sys::window()
        .and_then(|w| w.prompt_with_message(&label).ok())
        .flatten()
}

// Keyboard shortcuts:
// - O: Create offer (offerer)
// - A: Paste remote OFFER CODE -> create answer (answerer)
// - R: Paste remote ANSWER CODE -> set remote (offerer)
// - T: Send a text message on the data channel (both sides)
fn keyboard_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut w_offer: MessageWriter<CreateOffer>,
    mut w_answer: MessageWriter<CreateAnswer>,
    mut w_set: MessageWriter<SetRemote>,
    mut w_send: MessageWriter<SendData>,
    mut id_alloc: ResMut<ConnectionIdAllocator>,
    q_conns: Query<&NetConnection>,
) {
    if keys.just_pressed(KeyCode::KeyO) {
        let id = id_alloc.allocate();
        w_offer.write(CreateOffer { id });
        info!(
            "Creating offer for connection {:?}... wait for LOCAL CODE popup and console log.",
            id
        );
    }
    if keys.just_pressed(KeyCode::KeyA) {
        if let Some(code) = prompt("Paste REMOTE OFFER CODE:") {
            if let Some(sdp) = code_to_sdp(&code) {
                if !sdp.trim().is_empty() {
                    let id = id_alloc.allocate();
                    w_answer.write(CreateAnswer {
                        id,
                        remote_sdp: sdp,
                    });
                    info!(
                        "Creating answer for connection {:?}... wait for LOCAL CODE popup and console log.",
                        id
                    );
                }
            } else {
                info!("Invalid code. Please check and try again.");
            }
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        if let Some(code) = prompt("Paste REMOTE ANSWER CODE:") {
            if let Some(sdp) = code_to_sdp(&code) {
                if !sdp.trim().is_empty() {
                    // Determine target connection id
                    let target_id = select_target_connection(&q_conns);
                    if let Some(id) = target_id {
                        w_set.write(SetRemote { id, sdp });
                        info!(
                            "Applied remote answer for connection {:?}. Waiting for data channel to open...",
                            id
                        );
                    } else {
                        info!("No connection selected. Create a connection first (O or A).");
                    }
                }
            } else {
                info!("Invalid code. Please check and try again.");
            }
        }
    }
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

fn select_target_connection(q: &Query<&NetConnection>) -> Option<ConnectionId> {
    if let Some(id) = only_connection(q) {
        return Some(id);
    }
    let entered = prompt("Target connection id (number):").unwrap_or_default();
    entered.trim().parse::<u64>().ok().map(ConnectionId)
}

fn log_local_sdp_ready(mut r: MessageReader<LocalSdpReady>, mut commands: Commands) {
    for LocalSdpReady { id, sdp } in r.read() {
        let code = sdp_to_code(&sdp);
        info!(
            "[{:?}] ===== LOCAL_CODE_BEGIN =====\n{}\n===== LOCAL_CODE_END =====",
            id, code
        );
        if let Some(base) = current_base_url() {
            let link = format!("{}?code={}", base, code);
            info!("[{:?}] Shareable link (opens with this code): {}", id, link);
        }
        info!(
            "[{:?}] Local CODE logged to console. A prompt will show it for easy copy.",
            id
        );
        // Ensure an entity exists for this connection id
        commands.spawn(NetConnection { id: *id });
    }
}

fn log_connection_open(mut r: MessageReader<ConnectionOpen>) {
    for ConnectionOpen(id) in r.read() {
        info!("[{:?}] Data channel is OPEN", id);
    }
}

fn log_incoming_data(mut r: MessageReader<IncomingData>) {
    for IncomingData { id, text } in r.read() {
        info!("[{:?}] RECEIVED: {}", id, text);
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

// Extract the `code` query parameter from the current URL if present
fn extract_code_query_param() -> Option<String> {
    let href = get_url()?;
    let no_hash = href.split('#').next().unwrap_or(href.as_str());
    let query = no_hash.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        let key = it.next()?;
        if key == "code" {
            let val = it.next().unwrap_or("");
            return Some(val.to_string());
        }
    }
    None
}

// On page load, if `?code=` is present, treat it as a REMOTE OFFER CODE and create an answer immediately
fn auto_answer_from_url(
    mut w_answer: MessageWriter<CreateAnswer>,
    mut commands: Commands,
    mut id_alloc: ResMut<ConnectionIdAllocator>,
) {
    if let Some(code) = extract_code_query_param() {
        if let Some(sdp) = code_to_sdp(&code) {
            if !sdp.trim().is_empty() {
                let id = id_alloc.allocate();
                info!(
                    "URL contained an offer code. Creating answer automatically for connection {:?}...",
                    id
                );
                w_answer.write(CreateAnswer {
                    id,
                    remote_sdp: sdp,
                });
                commands.spawn(NetConnection { id });
            }
        } else {
            info!("Invalid ?code URL parameter. Unable to decode offer.");
        }
    } else {
        info!(
            "No ?code URL parameter found. Unable to create answer automatically. URL: {}",
            get_url().unwrap_or_default()
        );
    }
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((WebRtcPlugin, TestSignalingPlugin))
        .run();
}
