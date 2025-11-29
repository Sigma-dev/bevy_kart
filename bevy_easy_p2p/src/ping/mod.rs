use std::time::Duration;

use crate::schedules::EasyProcess;
use crate::{prelude::*, transports::api::EasyP2PTransportRequest};
use bevy::{prelude::*, time::common_conditions::on_timer};

pub struct PingPlugin<T: EasyP2PData>(std::marker::PhantomData<T>);

impl<T: EasyP2PData> Default for PingPlugin<T> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<T: EasyP2PData> Plugin for PingPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_message::<PingUpdate>().add_systems(
            EasyProcess,
            send_ping::<T>.run_if(on_timer(Duration::from_secs(1))),
        );
    }
}

#[derive(Message)]
pub struct PingUpdate(pub std::time::Duration);

pub(crate) fn send_ping<T: EasyP2PData>(
    time: Res<Time>,
    state: Res<EasyP2PState<T::PlayerData>>,
    mut w_send_host: MessageWriter<EasyP2PTransportRequest<P2PData<T>>>,
) {
    if state.is_host {
        return;
    }

    w_send_host.write(EasyP2PTransportRequest::SendToHost(
        P2PData::PingRequest(time.elapsed_secs()),
        crate::transports::api::Reliability::Reliable,
    ));
}
