use bevy::prelude::*;
use bevy::state::state::FreelyMutableState;
use serde::{Deserialize, Serialize};

use crate::OnApplyState;
use crate::api::EasyP2PData;
use crate::schedules::EasyProcess;
use crate::state::P2PData;
use crate::state::{IsHost, SyncedStateRegister};
use crate::transports::api::EasyP2PTransportRequest;
use core::any::TypeId;

pub struct NetworkedStatePlugin<S, T: EasyP2PData>(std::marker::PhantomData<(S, T)>);

impl<S, T: EasyP2PData> Default for NetworkedStatePlugin<S, T> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<S, T: EasyP2PData> Plugin for NetworkedStatePlugin<S, T>
where
    S: States
        + Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + FreelyMutableState,
{
    fn build(&self, app: &mut App) {
        app.add_message::<OnApplyState<S>>();
        {
            let mut reg = app
                .world_mut()
                .get_resource_mut::<SyncedStateRegister>()
                .expect("SyncedStateRegister not initialized");
            reg.register_state::<S>();
        }
        app.add_systems(EasyProcess, host_broadcast_state_change::<S, T>);
    }
}

pub(crate) fn host_broadcast_state_change<S, T: EasyP2PData>(
    host_flag: Res<IsHost>,
    current: Res<State<S>>,
    mut last: Local<Option<S>>,
    register: Res<SyncedStateRegister>,
    mut w_send_all: MessageWriter<EasyP2PTransportRequest<P2PData<T>>>,
) where
    S: States
        + Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + PartialEq
        + Send
        + Sync
        + core::fmt::Debug
        + 'static,
{
    if !host_flag.0 {
        return;
    }
    let current_value = current.get().clone();
    if last.as_ref().map(|v| v == &current_value).unwrap_or(false) {
        return;
    }
    *last = Some(current_value.clone());
    if let Some(index) = register.indexes.get(&TypeId::of::<S>()) {
        if let Ok(text) = serde_json::to_string(&current_value) {
            let payload = P2PData::StateSync(*index, text);
            w_send_all.write(EasyP2PTransportRequest::SendToAll(payload));
        }
    }
}
