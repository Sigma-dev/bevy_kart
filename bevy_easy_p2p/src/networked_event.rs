use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::api::EasyP2PData;
use crate::schedules::EasyProcess;
use crate::state::P2PData;
use crate::state::{IsHost, SyncedEventRegister};
use crate::transports::api::EasyP2PTransportRequest;
use core::any::TypeId;

pub struct NetworkedEventPlugin<E, T: EasyP2PData>(std::marker::PhantomData<(E, T)>);

impl<E, T: EasyP2PData> Default for NetworkedEventPlugin<E, T> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<E, T: EasyP2PData> Plugin for NetworkedEventPlugin<E, T>
where
    E: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Message,
{
    fn build(&self, app: &mut App) {
        app.add_message::<E>();
        {
            let mut reg = app
                .world_mut()
                .get_resource_mut::<SyncedEventRegister>()
                .expect("SyncedEventRegister not initialized");
            reg.register_event::<E>();
        }
        app.add_systems(EasyProcess, host_broadcast_event::<E, T>);
    }
}

pub(crate) fn host_broadcast_event<E, T: EasyP2PData>(
    host_flag: Res<IsHost>,
    mut events: MessageReader<E>,
    register: Res<SyncedEventRegister>,
    mut w_send_all: MessageWriter<EasyP2PTransportRequest<P2PData<T>>>,
) where
    E: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Message,
{
    if !host_flag.0 {
        return;
    }
    for e in events.read() {
        if let Some(index) = register.indexes.get(&TypeId::of::<E>()) {
            match serde_json::to_string(e) {
                Ok(text) => {
                    let payload = P2PData::EventSync(*index, text);
                    w_send_all.write(EasyP2PTransportRequest::SendToAll(payload));
                }
                Err(err) => {
                    warn!("Error serializing event for sync: {:?}", err);
                }
            }
        }
    }
}

pub(crate) struct EmitSyncedEvent {
    pub(crate) index: u8,
    pub(crate) payload: String,
}

impl bevy::ecs::system::Command for EmitSyncedEvent {
    fn apply(self, world: &mut World) {
        let Some(register) = world.get_resource::<SyncedEventRegister>() else {
            return;
        };
        let idx = self.index as usize;
        if idx < register.readers.len() {
            let reader = register.readers[idx];
            reader(&self.payload, world);
        }
    }
}
