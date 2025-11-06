use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::prelude::*;
use bevy::prelude::*;
use bevy::time::common_conditions::on_timer;
use bevy_webrtc::prelude::*;
use std::time::Duration;
use web_sys::window;

#[derive(States, Default, Debug, Clone, PartialEq, Eq, Hash)]
enum ConnectionState {
    #[default]
    NoConnection,
    Negotiating(ConnectionId),
    Connected(ConnectionId),
}

#[derive(Message, Clone, Copy)]
struct PingUpdate {
    total_ms: u64,
    outbound_ms: u64,
    inbound_ms: u64,
}

impl PingUpdate {
    fn from_durations(outbound_ms: f64, inbound_ms: f64) -> Self {
        let outbound = outbound_ms.max(0.0).round() as u64;
        let inbound = inbound_ms.max(0.0).round() as u64;
        let total = (outbound_ms + inbound_ms).max(0.0).round() as u64;
        Self {
            total_ms: total,
            outbound_ms: outbound,
            inbound_ms: inbound,
        }
    }
}

#[derive(Resource, Default)]
struct PingDisplay(Option<PingUpdate>);

#[derive(Component)]
struct UIRoot;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, WebRtcPlugin))
        .init_state::<ConnectionState>()
        .init_resource::<PingDisplay>()
        .add_message::<PingUpdate>()
        .add_systems(Startup, setup_ui)
        .add_systems(
            Update,
            (
                handle_keyboard_input,
                process_webrtc_events,
                consume_ping_updates,
                update_ui,
                update_ping.run_if(on_timer(Duration::from_secs(1))),
            ),
        )
        .run();
}

fn alert(msg: &str) {
    if let Some(window) = window() {
        let _ = window.alert_with_message(msg);
    }
}

fn encode_sdp_to_base64(sdp: &str) -> String {
    URL_SAFE_NO_PAD.encode(sdp.as_bytes())
}

fn decode_sdp_from_base64(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let decoded = URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| STANDARD.decode(trimmed))
        .ok()?;
    String::from_utf8(decoded).ok()
}

fn now_epoch_ms() -> f64 {
    js_sys::Date::now()
}

fn setup_ui(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn handle_keyboard_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut webrtc: WebRtc,
    state: Res<State<ConnectionState>>,
    mut next_state: ResMut<NextState<ConnectionState>>,
) {
    fn prompt(msg: &str) -> Option<String> {
        window()?.prompt_with_message(msg).ok().flatten()
    }

    match state.get() {
        ConnectionState::NoConnection => {
            if keyboard_input.just_pressed(KeyCode::KeyC) {
                // Create offer
                let id = webrtc.create_offer();
                next_state.set(ConnectionState::Negotiating(id));
            } else if keyboard_input.just_pressed(KeyCode::KeyJ) {
                // Join - prompt for offer SDP
                if let Some(offer_sdp_encoded) = prompt("Enter the offer SDP (base64):") {
                    match decode_sdp_from_base64(&offer_sdp_encoded) {
                        Some(offer_sdp) => {
                            let id = webrtc.create_answer(offer_sdp);
                            next_state.set(ConnectionState::Negotiating(id));
                        }
                        None => {
                            alert(
                                "Failed to decode offer SDP. Please ensure it is base64 encoded.",
                            );
                        }
                    }
                }
            }
        }
        ConnectionState::Negotiating(id) => {
            if keyboard_input.just_pressed(KeyCode::KeyR) {
                if let Some(answer_sdp_encoded) = prompt("Enter the answer SDP (base64):") {
                    match decode_sdp_from_base64(&answer_sdp_encoded) {
                        Some(answer_sdp) => {
                            webrtc.set_remote_answer(*id, answer_sdp);
                        }
                        None => {
                            alert(
                                "Failed to decode answer SDP. Please ensure it is base64 encoded.",
                            );
                        }
                    }
                }
            } else if keyboard_input.just_pressed(KeyCode::KeyE) {
                webrtc.close(*id);
            }
        }
        ConnectionState::Connected(id) => {
            if keyboard_input.just_pressed(KeyCode::KeyE) {
                webrtc.close(*id);
            }
        }
    }
}

fn process_webrtc_events(
    mut webrtc: WebRtc,
    mut ping_display: ResMut<PingDisplay>,
    mut ping_writer: MessageWriter<PingUpdate>,
    mut state: ResMut<NextState<ConnectionState>>,
) {
    let events = webrtc.drain_events();
    for event in events {
        match event {
            WebRtcEvent::LocalSdp { sdp, .. } => {
                let encoded = encode_sdp_to_base64(&sdp);
                alert(&format!("Share this SDP offer (base64):\n\n{}", encoded));
            }
            WebRtcEvent::IncomingData { id, text } => {
                if let Some(origin_str) = text.strip_prefix("PING:") {
                    if let Ok(origin) = origin_str.parse::<f64>() {
                        let response = format!("PONG:{}:{}", origin, now_epoch_ms());
                        webrtc.send_text(id, response);
                    }
                } else if let Some(rest) = text.strip_prefix("PONG:") {
                    let mut parts = rest.splitn(2, ':');
                    if let (Some(origin_str), Some(remote_str)) = (parts.next(), parts.next()) {
                        if let (Ok(origin), Ok(remote)) =
                            (origin_str.parse::<f64>(), remote_str.parse::<f64>())
                        {
                            let now = now_epoch_ms();
                            let outbound = remote - origin;
                            let inbound = now - remote;
                            ping_writer.write(PingUpdate::from_durations(outbound, inbound));
                        }
                    }
                }
            }
            WebRtcEvent::ConnectionOpen(id) => {
                state.set(ConnectionState::Connected(id));
            }
            WebRtcEvent::ConnectionClosed(_) => {
                ping_display.0 = None;
                state.set(ConnectionState::NoConnection);
            }
        }
    }
}

fn update_ui(
    mut commands: Commands,
    state: Res<State<ConnectionState>>,
    ping_display: Res<PingDisplay>,
    ui_root: Query<Entity, With<UIRoot>>,
) {
    // Despawn old UI if it exists
    for entity in ui_root.iter() {
        commands.entity(entity).despawn();
    }

    let text_content = match state.get() {
        ConnectionState::NoConnection => "No connection\n\nPress C to create offer\nPress J to join (paste base64 offer)\n".to_string(),
        ConnectionState::Negotiating(_) => "Negotiating connection...\n\nPress R to provide an answer (paste base64 answer)\nPress E to cancel".to_string(),
        ConnectionState::Connected(_) => {
            if let Some(ping) = ping_display.0 {
                format!(
                    "Ping: {} ms\nOutbound: {} ms\nInbound: {} ms\n\nPress E to exit",
                    ping.total_ms, ping.outbound_ms, ping.inbound_ms
                )
            } else {
                "Connected\nWaiting for ping...\n\nPress E to exit".to_string()
            }
        }
    };

    // Spawn new UI
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            UIRoot,
        ))
        .with_children(|parent| {
            parent.spawn(Text(text_content));
        });
}

fn update_ping(mut webrtc: WebRtc, state: Res<State<ConnectionState>>) {
    if !matches!(state.get(), ConnectionState::Connected(_)) {
        return;
    }

    if let ConnectionState::Connected(id) = state.get() {
        webrtc.send_text(*id, format!("PING:{}", now_epoch_ms()));
    }
}

fn consume_ping_updates(mut reader: MessageReader<PingUpdate>, mut display: ResMut<PingDisplay>) {
    for update in reader.read() {
        display.0 = Some(*update);
    }
}
