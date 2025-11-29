use bevy::{ecs::system::SystemParam, prelude::*};

use crate::{ConnectionId, ConnectionIdSeq, WebRtcApiCall, WebRtcUpdate};

#[derive(SystemParam)]
pub struct WebRtc<'w, 's> {
    next_id: ResMut<'w, ConnectionIdSeq>,
    api_call_writer: MessageWriter<'w, WebRtcApiCall>,
    update_reader: MessageReader<'w, 's, WebRtcUpdate>,
}

impl<'w, 's> WebRtc<'w, 's> {
    pub fn create_offer(&mut self) -> ConnectionId {
        let id = self.next_id.next();
        self.send_api_call(WebRtcApiCall::CreateOffer(id));
        id
    }

    pub fn create_answer(&mut self, remote_offer_sdp: impl Into<String>) -> ConnectionId {
        let id = self.next_id.next();
        self.send_api_call(WebRtcApiCall::CreateAnswer(id, remote_offer_sdp.into()));
        id
    }

    pub fn accept_answer(&mut self, id: ConnectionId, answer_sdp: impl Into<String>) {
        self.send_api_call(WebRtcApiCall::AcceptAnswer(id, answer_sdp.into()));
    }

    pub fn send_data(&mut self, id: ConnectionId, data: Vec<u8>) {
        self.send_api_call(WebRtcApiCall::SendData(id, data));
    }

    pub fn send_unreliable_data(&mut self, id: ConnectionId, data: Vec<u8>) {
        self.send_api_call(WebRtcApiCall::SendUnreliableData(id, data));
    }

    pub fn close(&mut self, id: ConnectionId) {
        self.send_api_call(WebRtcApiCall::CloseConnection(id));
    }

    pub fn close_all(&mut self) {
        self.send_api_call(WebRtcApiCall::CloseAllConnections);
    }

    pub fn read_updates(&mut self) -> Vec<WebRtcUpdate> {
        let updates: Vec<_> = self
            .update_reader
            .read()
            .map(|update| update.clone())
            .collect();
        self.update_reader.clear();
        updates
    }

    fn send_api_call(&mut self, api_call: WebRtcApiCall) {
        self.api_call_writer.write(api_call);
    }
}
