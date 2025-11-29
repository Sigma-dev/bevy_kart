//! Minimal browser WebRTC (WASM) using Bevy events and web-sys.
use crate::connection::RtcConnection;
use bevy::prelude::*;
use std::collections::HashMap;

pub(crate) mod api;
pub(crate) mod connection;
pub mod prelude;
pub(crate) mod systems;

pub use api::WebRtc;

pub struct WebRtcPlugin;

impl Plugin for WebRtcPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ConnectionIdSeq::default())
            .insert_non_send_resource(RtcContext::default())
            .add_message::<WebRtcApiCall>()
            .add_message::<WebRtcUpdate>()
            .add_plugins(systems::run_systems);
    }
}

type Sdp = String;
type Data = Vec<u8>;

// Messages exposed to Bevy application code (now include ConnectionId)
#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub struct ConnectionId(pub u64);

#[derive(Resource, Default)]
struct ConnectionIdSeq(u64);

impl ConnectionIdSeq {
    fn next(&mut self) -> ConnectionId {
        let id = self.0;
        self.0 = self.0.wrapping_add(1);
        ConnectionId(id)
    }
}

#[derive(Message)]
pub(crate) enum WebRtcApiCall {
    CreateOffer(ConnectionId),
    CreateAnswer(ConnectionId, Sdp),
    AcceptAnswer(ConnectionId, Sdp),
    SendData(ConnectionId, Data),
    SendUnreliableData(ConnectionId, Data),
    CloseConnection(ConnectionId),
    CloseAllConnections,
}

#[derive(Message, Debug, Clone)]
pub enum WebRtcUpdate {
    LocalSdp { id: ConnectionId, sdp: Sdp },
    IncomingData { id: ConnectionId, data: Data },
    ConnectionOpen(ConnectionId),
    ConnectionClosed(ConnectionId),
}

// NonSend resource (do not derive Resource; inserted as NonSend)
#[derive(Default)]
struct RtcContext {
    connections: HashMap<ConnectionId, RtcConnection>,
}

impl RtcContext {
    fn iter_connections(&self) -> impl Iterator<Item = (&ConnectionId, &RtcConnection)> {
        self.connections.iter()
    }
    fn insert_connection(&mut self, id: ConnectionId, connection: RtcConnection) {
        self.connections.insert(id, connection);
    }
    fn get_connection(&self, id: ConnectionId) -> Option<&RtcConnection> {
        self.connections.get(&id)
    }
    fn close_connection(&mut self, id: ConnectionId) -> Result<(), String> {
        let state = self
            .get_connection(id)
            .ok_or_else(|| format!("Connection {:?} not found", id))?;
        state.close()?;
        self.connections.remove(&id);
        Ok(())
    }
}
