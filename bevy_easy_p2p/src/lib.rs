use bevy::prelude::*;
use bevy::state::state::FreelyMutableState;
use serde::{Deserialize, Serialize};

mod api;
mod networked_event;
mod networked_state;
mod schedules;
mod state;
mod systems;
mod transports;
mod updates;

pub mod networked_transform;
pub mod prelude;

pub use api::{EasyP2P, EasyP2PData, EasyP2PPlugin, ExitReason, OnApplyState, PingUpdate};
pub use state::*;
pub use transports::easy_firestore_p2p;
pub use updates::EasyP2PUpdate;

pub type ClientId = u64;

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
        self.add_plugins(networked_state::NetworkedStatePlugin::<S, T>::default());
        self
    }
}

pub trait NetworkedEventsExt {
    fn init_networked_event<E, T>(&mut self) -> &mut Self
    where
        E: Serialize
            + for<'de> Deserialize<'de>
            + Clone
            + Send
            + Sync
            + core::fmt::Debug
            + 'static
            + Message,
        T: EasyP2PData;
}

impl NetworkedEventsExt for App {
    fn init_networked_event<E, T>(&mut self) -> &mut Self
    where
        E: Serialize
            + for<'de> Deserialize<'de>
            + Clone
            + Send
            + Sync
            + core::fmt::Debug
            + 'static
            + Message,
        T: EasyP2PData,
    {
        self.add_plugins(networked_event::NetworkedEventPlugin::<E, T>::default());
        self
    }
}
