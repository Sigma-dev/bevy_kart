use bevy::prelude::*;
use bevy::state::state::FreelyMutableState;
use serde::{Deserialize, Serialize};

mod api;
mod state;
mod systems;
mod updates;

pub mod networked_transform;
pub mod prelude;

pub use api::{
    EasyP2P, EasyP2PPlugin, EasyP2PSystemSet, EasyP2PTransportIo, ExitReason, OnApplyState,
    PingUpdate, P2PTransport,
};
pub use state::*;
pub use updates::{EasyP2PUpdate, EasyP2PUpdateQueue};

pub type ClientId = u64;

pub trait NetworkedStatesExt {
    fn init_networked_state<S>(&mut self) -> &mut Self
    where
        S: States
            + Serialize
            + for<'de> Deserialize<'de>
            + Clone
            + Send
            + Sync
            + core::fmt::Debug
            + 'static
            + FreelyMutableState;
}

impl NetworkedStatesExt for App {
    fn init_networked_state<S>(&mut self) -> &mut Self
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
        self.add_message::<OnApplyState<S>>();
        {
            let mut reg = self
                .world_mut()
                .get_resource_mut::<SyncedStateRegister>()
                .expect("SyncedStateRegister not initialized");
            reg.register_state::<S>();
        }
        self.add_systems(
            Update,
            systems::host_broadcast_state_change::<S>.in_set(EasyP2PSystemSet::Core),
        );
        self
    }
}

pub trait NetworkedEventsExt {
    fn init_networked_event<E>(&mut self) -> &mut Self
    where
        E: Serialize
            + for<'de> Deserialize<'de>
            + Clone
            + Send
            + Sync
            + core::fmt::Debug
            + 'static
            + Message;
}

impl NetworkedEventsExt for App {
    fn init_networked_event<E>(&mut self) -> &mut Self
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
        self.add_message::<E>();
        {
            let mut reg = self
                .world_mut()
                .get_resource_mut::<SyncedEventRegister>()
                .expect("SyncedEventRegister not initialized");
            reg.register_event::<E>();
        }
        self.add_systems(
            Update,
            systems::host_broadcast_event::<E>.in_set(EasyP2PSystemSet::Core),
        );
        self
    }
}
