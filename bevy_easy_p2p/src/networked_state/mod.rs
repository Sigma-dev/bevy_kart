use crate::EasyP2PState;
use crate::api::EasyP2PData;
use crate::schedules::EasyProcess;
use crate::state::P2PData;
use crate::transports::api::EasyP2PTransportRequest;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::state::state::FreelyMutableState;
use core::any::TypeId;
use serde::{Deserialize, Serialize};

pub trait NetworkedStatesExt {
    fn init_networked_state<S, T>(&mut self) -> &mut Self
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
        T: EasyP2PData;
}

impl NetworkedStatesExt for App {
    fn init_networked_state<S, T>(&mut self) -> &mut Self
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
        T: EasyP2PData,
    {
        self.add_plugins(NetworkedStatePlugin::<S, T>::default());
        self
    }
}

pub struct NetworkedStatePlugin<S, T: EasyP2PData>(std::marker::PhantomData<(S, T)>);

impl<S, T: EasyP2PData> Default for NetworkedStatePlugin<S, T> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

#[derive(Resource, Default)]
pub struct SyncedStateRegister {
    pub readers: Vec<fn(&str, &mut Commands) -> ()>,
    pub indexes: HashMap<TypeId, u8>,
    pub counter: u8,
}

#[derive(Message)]
pub(crate) struct OnApplyState<S>(pub S)
where
    S: States + Clone + Send + Sync + 'static;

impl SyncedStateRegister {
    pub fn register_state<S>(&mut self)
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
        if self.indexes.contains_key(&TypeId::of::<S>()) {
            return;
        }
        let idx = self.counter;
        self.indexes.insert(TypeId::of::<S>(), idx);
        self.counter = self.counter.wrapping_add(1);
        self.readers.push(|payload: &str, commands: &mut Commands| {
            if let Ok(value) = serde_json::from_str::<S>(payload) {
                commands.set_state::<S>(value);
            }
        });
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
    state: Res<EasyP2PState<T::PlayerData>>,
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
    if !state.is_host {
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
