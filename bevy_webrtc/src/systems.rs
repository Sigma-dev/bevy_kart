use bevy::prelude::*;

use crate::{
    ConnectionId, Data, RtcContext, Sdp, WebRtcApiCall, WebRtcUpdate, connection::RtcConnection,
};

pub(crate) fn run_systems(app: &mut App) {
    app.add_systems(PreUpdate, pump_js_callbacks)
        .add_systems(PostUpdate, read_api_calls);
}

// Periodically pump events produced by JS callbacks and async tasks back into Bevy
fn pump_js_callbacks(
    mut ctx: NonSendMut<RtcContext>,
    mut event_writer: MessageWriter<WebRtcUpdate>,
) {
    // Iterate over connections and flush their pending buffers with ids
    let mut to_remove = Vec::new();
    for (id, state) in ctx.iter_connections() {
        if state.pending_open.replace(false) {
            event_writer.write(WebRtcUpdate::ConnectionOpen(*id));
        }
        if state.pending_closed.replace(false) {
            event_writer.write(WebRtcUpdate::ConnectionClosed(*id));
            to_remove.push(*id);
        }
        for s in state.pending_messages.borrow_mut().drain(..) {
            event_writer.write(WebRtcUpdate::IncomingData { id: *id, data: s });
        }
        for s in state.pending_local_sdp.borrow_mut().drain(..) {
            event_writer.write(WebRtcUpdate::LocalSdp { id: *id, sdp: s });
        }
    }
    for id in to_remove {
        if let Err(err) = ctx.close_connection(id) {
            error!("Failed to close connection {:?}: {:?}", id, err);
        }
    }
}

fn read_api_calls(
    mut ctx: NonSendMut<RtcContext>,
    mut ev: MessageReader<WebRtcApiCall>,
    mut update_w: MessageWriter<WebRtcUpdate>,
) {
    for call in ev.read() {
        if let Err(err) = match call {
            WebRtcApiCall::CreateOffer(id) => create_offer(&mut ctx, *id),
            WebRtcApiCall::CreateAnswer(id, remote_sdp) => create_answer(&mut ctx, *id, remote_sdp),
            WebRtcApiCall::AcceptAnswer(id, sdp) => accept_answer(&ctx, *id, sdp),
            WebRtcApiCall::SendData(id, data) => send_data(&ctx, *id, data, true),
            WebRtcApiCall::SendUnreliableData(id, data) => send_data(&ctx, *id, data, false),
            WebRtcApiCall::CloseConnection(id) => close_connection(&mut ctx, *id, &mut update_w),
            WebRtcApiCall::CloseAllConnections => close_all_connections(&mut ctx, &mut update_w),
        } {
            error!("Failed to process API call: {:?}", err);
        }
    }
}

fn create_offer(ctx: &mut NonSendMut<RtcContext>, id: ConnectionId) -> Result<(), String> {
    let connection = RtcConnection::new_offer()?;
    ctx.insert_connection(id, connection);
    Ok(())
}

fn create_answer(
    ctx: &mut NonSendMut<RtcContext>,
    id: ConnectionId,
    remote_sdp: &Sdp,
) -> Result<(), String> {
    let connection = RtcConnection::new_answer(remote_sdp)?;
    ctx.insert_connection(id, connection);
    Ok(())
}

fn accept_answer(ctx: &NonSendMut<RtcContext>, id: ConnectionId, sdp: &Sdp) -> Result<(), String> {
    ctx.get_connection(id)
        .ok_or_else(|| format!("Connection {:?} not found", id))?
        .accept_answer(sdp)
        .map_err(|e| format!("Failed to accept answer: {:?}", e))
}

fn send_data(
    ctx: &NonSendMut<RtcContext>,
    id: ConnectionId,
    data: &Data,
    reliable: bool,
) -> Result<(), String> {
    ctx.get_connection(id)
        .ok_or_else(|| format!("Connection {:?} not found", id))?
        .send_data(data, reliable)
}

fn close_connection(
    ctx: &mut NonSendMut<RtcContext>,
    id: ConnectionId,
    event_writer: &mut MessageWriter<WebRtcUpdate>,
) -> Result<(), String> {
    ctx.close_connection(id)?;

    // Emit manually because it will not be pumped by the pump_js_callbacks system
    event_writer.write(WebRtcUpdate::ConnectionClosed(id));
    Ok(())
}

fn close_all_connections(
    ctx: &mut NonSendMut<RtcContext>,
    event_writer: &mut MessageWriter<WebRtcUpdate>,
) -> Result<(), String> {
    let ids: Vec<ConnectionId> = ctx.iter_connections().map(|(id, _)| *id).collect();
    for id in ids {
        if let Err(err) = close_connection(ctx, id, event_writer) {
            error!("Failed to close connection {:?}: {:?}", id, err);
        }
    }
    Ok(())
}
