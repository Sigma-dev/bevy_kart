use bevy::prelude::*;
use bevy_ensemble::prelude::*;
use bevy_ensemble_webrtc::{
    JoinWebrtcLobby, JoinWebrtcLobbyByCode, LobbyWebrtcCode, RefreshLobbyList,
};
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;

use crate::{AppState, GameStateChanged, LobbyState, LocalPlayerData};

pub struct LobbyLifecyclePlugin;

impl Plugin for LobbyLifecyclePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, apply_session_params)
            .add_systems(
                Update,
                (
                    on_lobby_ready,
                    cleanup_on_lobby_gone,
                    receive_game_state_changed,
                    autostart_join,
                    autostart_race,
                ),
            );
    }
}

#[cfg(target_arch = "wasm32")]
fn get_url() -> Option<String> {
    web_sys::window()?.location().href().ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn get_url() -> Option<String> {
    None
}

fn current_base_url() -> Option<String> {
    let source = get_url()?;
    let no_hash = source.split('#').next().unwrap_or(source.as_str());
    let base = no_hash.split('?').next().unwrap_or(no_hash);
    Some(base.trim_end_matches('/').to_string())
}

fn extract_query_param(target: &str) -> Option<String> {
    let href = get_url()?;
    let no_hash = href.split('#').next().unwrap_or(href.as_str());
    let query = no_hash.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        let key = it.next()?;
        if key == target {
            let val = it.next().unwrap_or("");
            return Some(val.to_string());
        }
    }
    None
}

/// How this copy of the game starts, with nobody touching the menu.
///
/// On the web it comes from the page URL, on native from the environment, and
/// the two spell the same choices. Absent, which is every ordinary launch,
/// nothing here does anything. A development affordance, but the one that
/// makes local multiplayer testable: `scripts/local-session.sh` starts a
/// signalling server and several copies of the game, and none of them has
/// anybody to press keys at it.
///
/// | URL | environment | effect |
/// |---|---|---|
/// | `?room=CODE` | `KART_ROOM=CODE` | join that lobby |
/// | `?host=1` | `KART_AUTOSTART=host` | host a lobby |
/// | `?join=1` | `KART_AUTOSTART=join` | join the first lobby the signalling server lists |
/// | `?autostart=N` | `KART_AUTOSTART_PLAYERS=N` | as host, start the race once `N` players are in |
/// | `?name=NAME` | `KART_NAME=NAME` | the name this peer plays under |
/// | `?autodrive=1` | `KART_AUTODRIVE=1` | hold the throttle and weave |
/// | `?perf=1` | `KART_PERF=1` | log the frame-cost readout at `warn` |
///
/// `KART_AUTOSTART=host` on its own starts the race at two players, which is
/// what a script wants; `?host=1` on its own does not, because a person hosting
/// from a link wants to press start themselves.
///
/// `perf` logs at `warn` because that is the only level a release build keeps:
/// bevy_ticked turns on `release_max_level_warn` for `log` and `tracing`,
/// which strips `info!` from every crate in the binary.
#[derive(Resource, Default, Debug)]
pub struct SessionParams {
    pub room: Option<String>,
    pub host: bool,
    pub join_first: bool,
    pub autostart: Option<usize>,
    pub name: Option<String>,
    pub autodrive: bool,
    pub perf: bool,
}

impl SessionParams {
    fn read() -> Self {
        // The URL wins where both are set; on the web there is no environment
        // to read, and on native there is no URL.
        let get = |url_key: &str, env_key: &str| -> Option<String> {
            extract_query_param(url_key)
                .or_else(|| std::env::var(env_key).ok())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let flag = |url_key: &str, env_key: &str| get(url_key, env_key).is_some_and(|v| v == "1");
        let autostart_env = std::env::var("KART_AUTOSTART").ok();
        let host = flag("host", "") || autostart_env.as_deref() == Some("host");
        let join_first = flag("join", "") || autostart_env.as_deref() == Some("join");
        let autostart = get("autostart", "KART_AUTOSTART_PLAYERS")
            .and_then(|v| v.parse().ok())
            .or(if autostart_env.as_deref() == Some("host") { Some(2) } else { None });
        if let Some(other) = autostart_env.filter(|v| v != "host" && v != "join") {
            warn!("KART_AUTOSTART={other} is not `host` or `join`; ignoring");
        }
        Self {
            room: get("room", "KART_ROOM"),
            host,
            join_first,
            autostart,
            name: get("name", "KART_NAME"),
            autodrive: flag("autodrive", "KART_AUTODRIVE"),
            perf: flag("perf", "KART_PERF"),
        }
    }
}

fn apply_session_params(
    mut commands: Commands,
    mut local_data: ResMut<LocalPlayerData>,
    mut join_by_code: MessageWriter<JoinWebrtcLobbyByCode>,
    mut start_hosting: MessageWriter<StartHosting>,
) {
    let params = SessionParams::read();
    if params.host || params.join_first || params.room.is_some() {
        info!("session from parameters: {params:?}");
    }
    if let Some(name) = &params.name {
        local_data.0.name = name.clone();
    }
    if let Some(room) = &params.room {
        join_by_code.write(JoinWebrtcLobbyByCode(room.clone()));
    }
    if params.host {
        start_hosting.write(StartHosting);
    }
    commands.insert_resource(params);
}

/// `join`: ask the signalling server for lobbies until there is one, then join it.
///
/// Retrying rather than joining once: a client started alongside its host has
/// nothing to join for the first second or two, and the script has no way to
/// know when that stops being true. Throttled, and the join is throttled with
/// the refresh rather than fired the instant a listing appears, because the
/// backend spawns its pending lobby a frame or two after accepting the request
/// and an eager retry asks twice.
fn autostart_join(
    params: Res<SessionParams>,
    time: Res<Time>,
    lobbies: Query<(), Or<(With<Lobby>, With<PendingLobby>)>>,
    lobby_list: Option<Res<PublicLobbies>>,
    mut refresh: MessageWriter<RefreshLobbyList>,
    mut join: MessageWriter<JoinWebrtcLobby>,
    mut since_asked: Local<f32>,
) {
    if !params.join_first || !lobbies.is_empty() {
        return;
    }
    *since_asked += time.delta_secs();
    if *since_asked < 1.0 {
        return;
    }
    *since_asked = 0.0;
    match lobby_list.as_ref().and_then(|list| list.0.first()) {
        Some(first) => {
            info!("autostart: joining lobby {}", first.code);
            join.write(JoinWebrtcLobby(first.lobby_id));
        }
        None => {
            refresh.write(RefreshLobbyList);
        }
    }
}

/// `autostart`: what the lobby's start button does, once that many players are in.
fn autostart_race(
    params: Res<SessionParams>,
    mut fired: Local<bool>,
    server_player: Option<Res<LocalServerPlayer>>,
    app_state: Res<State<AppState>>,
    participants: Query<(), With<LobbyParticipant>>,
    lobbies: Query<Entity, (With<Lobby>, With<Host>)>,
    mut next_state: ResMut<NextState<AppState>>,
    mut commands: Commands,
) {
    let Some(wanted) = params.autostart else { return };
    if *fired || server_player.is_none() || *app_state.get() != AppState::OutOfGame {
        return;
    }
    if participants.iter().count() < wanted {
        return;
    }
    let Ok(lobby) = lobbies.single() else { return };
    *fired = true;
    info!("autostart: {} players in, starting the race", wanted);
    next_state.set(AppState::Game);
    let msg = GameStateChanged(AppState::Game);
    commands
        .entity(lobby)
        .trigger(move |e| BroadcastLobbyMessage::new(e, msg));
}

fn on_lobby_ready(
    mut commands: Commands,
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    server_player: Option<Res<LocalServerPlayer>>,
    client_player: Option<Res<LocalClientPlayer>>,
    host_lobbies: Query<(Entity, &LobbyWebrtcCode), (With<Lobby>, With<Host>)>,
    client_lobbies: Query<Entity, (With<Lobby>, Without<Host>)>,
    mut lobby_state: ResMut<NextState<LobbyState>>,
    local_data: Res<LocalPlayerData>,
    params: Option<Res<SessionParams>>,
) {
    let Some(local_player) = local_player else {
        return;
    };
    if let Ok((lobby_entity, code)) = host_lobbies.single() {
        if server_player.is_none() {
            info!("Hosting room: {}", code.0);
            if params.is_some_and(|p| p.perf) {
                warn!("PERF-ROOM {}", code.0);
            }
            if let Some(base) = current_base_url() {
                info!("Share link: {}?room={}", base, code.0);
            }
            commands.insert_resource(LocalServerPlayer(local_player.0));
            commands.remove_resource::<TicksPaused>();
            lobby_state.set(LobbyState::InLobby);
            let data = local_data.0.clone();
            commands
                .entity(lobby_entity)
                .trigger(move |entity| SetPlayerData::new(entity, data));
        }
    }
    if let Ok(lobby_entity) = client_lobbies.single() {
        if client_player.is_none() {
            commands.insert_resource(LocalClientPlayer(local_player.0));
            lobby_state.set(LobbyState::InLobby);
            let data = local_data.0.clone();
            commands
                .entity(lobby_entity)
                .trigger(move |entity| SetPlayerData::new(entity, data));
        }
    }
}

fn cleanup_on_lobby_gone(
    mut commands: Commands,
    mut removed_lobbies: RemovedComponents<Lobby>,
    game_entities: Query<Entity, With<TickTrackedEntity>>,
    mut lobby_state: ResMut<NextState<LobbyState>>,
    mut app_state: ResMut<NextState<AppState>>,
) {
    if removed_lobbies.read().next().is_none() {
        return;
    }
    for entity in game_entities.iter() {
        commands.entity(entity).try_despawn();
    }
    commands.remove_resource::<LocalMultiplayerPlayerId>();
    commands.remove_resource::<LocalServerPlayer>();
    commands.remove_resource::<LocalClientPlayer>();
    commands.insert_resource(CurrentTick(0));
    commands.insert_resource(TicksPaused);
    lobby_state.set(LobbyState::OutOfLobby);
    app_state.set(AppState::OutOfGame);
}

fn receive_game_state_changed(
    server_player: Option<Res<LocalServerPlayer>>,
    mut reader: MessageReader<ReceivedEnsembleMessage<GameStateChanged>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for msg in reader.read() {
        if server_player.is_some() {
            continue;
        }
        next_state.set(msg.message.0.clone());
    }
}
