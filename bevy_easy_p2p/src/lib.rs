use bevy::{ecs::system::SystemParam, prelude::*};
use serde::{Deserialize, Serialize};

pub type ClientId = u64;

#[derive(Clone, Copy, Default, Eq, PartialEq, Debug, Hash, States)]
pub enum P2PLobbyState {
    #[default]
    OutOfLobby,
    JoiningLobby,
    InLobby,
}

// Typed transport data
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum NetworkedId {
    Host,
    ClientId(u64),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerInfo<PlayerData> {
    pub id: NetworkedId,
    pub data: PlayerData,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum P2PData<PlayerData, PlayerInputData> {
    ClientLobbyChatMessage(String, NetworkedId),
    ClientInput(PlayerInputData),
    ClientDataUpdate(PlayerData),
    HostLobbyInfoUpdate(Vec<PlayerInfo<PlayerData>>),
}

// Events
#[derive(Message, Clone)]
pub struct OnLobbyCreated(pub String);
#[derive(Message, Clone)]
pub struct OnLobbyJoined(pub String);
#[derive(Message, Clone)]
pub struct OnLobbyEntered(pub String);
#[derive(Message, Clone)]
pub struct OnClientMessageReceived(pub ClientId, pub String);
#[derive(Message, Clone)]
pub struct OnHostMessageReceived(pub String);

#[derive(Message)]
pub struct OnRosterUpdate<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
>(pub Vec<PlayerInfo<PlayerData>>);
#[derive(Message, Clone)]
pub struct OnRelayToAllExcept<PlayerData, PlayerInputData>(
    pub ClientId,
    pub P2PData<PlayerData, PlayerInputData>,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Disconnected,
    Kicked,
}

#[derive(Message, Clone)]
pub struct OnLobbyExit(pub ExitReason);
#[derive(Message, Clone)]
pub struct OnTransportRosterChanged(pub Vec<String>);
#[derive(Message, Clone)]
pub struct OnTransportSendToHost(pub String);
#[derive(Message, Clone)]
pub struct OnTransportSendToAll(pub String);
#[derive(Message, Clone)]
pub struct OnTransportSendToClient(pub ClientId, pub String);
#[derive(Message, Clone)]
pub struct OnTransportRelayToAllExcept(pub ClientId, pub String);
#[derive(Message, Clone)]
pub struct OnTransportIncomingFromClient(pub ClientId, pub String);
#[derive(Message, Clone)]
pub struct OnTransportIncomingFromHost(pub String);
#[derive(Message, Clone)]
pub struct OnInternalClientData<PlayerData, PlayerInputData>(
    pub ClientId,
    pub P2PData<PlayerData, PlayerInputData>,
);
#[derive(Message, Clone)]
pub struct OnInternalHostData<PlayerData, PlayerInputData>(
    pub P2PData<PlayerData, PlayerInputData>,
);

// Heartbeat removed: rely on WebRTC close events instead

// Easy state
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
    pub fn get_players(&self) -> Vec<PlayerInfo<PlayerData>> {
        let mut players = vec![PlayerInfo {
            id: NetworkedId::Host,
            data: self.local_player_data.clone(),
        }];
        players.extend(self.players.clone());
        players
    }
}
// Transport abstraction (WASM-only expectation)
pub trait P2PTransport: Send + Sync + 'static {
    type Error;

    fn create_lobby(world: &mut World) -> Result<String, Self::Error>;
    fn join_lobby(world: &mut World, code: &str) -> Result<(), Self::Error>;
    fn exit_lobby(world: &mut World);
    fn send_to_host(world: &mut World, text: String);
    fn send_to_all(world: &mut World, text: String);
    fn kick(world: &mut World, client_id: ClientId);
    fn poll_transport(world: &mut World);
}

// SystemParam wrapper
#[derive(SystemParam)]
pub struct EasyP2P<
    'w,
    's,
    T: P2PTransport,
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
> {
    create_w: MessageWriter<'w, OnCreateLobbyReq>,
    join_w: MessageWriter<'w, OnJoinLobbyReq>,
    exit_w: MessageWriter<'w, OnExitLobbyReq>,
    send_host_w: MessageWriter<'w, OnSendToHostReq<PlayerData, PlayerInputData>>,
    send_all_w: MessageWriter<'w, OnSendToAllReq<PlayerData, PlayerInputData>>,
    send_client_w: MessageWriter<'w, OnSendToClientReq<PlayerData, PlayerInputData>>,
    kick_w: MessageWriter<'w, OnKickReq>,
    state: ResMut<'w, EasyP2PState<PlayerData>>,
    _marker: std::marker::PhantomData<&'s T>,
}

impl<'w, 's, T: P2PTransport, PlayerData: Default + PartialEq, PlayerInputData>
    EasyP2P<'w, 's, T, PlayerData, PlayerInputData>
where
    PlayerData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    PlayerInputData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
{
    pub fn create_lobby(&mut self) {
        self.create_w.write(OnCreateLobbyReq);
    }
    pub fn join_lobby(&mut self, code: &str) {
        info!("joining lobby... : {}", code);
        self.join_w.write(OnJoinLobbyReq(code.to_string()));
    }
    pub fn exit_lobby(&mut self) {
        info!("exiting lobby...");
        self.exit_w.write(OnExitLobbyReq);
    }
    pub fn send_message_to_host(&mut self, text: String) {
        let msg = P2PData::ClientLobbyChatMessage(text.clone(), NetworkedId::ClientId(0));
        info!("sending message to host (typed JSON): {:?}", &msg);
        self.send_host_w.write(OnSendToHostReq(msg));
    }
    pub fn send_message_all(&mut self, text: String) {
        let msg = P2PData::ClientLobbyChatMessage(text.clone(), NetworkedId::Host);
        info!("sending message to all (typed JSON): {:?}", &msg);
        self.send_all_w.write(OnSendToAllReq(msg));
    }
    pub fn send_message_to_client(&mut self, client_id: ClientId, text: String) {
        let msg = P2PData::ClientLobbyChatMessage(text.clone(), NetworkedId::Host);
        info!(
            "sending message to client {} (typed JSON): {:?}",
            client_id, &msg
        );
        self.send_client_w.write(OnSendToClientReq(client_id, msg));
    }
    pub fn kick(&mut self, client_id: ClientId) {
        self.kick_w.write(OnKickReq(client_id));
    }
    pub fn is_host(&self) -> bool {
        self.state.is_host
    }
    pub fn get_players(&self) -> Vec<PlayerInfo<PlayerData>> {
        self.state.get_players()
    }
    pub fn get_local_player_data(&self) -> PlayerData {
        self.state.local_player_data.clone()
    }
    pub fn set_local_player_data(&mut self, data: PlayerData) {
        self.state.local_player_data = data;
    }
    pub fn get_player_data(&self, id: NetworkedId) -> PlayerData {
        self.state
            .players
            .iter()
            .find(|player| player.id == id)
            .unwrap()
            .data
            .clone()
    }
}

pub struct EasyP2PPlugin<T: P2PTransport, PlayerData, PlayerInputData>(
    std::marker::PhantomData<(T, PlayerData, PlayerInputData)>,
);

impl<T: P2PTransport, PlayerData, PlayerInputData> Default
    for EasyP2PPlugin<T, PlayerData, PlayerInputData>
{
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<T, PlayerData, PlayerInputData> Plugin for EasyP2PPlugin<T, PlayerData, PlayerInputData>
where
    T: P2PTransport,
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
{
    fn build(&self, app: &mut App) {
        app.init_resource::<EasyP2PState<PlayerData>>()
            .init_state::<P2PLobbyState>() // Alternatively we could use .insert_state(AppState::Menu)
            // Request channel messages
            .add_message::<OnCreateLobbyReq>()
            .add_message::<OnJoinLobbyReq>()
            .add_message::<OnSendToHostReq<PlayerData, PlayerInputData>>()
            .add_message::<OnSendToAllReq<PlayerData, PlayerInputData>>()
            .add_message::<OnSendToClientReq<PlayerData, PlayerInputData>>()
            .add_message::<OnExitLobbyReq>()
            .add_message::<OnKickReq>()
            .add_message::<OnLobbyCreated>()
            .add_message::<OnLobbyJoined>()
            .add_message::<OnLobbyEntered>()
            .add_message::<OnClientMessageReceived>()
            .add_message::<OnHostMessageReceived>()
            .add_message::<OnInternalClientData<PlayerData, PlayerInputData>>()
            .add_message::<OnInternalHostData<PlayerData, PlayerInputData>>()
            .add_message::<OnLobbyExit>()
            .add_message::<OnTransportRosterChanged>()
            .add_message::<OnTransportSendToHost>()
            .add_message::<OnTransportSendToAll>()
            .add_message::<OnTransportSendToClient>()
            .add_message::<OnTransportRelayToAllExcept>()
            .add_message::<OnTransportIncomingFromClient>()
            .add_message::<OnTransportIncomingFromHost>()
            .add_message::<OnRosterUpdate<PlayerData>>()
            .add_message::<OnRelayToAllExcept<PlayerData, PlayerInputData>>()
            .add_systems(
                Update,
                ((
                    (state_update_system::<PlayerData>),
                    (
                        on_external_lobby_exit::<PlayerData>,
                        intercept_data_messages::<PlayerData, PlayerInputData>,
                        send_local_data_after_enter::<PlayerData, PlayerInputData>,
                        handle_client_data_update_on_host::<PlayerData, PlayerInputData>,
                        broadcast_roster_on_host::<PlayerData, PlayerInputData>,
                        encode_outgoing::<PlayerData, PlayerInputData>,
                        decode_incoming::<PlayerData, PlayerInputData>,
                    ),
                )
                    .chain(),),
            );
    }
}

// Request channel messages
#[derive(Message, Clone)]
pub struct OnCreateLobbyReq;
#[derive(Message, Clone)]
pub struct OnJoinLobbyReq(pub String);
#[derive(Message, Clone)]
pub struct OnSendToHostReq<PlayerData, PlayerInputData>(pub P2PData<PlayerData, PlayerInputData>);
#[derive(Message, Clone)]
pub struct OnSendToAllReq<PlayerData, PlayerInputData>(pub P2PData<PlayerData, PlayerInputData>);
#[derive(Message, Clone)]
pub struct OnSendToClientReq<PlayerData, PlayerInputData>(
    pub ClientId,
    pub P2PData<PlayerData, PlayerInputData>,
);
#[derive(Message, Clone)]
pub struct OnExitLobbyReq;
#[derive(Message, Clone)]
pub struct OnKickReq(pub ClientId);

// Handle external lobby exit (e.g., WebRTC disconnect) to clear local state
fn on_external_lobby_exit<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
>(
    mut state: ResMut<EasyP2PState<PlayerData>>,
    mut r: MessageReader<OnLobbyExit>,
    mut lobby_state: ResMut<NextState<P2PLobbyState>>,
) {
    let mut any = false;
    for _ in r.read() {
        any = true;
    }
    if !any {
        return;
    }
    state.is_host = false;
    state.lobby_code.clear();
    state.players.clear();
    lobby_state.set(P2PLobbyState::OutOfLobby);
}

fn broadcast_roster_on_host<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
>(
    mut info_r: MessageReader<OnTransportRosterChanged>,
    mut w_send_all: MessageWriter<OnSendToAllReq<PlayerData, PlayerInputData>>,
    state: Res<EasyP2PState<PlayerData>>,
) {
    if !state.is_host {
        return;
    }
    for OnTransportRosterChanged(_) in info_r.read() {
        let players = state.get_players();
        info!("broadcast_roster_on_host: {:?}", players);
        w_send_all.write(OnSendToAllReq(P2PData::HostLobbyInfoUpdate(players)));
    }
}

fn encode_outgoing<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
>(
    mut to_host_r: MessageReader<OnSendToHostReq<PlayerData, PlayerInputData>>,
    mut to_all_r: MessageReader<OnSendToAllReq<PlayerData, PlayerInputData>>,
    mut to_client_r: MessageReader<OnSendToClientReq<PlayerData, PlayerInputData>>,
    mut relay_except_r: MessageReader<OnRelayToAllExcept<PlayerData, PlayerInputData>>,
    mut w_send_host: MessageWriter<OnTransportSendToHost>,
    mut w_send_all: MessageWriter<OnTransportSendToAll>,
    mut w_send_client: MessageWriter<OnTransportSendToClient>,
    mut w_relay_except: MessageWriter<OnTransportRelayToAllExcept>,
) {
    for OnSendToHostReq(data) in to_host_r.read() {
        if let Ok(text) = serde_json::to_string(&data) {
            w_send_host.write(OnTransportSendToHost(text));
        }
    }
    for OnSendToAllReq(data) in to_all_r.read() {
        if let Ok(text) = serde_json::to_string(&data) {
            w_send_all.write(OnTransportSendToAll(text));
        }
    }
    for OnSendToClientReq(cid, data) in to_client_r.read() {
        if let Ok(text) = serde_json::to_string(&data) {
            w_send_client.write(OnTransportSendToClient(*cid, text));
        }
    }
    for OnRelayToAllExcept(sender, data) in relay_except_r.read() {
        if let Ok(text) = serde_json::to_string(&data) {
            w_relay_except.write(OnTransportRelayToAllExcept(*sender, text));
        }
    }
}

fn decode_incoming<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
>(
    mut from_client_r: MessageReader<OnTransportIncomingFromClient>,
    mut from_host_r: MessageReader<OnTransportIncomingFromHost>,
    mut ev_client: MessageWriter<OnInternalClientData<PlayerData, PlayerInputData>>,
    mut ev_host: MessageWriter<OnInternalHostData<PlayerData, PlayerInputData>>,
) {
    for OnTransportIncomingFromClient(cid, text) in from_client_r.read() {
        if let Ok(data) = serde_json::from_str::<P2PData<PlayerData, PlayerInputData>>(text) {
            ev_client.write(OnInternalClientData(*cid, data));
        }
    }
    for OnTransportIncomingFromHost(text) in from_host_r.read() {
        if let Ok(data) = serde_json::from_str::<P2PData<PlayerData, PlayerInputData>>(text) {
            ev_host.write(OnInternalHostData(data));
        }
    }
}

fn state_update_system<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
>(
    mut state: ResMut<EasyP2PState<PlayerData>>,
    mut created_r: MessageReader<OnLobbyCreated>,
    mut joined_r: MessageReader<OnLobbyJoined>,
    mut entered_r: MessageReader<OnLobbyEntered>,
    mut exit_r: MessageReader<OnExitLobbyReq>,
    mut lobby_state: ResMut<NextState<P2PLobbyState>>,
) {
    for OnLobbyCreated(code) in created_r.read() {
        state.is_host = true;
        state.lobby_code = code.clone();
    }
    for OnLobbyJoined(code) in joined_r.read() {
        state.is_host = false;
        state.lobby_code = code.clone();
    }
    for OnLobbyEntered(code) in entered_r.read() {
        state.lobby_code = code.clone();
        lobby_state.set(P2PLobbyState::InLobby);
    }
    for OnExitLobbyReq in exit_r.read() {
        info!("exiting lobby...");
        state.is_host = false;
        state.lobby_code.clear();
        state.players.clear();
        lobby_state.set(P2PLobbyState::OutOfLobby);
    }
}

fn intercept_data_messages<PlayerData: Default + PartialEq, PlayerInputData>(
    mut internal_client_r: MessageReader<OnInternalClientData<PlayerData, PlayerInputData>>,
    mut internal_host_r: MessageReader<OnInternalHostData<PlayerData, PlayerInputData>>,
    mut client_w: MessageWriter<OnClientMessageReceived>,
    mut host_w: MessageWriter<OnHostMessageReceived>,
    mut roster_w: MessageWriter<OnRosterUpdate<PlayerData>>,
    mut relay_w: MessageWriter<OnRelayToAllExcept<PlayerData, PlayerInputData>>,
    mut state: ResMut<EasyP2PState<PlayerData>>,
) where
    PlayerData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    PlayerInputData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
{
    for OnInternalClientData(cid, data) in internal_client_r.read() {
        match data {
            P2PData::ClientLobbyChatMessage(text, _sender) => {
                let _ = client_w.write(OnClientMessageReceived(*cid, text.clone()));
                if state.is_host {
                    relay_w.write(OnRelayToAllExcept(
                        *cid,
                        P2PData::ClientLobbyChatMessage(text.clone(), NetworkedId::ClientId(*cid)),
                    ));
                }
            }
            P2PData::HostLobbyInfoUpdate(_) => {
                // Host should not receive this; ignore
            }
            P2PData::ClientInput(_) => {}
            P2PData::ClientDataUpdate(data) => {
                info!("received client data update: {:?}", data);
                state
                    .players
                    .iter_mut()
                    .find(|player| player.id == NetworkedId::ClientId(*cid))
                    .unwrap()
                    .data = data.clone();
            }
        }
    }
    for OnInternalHostData(data) in internal_host_r.read() {
        match data {
            P2PData::ClientLobbyChatMessage(text, sender) => match sender {
                NetworkedId::Host => {
                    let _ = host_w.write(OnHostMessageReceived(text.clone()));
                }
                NetworkedId::ClientId(cid) => {
                    let _ = client_w.write(OnClientMessageReceived(*cid, text.clone()));
                }
            },
            P2PData::HostLobbyInfoUpdate(players_data) => {
                info!("HostLobbyInfoUpdate: {:?}", players_data);
                state.players = players_data.clone();
                let _ = roster_w.write(OnRosterUpdate(players_data.clone()));
            }
            P2PData::ClientInput(_) => {}
            P2PData::ClientDataUpdate(_) => {}
        }
    }
}

fn send_local_data_after_enter<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
>(
    mut entered_r: MessageReader<OnLobbyEntered>,
    state: Res<EasyP2PState<PlayerData>>,
    mut w_send_host: MessageWriter<OnSendToHostReq<PlayerData, PlayerInputData>>,
) {
    for OnLobbyEntered(_code) in entered_r.read() {
        if state.is_host {
            continue;
        }
        info!(
            "sending local data to host after enter: {:?}",
            state.local_player_data
        );

        w_send_host.write(OnSendToHostReq(P2PData::ClientDataUpdate(
            state.local_player_data.clone(),
        )));
    }
}

fn handle_client_data_update_on_host<
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
>(
    mut internal_client_r: MessageReader<OnInternalClientData<PlayerData, PlayerInputData>>,
    mut state: ResMut<EasyP2PState<PlayerData>>,
    mut w_send_all: MessageWriter<OnSendToAllReq<PlayerData, PlayerInputData>>,
) {
    if !state.is_host {
        return;
    }
    for OnInternalClientData(cid, data) in internal_client_r.read() {
        if let P2PData::ClientDataUpdate(client_info) = data {
            info!("received client data update: {:?}", client_info);
            let client_id = *cid;
            let mut found = false;
            for entry in state.players.iter_mut() {
                if entry.id == NetworkedId::ClientId(client_id) {
                    entry.data = client_info.clone();
                    found = true;
                    break;
                }
            }
            if !found {
                state.players.push(PlayerInfo::<PlayerData> {
                    id: NetworkedId::ClientId(client_id),
                    data: client_info.clone(),
                });
            }

            let payload = state.get_players();
            info!("sending local data to all after enter: {:?}", payload);
            w_send_all.write(OnSendToAllReq(P2PData::HostLobbyInfoUpdate(payload)));
        }
    }
}
