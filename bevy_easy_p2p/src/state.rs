use bevy::{prelude::*, state::state::FreelyMutableState};
use core::any::TypeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Default, Eq, PartialEq, Debug, Hash, States)]
pub enum P2PLobbyState {
    #[default]
    OutOfLobby,
    JoiningLobby,
    InLobby,
}

#[derive(Component, Serialize, Deserialize, Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum NetworkedId {
    Host,
    ClientId(u64),
}

#[derive(Component)]
pub struct NetworkedEntity {
    pub(crate) id: NetworkedId,
    pub(crate) despawn_on_leave: bool,
}

impl NetworkedEntity {
    pub fn new(id: NetworkedId) -> Self {
        Self {
            id,
            despawn_on_leave: true,
        }
    }

    pub fn id(&self) -> NetworkedId {
        self.id
    }

    pub fn despawn_on_leave(&self) -> bool {
        self.despawn_on_leave
    }

    pub fn set_despawn_on_leave(&mut self, value: bool) {
        self.despawn_on_leave = value;
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerInfo<PlayerData> {
    pub id: NetworkedId,
    pub data: PlayerData,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum P2PData<PlayerData, PlayerInputData, Instantiations> {
    ClientLobbyChatMessage(String, NetworkedId),
    ClientInput(PlayerInputData),
    ClientDataUpdate(PlayerData),
    HostLobbyInfoUpdate(Vec<PlayerInfo<PlayerData>>),
    StateSync(u8, String),
    EventSync(u8, String),
    HostInstantiation(InstantiationDataNet<Instantiations>),
    PingRequest(f32),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NetTransform {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl From<&Transform> for NetTransform {
    fn from(t: &Transform) -> Self {
        Self {
            translation: [t.translation.x, t.translation.y, t.translation.z],
            rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
            scale: [t.scale.x, t.scale.y, t.scale.z],
        }
    }
}

impl From<&NetTransform> for Transform {
    fn from(nt: &NetTransform) -> Self {
        Transform::from_xyz(nt.translation[0], nt.translation[1], nt.translation[2])
            .with_rotation(Quat::from_xyzw(
                nt.rotation[0],
                nt.rotation[1],
                nt.rotation[2],
                nt.rotation[3],
            ))
            .with_scale(Vec3::new(nt.scale[0], nt.scale[1], nt.scale[2]))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InstantiationDataNet<Instantiations> {
    pub transform: NetTransform,
    pub instantiation: Instantiations,
}

#[derive(Clone, Debug)]
pub struct InstantiationData<Instantiations> {
    pub transform: Transform,
    pub instantiation: Instantiations,
}

impl<Instantiations: Clone> From<&InstantiationData<Instantiations>>
    for InstantiationDataNet<Instantiations>
{
    fn from(value: &InstantiationData<Instantiations>) -> Self {
        Self {
            transform: NetTransform::from(&value.transform),
            instantiation: value.instantiation.clone(),
        }
    }
}

impl<Instantiations: Clone> From<&InstantiationDataNet<Instantiations>>
    for InstantiationData<Instantiations>
{
    fn from(value: &InstantiationDataNet<Instantiations>) -> Self {
        Self {
            transform: Transform::from(&value.transform),
            instantiation: value.instantiation.clone(),
        }
    }
}

#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct EasyP2PState<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
> {
    pub local_player_data: PlayerData,
    pub is_host: bool,
    pub lobby_code: String,
    pub players: Vec<PlayerInfo<PlayerData>>,
}

impl<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
> EasyP2PState<PlayerData>
{
    pub fn get_players(&self, add_host: bool) -> Vec<PlayerInfo<PlayerData>> {
        let mut players = if add_host {
            vec![PlayerInfo {
                id: NetworkedId::Host,
                data: self.local_player_data.clone(),
            }]
        } else {
            vec![]
        };
        players.extend(self.players.clone());
        players
    }
}

#[derive(Resource, Default, Clone, Copy)]
pub struct IsHost(pub bool);

#[derive(Resource, Default)]
pub struct SyncedStateRegister {
    pub readers: Vec<fn(&str, &mut Commands) -> ()>,
    pub indexes: HashMap<TypeId, u8>,
    pub counter: u8,
}

#[derive(Resource, Default)]
pub struct SyncedEventRegister {
    pub readers: Vec<fn(&str, &mut World) -> ()>,
    pub indexes: HashMap<TypeId, u8>,
    pub counter: u8,
}

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

impl SyncedEventRegister {
    pub fn register_event<E>(&mut self)
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
        if self.indexes.contains_key(&TypeId::of::<E>()) {
            return;
        }
        let idx = self.counter;
        self.indexes.insert(TypeId::of::<E>(), idx);
        self.counter = self.counter.wrapping_add(1);
        self.readers.push(|payload: &str, world: &mut World| {
            if let Ok(value) = serde_json::from_str::<E>(payload) {
                world.write_message(value);
            }
        });
    }
}
