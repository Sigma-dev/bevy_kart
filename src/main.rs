use bevy::prelude::*;
use bevy_webrtc::{
    ConnectionOpen, CreateAnswer, CreateOffer, IncomingData, LocalSdpReady, SendData, SetRemote,
    WebRtcPlugin,
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

impl Plugin for TestSignalingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
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
) {
    if keys.just_pressed(KeyCode::KeyO) {
        w_offer.write(CreateOffer);
        info!("Creating offer... wait for LOCAL CODE popup and console log.");
    }
    if keys.just_pressed(KeyCode::KeyA) {
        if let Some(code) = prompt("Paste REMOTE OFFER CODE:") {
            if let Some(sdp) = code_to_sdp(&code) {
                if !sdp.trim().is_empty() {
                    w_answer.write(CreateAnswer { remote_sdp: sdp });
                    info!("Creating answer... wait for LOCAL CODE popup and console log.");
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
                    w_set.write(SetRemote { sdp });
                    info!("Applied remote answer. Waiting for data channel to open...");
                }
            } else {
                info!("Invalid code. Please check and try again.");
            }
        }
    }
    if keys.just_pressed(KeyCode::KeyT) {
        let text = prompt("Send text over data channel:").unwrap_or_default();
        if !text.is_empty() {
            info!("Sending text: {}", text);
            w_send.write(SendData { text });
        }
    }
}

fn log_local_sdp_ready(mut r: MessageReader<LocalSdpReady>) {
    for LocalSdpReady(sdp) in r.read() {
        let code = sdp_to_code(&sdp);
        info!(
            "===== LOCAL_CODE_BEGIN =====\n{}\n===== LOCAL_CODE_END =====",
            code
        );
        info!("Local CODE logged to console. A prompt will show it for easy copy.");
    }
}

fn log_connection_open(mut r: MessageReader<ConnectionOpen>) {
    if !r.is_empty() {
        r.clear();
        info!("Data channel is OPEN");
    }
}

fn log_incoming_data(mut r: MessageReader<IncomingData>) {
    for IncomingData(s) in r.read() {
        info!("RECEIVED: {}", s);
    }
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((WebRtcPlugin, TestSignalingPlugin))
        .run();
}
