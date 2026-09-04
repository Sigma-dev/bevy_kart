//! The signalling server, so two peers on this machine can find each other.
//!
//! WebRTC peers cannot introduce themselves: somebody has to carry the offer, the answer and the
//! ICE candidates between them before there is any connection to carry them over. That somebody is
//! `bevy_ensemble_webrtc`'s server, and this is it.
//!
//! ```text
//! cargo run --example signalling_server                    # 127.0.0.1:9090
//! cargo run --example signalling_server -- 9095            # another port
//! cargo run --example signalling_server -- 0.0.0.0:9090    # reachable from the LAN
//! ```
//!
//! An example rather than a `[[bin]]` because examples may use dev-dependencies: the server half of
//! `bevy_ensemble_webrtc`, `axum` and tokio have no business being linked into the game.
//!
//! `scripts/local-session.sh` starts this and a few copies of the game in one command. By hand:
//!
//! ```text
//! cargo run --example signalling_server                              # terminal one
//! SIGNALLING_SERVER_URL=ws://127.0.0.1:9090/ws cargo run             # terminal two, host
//! SIGNALLING_SERVER_URL=ws://127.0.0.1:9090/ws cargo run             # terminal three, join by code
//! ```
//!
//! The game looks at the public server unless `SIGNALLING_SERVER_URL` says otherwise.

use std::sync::Arc;

use axum::Router;
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use bevy_ensemble_webrtc::server::{ServerState, handle_socket};

const DEFAULT_ADDRESS: &str = "127.0.0.1:9090";

/// Where to listen, from the first argument.
///
/// A bare port means loopback, which is the common case. A full `host:port` is for anything else,
/// `0.0.0.0:9090` being the one worth knowing: it makes the server reachable from other machines.
fn address() -> String {
    match std::env::args().nth(1) {
        None => DEFAULT_ADDRESS.to_string(),
        Some(argument) if argument.parse::<u16>().is_ok() => format!("127.0.0.1:{argument}"),
        Some(argument) => argument,
    }
}

#[tokio::main]
async fn main() {
    let address = address();
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .unwrap_or_else(|error| panic!("could not bind {address}: {error}"));

    println!("signalling server listening on ws://{address}/ws");

    let router = Router::new()
        .route("/ws", get(upgrade))
        .with_state(Arc::new(ServerState::new()));

    axum::serve(listener, router)
        .await
        .expect("the signalling server stopped");
}

async fn upgrade(
    websocket: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
) -> impl IntoResponse {
    websocket.on_upgrade(move |socket| handle_socket(socket, state))
}
