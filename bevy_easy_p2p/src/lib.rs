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
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum NetworkedId {
    Host,
    ClientId(u64),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayerInfo<PlayerData> {
    pub id: NetworkedId,
    pub data: PlayerData,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum P2PData<PlayerData, PlayerInputData> {
    ClientLobbyChatMessage(String, NetworkedId),
    ClientInput(PlayerInputData),
    ClientDataUpdate(PlayerInfo<PlayerData>),
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
pub struct OnRosterUpdate(pub Vec<String>);
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
pub struct OnLobbyInfoUpdate(pub Vec<String>);
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
pub struct EasyP2PState {
    pub is_host: bool,
    pub lobby_code: String,
    pub players: Vec<String>,
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
    PlayerData: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    PlayerInputData: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
> {
    create_w: MessageWriter<'w, OnCreateLobbyReq>,
    join_w: MessageWriter<'w, OnJoinLobbyReq>,
    exit_w: MessageWriter<'w, OnExitLobbyReq>,
    send_host_w: MessageWriter<'w, OnSendToHostReq<PlayerData, PlayerInputData>>,
    send_all_w: MessageWriter<'w, OnSendToAllReq<PlayerData, PlayerInputData>>,
    send_client_w: MessageWriter<'w, OnSendToClientReq<PlayerData, PlayerInputData>>,
    kick_w: MessageWriter<'w, OnKickReq>,
    state: ResMut<'w, EasyP2PState>,
    _marker: std::marker::PhantomData<&'s T>,
}

impl<'w, 's, T: P2PTransport, PlayerData, PlayerInputData>
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
    pub fn get_players(&self) -> Vec<(String, bool)> {
        let mut players = vec![("Host".to_string(), true)];
        players.extend(
            self.state
                .players
                .clone()
                .iter()
                .map(|p| (p.clone(), false)),
        );
        players
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
    PlayerData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    PlayerInputData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
{
    fn build(&self, app: &mut App) {
        app.init_resource::<EasyP2PState>()
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
            .add_message::<OnLobbyInfoUpdate>()
            .add_message::<OnRosterUpdate>()
            .add_message::<OnRelayToAllExcept<PlayerData, PlayerInputData>>()
            .add_systems(
                Update,
                (
                    state_update_system,
                    on_external_lobby_exit,
                    intercept_data_messages::<PlayerData, PlayerInputData>,
                ),
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
fn on_external_lobby_exit(
    mut state: ResMut<EasyP2PState>,
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

fn state_update_system(
    mut state: ResMut<EasyP2PState>,
    mut created_r: MessageReader<OnLobbyCreated>,
    mut joined_r: MessageReader<OnLobbyJoined>,
    mut entered_r: MessageReader<OnLobbyEntered>,
    mut info_r: MessageReader<OnLobbyInfoUpdate>,
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
    for OnLobbyInfoUpdate(list) in info_r.read() {
        state.players = list.clone();
    }
    for OnExitLobbyReq in exit_r.read() {
        info!("exiting lobby...");
        state.is_host = false;
        state.lobby_code.clear();
        state.players.clear();
        lobby_state.set(P2PLobbyState::OutOfLobby);
    }
}

fn intercept_data_messages<PlayerData, PlayerInputData>(
    mut internal_client_r: MessageReader<OnInternalClientData<PlayerData, PlayerInputData>>,
    mut internal_host_r: MessageReader<OnInternalHostData<PlayerData, PlayerInputData>>,
    mut client_w: MessageWriter<OnClientMessageReceived>,
    mut host_w: MessageWriter<OnHostMessageReceived>,
    mut roster_w: MessageWriter<OnRosterUpdate>,
    mut relay_w: MessageWriter<OnRelayToAllExcept<PlayerData, PlayerInputData>>,
    state: Res<EasyP2PState>,
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
            P2PData::ClientDataUpdate(_) => {}
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
            P2PData::HostLobbyInfoUpdate(players_any) => {
                let mut list: Vec<String> = Vec::new();
                for v in players_any {
                    match v.id {
                        NetworkedId::Host => list.push("Host".to_string()),
                        NetworkedId::ClientId(cid) => list.push(cid.to_string()),
                    }
                }
                if !list.is_empty() {
                    let _ = roster_w.write(OnRosterUpdate(list));
                }
            }
            P2PData::ClientInput(_) => {}
            P2PData::ClientDataUpdate(_) => {}
        }
    }
}
