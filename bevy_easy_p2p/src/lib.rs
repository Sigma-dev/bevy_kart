mod api;
mod networked_event;
mod networked_instantiation;
mod networked_state;
mod networked_transform;
mod ping;
mod schedules;
mod state;
mod systems;
mod transports;
mod updates;

pub mod prelude;
pub use api::{EasyP2P, EasyP2PData, EasyP2PPlugin, ExitReason};
pub use state::*;
pub use transports::easy_firestore_p2p;
pub use updates::EasyP2PUpdate;

pub type ClientId = u64;
