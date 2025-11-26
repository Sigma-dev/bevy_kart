use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::ClientId;

type LobbyIdentifier = String;

#[derive(Message)]
pub enum EasyP2PTransportRequest<Packet: Send + Sync + 'static> {
    CreateLobby,
    JoinLobby(LobbyIdentifier),
    ExitLobby,
    SendToHost(Packet),
    SendToAll(Packet),
    SendToClient(ClientId, Packet),
    Kick(ClientId),
    SendToAllExcept(ClientId, Packet),
}

#[derive(Message)]
pub enum EasyP2PTransportUpdate<Packet: Send + Sync + 'static> {
    LobbyCreated(LobbyIdentifier),
    LobbyJoined(LobbyIdentifier),
    LobbyExited,
    MessageReceivedFromHost(Packet),
    MessageReceivedFromClient(ClientId, Packet),
    RosterUpdated(Vec<ClientId>),
}

#[derive(SystemParam)]
pub struct EasyP2PTransportIo<'w, 's, Packet: Send + Sync + 'static> {
    requests_r: MessageReader<'w, 's, EasyP2PTransportRequest<Packet>>,
    updates_w: MessageWriter<'w, EasyP2PTransportUpdate<Packet>>,
}

impl<'w, 's, Packet: Send + Sync + 'static> EasyP2PTransportIo<'w, 's, Packet> {
    pub fn take_requests(&mut self) -> impl Iterator<Item = &EasyP2PTransportRequest<Packet>> {
        self.requests_r.read()
    }

    pub fn emit_update(&mut self, update: EasyP2PTransportUpdate<Packet>) {
        self.updates_w.write(update);
    }
}
