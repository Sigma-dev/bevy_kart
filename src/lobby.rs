use bevy::prelude::*;
use bevy_ensemble::prelude::*;
use bevy_ensemble::{EnsembleSet, HostChanged};
use bevy_ensemble_webrtc::{
    JoinWebrtcLobby, JoinWebrtcLobbyByCode, LobbyWebrtcCode, RefreshLobbyList,
};
use bevy_ticked_networking::prelude::*;
use bevy_ticked_networking_ensemble::{HandshakeTimedOut, RegistryMismatch};

use crate::{AppPlayerData, AppState, GameStateChanged, LobbyState};

/// The game's side of a session's lifecycle.
///
/// Who is host and who is client is not decided here. `TickedEnsembleSessionPlugin`
/// adopts the role the moment the local uuid exists, releases it when the lobby
/// goes, and upstream's `reset_on_leave` despawns the tracked world, zeroes the
/// tick and clears the input queue on the way out. What is left is what only the
/// game knows: its menu state, how a copy started with nobody at the keyboard
/// behaves, what a race in progress becomes when the lobby changes host, and
/// what to do when the handshake finds the other peer was built from a
/// different commit. The player data is the stack's too:
/// `PlayerDataPlugin::persisted` publishes it into every lobby.
pub struct LobbyLifecyclePlugin;

impl Plugin for LobbyLifecyclePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, apply_session_params)
            // Where the bridge re-enters the roles, so the race is called off in
            // this frame's state transition, before anything in `Update` sees the
            // new host's world with the old race's state. See `follow_host_change`.
            .add_systems(
                PreUpdate,
                follow_host_change.after(EnsembleSet::ReceivePackets),
            )
            .add_systems(
                Update,
                (
                    enter_lobby,
                    leave_on_registry_mismatch,
                    leave_on_handshake_timeout,
                    exit_lobby_when_session_ends,
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
/// | `?map=SLUG` | `KART_MAP=SLUG` | race that map, built-in or saved |
/// | `?editor=1` | `KART_EDITOR=1` | open the track editor |
/// | `?perf=1` | `KART_PERF=1` | log the frame-cost readout |
///
/// `KART_AUTOSTART=host` on its own starts the race at two players, which is
/// what a script wants; `?host=1` on its own does not, because a person hosting
/// from a link wants to press start themselves.
///
/// `perf` is what turns the readout on at all, and it logs at `warn` because that
/// is the only level a release build keeps: bevy_ticked turns on
/// `release_max_level_warn` for `log` and `tracing`, which strips `info!` from
/// every crate in the binary. Without the flag the numbers still reach the F3
/// overlay, they just do not go to the log -- a line every two seconds buries the
/// ones that are trying to explain a failure.
#[derive(Resource, Default, Debug)]
pub struct SessionParams {
    pub room: Option<String>,
    pub host: bool,
    pub join_first: bool,
    pub autostart: Option<usize>,
    pub name: Option<String>,
    pub autodrive: bool,
    pub perf: bool,
    /// Which built-in map to race, by slug. An unknown one warns and falls back,
    /// because a typo in a launch flag should not be the end of the session.
    pub map: Option<String>,
    /// Open the track editor instead of the menu.
    pub editor: bool,
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
            .or(if autostart_env.as_deref() == Some("host") {
                Some(2)
            } else {
                None
            });
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
            map: get("map", "KART_MAP"),
            editor: flag("editor", "KART_EDITOR"),
        }
    }
}

fn apply_session_params(
    mut commands: Commands,
    mut editor_state: ResMut<NextState<crate::EditorState>>,
    mut local_data: ResMut<LocalPlayerData<AppPlayerData>>,
    mut selected_map: ResMut<crate::track::SelectedMap>,
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
    if let Some(slug) = &params.map {
        // A built-in by slug, or one of the player's own by id. Saved maps too,
        // because "it goes wrong on the track I made" is exactly the report that
        // needs reproducing from a command line.
        let found =
            crate::track::map::by_slug(slug).or_else(|| crate::track::map::store::load(slug).ok());
        match found {
            Some(map) => {
                info!("racing `{}` ({slug}), from the launch parameters", map.name);
                selected_map.0 = map;
            }
            // Naming a map that does not exist is a typo in a script, not a
            // reason to refuse to start.
            None => warn!("no map called `{slug}`; keeping the default"),
        }
    }
    if params.editor {
        editor_state.set(crate::EditorState::Open);
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
    selected: Res<crate::track::SelectedMap>,
    mut fired: Local<bool>,
    mut host_changes: MessageReader<HostChanged>,
    server_player: Option<Res<LocalServerPlayer>>,
    app_state: Res<State<AppState>>,
    participants: Query<(), With<LobbyParticipant>>,
    lobbies: Query<Entity, (With<Lobby>, With<Host>)>,
    mut next_state: ResMut<NextState<AppState>>,
    mut commands: Commands,
) {
    // A new host starts an empty world, and the race it would start is a new one.
    if host_changes.read().count() > 0 {
        *fired = false;
    }
    let Some(wanted) = params.autostart else {
        return;
    };
    if *fired || server_player.is_none() || *app_state.get() != AppState::OutOfGame {
        return;
    }
    if participants.iter().count() < wanted {
        return;
    }
    let Ok(lobby) = lobbies.single() else { return };
    *fired = true;
    info!("autostart: {} players in, starting the race", wanted);
    let map = selected.0.clone();
    let msg = crate::map_sync::StartRace(map.clone());
    commands
        .entity(lobby)
        .trigger(move |e| BroadcastLobbyMessage::new(e, msg));
    crate::map_sync::begin_race(&mut commands, &mut next_state, map);
}

/// A lobby this game has already switched its screen to, so a promotion is
/// acted on once. A lobby entity lives for as long as the lobby does, through
/// every client that comes and goes and through a change of host.
#[derive(Component)]
struct EnteredLobby;

/// Once a lobby is promoted: show the lobby screen.
///
/// Keyed on `Lobby` and not on the role, deliberately. The role arrives earlier,
/// on `PendingLobby`, before any data channel exists, and that is right for the
/// simulation, which must not build a solo world in that window. The menu wants
/// the promoted lobby: the lobby screen reads the code and the roster off it.
/// This player's data goes in on its own, from `PlayerDataPlugin::persisted`,
/// which publishes it into every lobby and again whenever it changes.
fn enter_lobby(
    mut commands: Commands,
    local_player: Option<Res<LocalMultiplayerPlayerId>>,
    lobbies: Query<
        (Entity, Has<Host>, Option<&LobbyWebrtcCode>),
        (With<Lobby>, Without<EnteredLobby>),
    >,
    mut lobby_state: ResMut<NextState<LobbyState>>,
    params: Option<Res<SessionParams>>,
) {
    if local_player.is_none() {
        return;
    }
    for (lobby, is_host, code) in &lobbies {
        if let (true, Some(code)) = (is_host, code) {
            info!("Hosting room: {}", code.0);
            if params.as_ref().is_some_and(|p| p.perf) {
                warn!("PERF-ROOM {}", code.0);
            }
            if let Some(base) = current_base_url() {
                info!("Share link: {}?room={}", base, code.0);
            }
        }
        lobby_state.set(LobbyState::InLobby);
        commands.entity(lobby).insert(EnteredLobby);
    }
}

/// When the role goes, so does the lobby screen.
///
/// The role rather than the lobby entity, because the role is released in every
/// way a session can end. `TickedEnsembleSessionPlugin` drops it when no lobby
/// is left, promoted or not, which covers a refused join, where
/// `RemovedComponents<Lobby>` never fires because the entity never carried
/// `Lobby`. The handshake drops it on a registry mismatch while the lobby is
/// still standing. To the menu both are the same event.
///
/// A change of host is not an end. The bridge swaps the roles in one command,
/// so there is no frame without one and this never sees the lobby go: the
/// survivors stay in it, and `follow_host_change` takes them back to its screen.
/// A lobby that is never given a new host does end, with `LobbyLeft { HostGone }`
/// and the role with it, and that comes through here like any other end.
///
/// Nothing here touches the simulation. The stack's leave door runs on the
/// same removal: it despawns every tracked entity, zeroes the tick, clears the
/// input queue and un-pauses. The un-pause is why ticks run in the menu after a
/// session, as they do before the first one.
fn exit_lobby_when_session_ends(
    mut had_role: Local<bool>,
    server_player: Option<Res<LocalServerPlayer>>,
    client_player: Option<Res<LocalClientPlayer>>,
    mut lobby_state: ResMut<NextState<LobbyState>>,
    mut app_state: ResMut<NextState<AppState>>,
) {
    let has_role = server_player.is_some() || client_player.is_some();
    let ended = *had_role && !has_role;
    *had_role = has_role;
    if !ended {
        return;
    }
    lobby_state.set(LobbyState::OutOfLobby);
    app_state.set(AppState::OutOfGame);
}

/// The lobby changed host: the race, if there was one, is over, and everybody
/// still in the lobby goes back to its screen.
///
/// The snapshot model cannot carry a race across a host change -- the world was
/// the old host's -- so the bridge has already ended the ticked session and
/// started a fresh one under the new host, in the same lobby. What is left is
/// what is not a ticked resource: the screen, the finishing order that labels
/// the lobby cars, and the host-only "race is over" timer. `LobbyState` stays
/// `InLobby`, because the lobby did.
///
/// A promoted peer also announces the map it last heard about, so the new
/// lobby starts on the track everybody was shown: `SelectedMap` is marked
/// changed and `map_sync` sends it. Followers are seated on the new host as new
/// clients and hear it from there.
///
/// In `PreUpdate`, after the transport, beside the bridge's own reaction. The
/// state is then out of the race by this frame's transition, so nothing in
/// `Update` -- `map_sync` catching a reseated follower up on "the race that is
/// running", in particular -- acts on a race that is gone.
fn follow_host_change(
    mut commands: Commands,
    mut changes: MessageReader<HostChanged>,
    mut finish_times: ResMut<crate::FinishTimes>,
    mut selected: ResMut<crate::track::SelectedMap>,
    mut app_state: ResMut<NextState<AppState>>,
) {
    let Some(change) = changes.read().last() else {
        return;
    };
    info!(
        "the lobby has a new host ({:#x}); back to the lobby",
        change.new
    );
    app_state.set(AppState::OutOfGame);
    finish_times.times.clear();
    commands.remove_resource::<crate::track::RaceEnded>();
    if change.promoted {
        selected.set_changed();
    }
}

/// A peer built from a different commit cannot be played with, so leave.
///
/// The handshake has already said which registration differs, at `error`, and
/// dropped the role. It leaves the lobby standing for the game to decide, and
/// this game has no screen for "in a lobby with nobody to play with", so it
/// does what the leave button does. Going through the lobby entity rather than the state is what makes the
/// signalling server, the peer connections and the mismatch itself all clean up
/// behind it.
fn leave_on_registry_mismatch(
    mut commands: Commands,
    mismatch: Option<Res<RegistryMismatch>>,
    lobbies: Query<Entity, Or<(With<Lobby>, With<PendingLobby>)>>,
) {
    let Some(mismatch) = mismatch else { return };
    if !mismatch.is_added() {
        return;
    }
    warn!(
        "leaving the lobby: peer {:#x} is not running this build ({})",
        mismatch.peer, mismatch.difference
    );
    for lobby in &lobbies {
        commands.entity(lobby).despawn();
    }
}

/// A host that never announced its registries is on a build without the
/// handshake, or behind a link that never delivered it. Either way there is
/// nothing to wait for; the same leave as a mismatch.
fn leave_on_handshake_timeout(
    mut commands: Commands,
    timed_out: Option<Res<HandshakeTimedOut>>,
    lobbies: Query<Entity, Or<(With<Lobby>, With<PendingLobby>)>>,
) {
    let Some(timed_out) = timed_out else { return };
    if !timed_out.is_added() {
        return;
    }
    warn!(
        "leaving the lobby: the host never announced its registries in {:?}",
        timed_out.waited
    );
    for lobby in &lobbies {
        commands.entity(lobby).despawn();
    }
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
        // Starting a race goes through `map_sync::StartRace` now, because the map
        // has to arrive in the same packet. Seeing `Game` here means the sender
        // is on a build from before that -- which the registry handshake should
        // already have refused, so it is worth saying out loud.
        if msg.message.0 == AppState::Game {
            warn!("a peer asked to start a race the old way; it is running a different build");
        }
        next_state.set(msg.message.0.clone());
    }
}

#[cfg(test)]
mod tests {
    //! The menu's side of a session, over the real lobby crate and the loopback
    //! backend: a host, two clients, and the bridge doing what it does in the
    //! game. No window and no assets, so no screens -- what is checked is the
    //! state the screens are derived from.
    use std::time::Duration;

    use bevy::state::app::StatesPlugin;
    use bevy_ensemble::CloseLobby;
    use bevy_ticked_testing::prelude::*;

    use super::*;
    use crate::{EditorState, FinishTimes, PlayerInput};

    const SETTLE: usize = 400;

    const MIGRATION: HostMigratable = HostMigratable {
        successor_within: Duration::from_secs(4),
        reach_within: Duration::from_secs(20),
    };

    /// What `main` adds for a session, less everything that draws.
    fn install(app: &mut App) {
        if !app.is_plugin_added::<StatesPlugin>() {
            app.add_plugins(StatesPlugin);
        }
        app.add_plugins((
            LobbyBroadcastPlugin,
            PlayerDataPlugin::<AppPlayerData>::default().persisted("bevy_kart.test-profile"),
            LobbyLifecyclePlugin,
            crate::map_sync::MapSyncPlugin,
        ))
        // The WebRTC backend's own requests, which the autostart systems write.
        .add_message::<JoinWebrtcLobbyByCode>()
        .add_message::<JoinWebrtcLobby>()
        .add_message::<RefreshLobbyList>()
        .init_state::<AppState>()
        .init_state::<LobbyState>()
        .init_state::<EditorState>()
        .init_resource::<crate::track::SelectedMap>()
        .insert_resource(FinishTimes {
            times: Default::default(),
        });
        crate::register_broadcast_messages(app);
    }

    fn lobby_state(net: &TickedNetwork, peer: PeerId) -> LobbyState {
        net.app(peer)
            .world()
            .resource::<State<LobbyState>>()
            .get()
            .clone()
    }

    fn app_state(net: &TickedNetwork, peer: PeerId) -> AppState {
        net.app(peer)
            .world()
            .resource::<State<AppState>>()
            .get()
            .clone()
    }

    /// A host and two clients, settled, every one of them on the lobby screen and
    /// then in a race, with a finishing order from an earlier one.
    fn racing_session() -> (TickedNetwork, PeerId, PeerId, PeerId) {
        let mut net = TickedNetwork::client_server::<PlayerInput>(2, install)
            .with_link(Link::cable())
            .with_seed(7)
            .with_host_migration(MIGRATION);
        assert!(net.settle(SETTLE), "the session did not settle");
        let host = net.host();
        let clients = net.clients();
        let (a, b) = (clients[0], clients[1]);
        net.run(10);
        for peer in [host, a, b] {
            assert_eq!(lobby_state(&net, peer), LobbyState::InLobby);
            let world = net.world_mut(peer);
            world
                .resource_mut::<NextState<AppState>>()
                .set(AppState::Game);
            world
                .resource_mut::<FinishTimes>()
                .times
                .insert(1, Tick(10));
        }
        net.run(2);
        for peer in [host, a, b] {
            assert_eq!(app_state(&net, peer), AppState::Game);
        }
        (net, host, a, b)
    }

    /// The host leaves mid-race -- the leave button, which despawns its lobby --
    /// and the lobby is handed on. Nobody who stayed is thrown out of it: both
    /// survivors are back on the lobby screen, one hosting and one a client of it,
    /// with the old race's finishing order gone, and the client is given the new
    /// host's world.
    #[test]
    fn the_players_who_stay_keep_their_lobby_when_the_host_leaves_mid_race() {
        let (mut net, host, a, b) = racing_session();

        net.lose_host(HostDeparture::Quits);
        net.run(5);
        assert_eq!(
            lobby_state(&net, b),
            LobbyState::InLobby,
            "waiting for a new host is not leaving the lobby"
        );
        net.name_host(a);

        let started = net.run_until(SETTLE, |net| {
            role(net.app(a)) == Role::Host
                && role(net.app(b)) == Role::Client
                && applied_tick(net.app(b)).is_some()
        });
        assert!(started, "a hosts and b applies its snapshots");
        for peer in [a, b] {
            assert_eq!(lobby_state(&net, peer), LobbyState::InLobby, "{peer:?}");
            assert_eq!(app_state(&net, peer), AppState::OutOfGame, "{peer:?}");
            assert!(
                net.app(peer)
                    .world()
                    .resource::<FinishTimes>()
                    .times
                    .is_empty(),
                "{peer:?} forgot the old race's finishing order"
            );
        }
        assert_eq!(
            lobby_state(&net, host),
            LobbyState::OutOfLobby,
            "the host that left is out"
        );
    }

    /// A host that means the game is over closes the lobby, and it ends for
    /// everyone rather than being handed on.
    #[test]
    fn closing_the_lobby_ends_it_for_everyone() {
        let (mut net, host, a, b) = racing_session();

        net.world_mut(host).write_message(CloseLobby);
        let ended = net.run_until(SETTLE, |net| {
            [host, a, b]
                .into_iter()
                .all(|peer| lobby_state(net, peer) == LobbyState::OutOfLobby)
        });
        assert!(ended, "every peer left the lobby");
        for peer in [host, a, b] {
            assert_eq!(app_state(&net, peer), AppState::OutOfGame, "{peer:?}");
            assert_eq!(role(net.app(peer)), Role::Solo, "{peer:?} holds no role");
        }
    }
}
