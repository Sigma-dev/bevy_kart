use bevy::prelude::*;
use bevy_ensemble::prelude::*;
use bevy_ensemble_webrtc::{JoinWebrtcLobbyByCode, LobbyWebrtcCode};
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;

use crate::{AppState, GameStateChanged, LobbyState, LocalPlayerData};

pub struct LobbyLifecyclePlugin;

impl Plugin for LobbyLifecyclePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, auto_join_from_url)
            .add_systems(
                Update,
                (on_lobby_ready, cleanup_on_lobby_gone, receive_game_state_changed),
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

fn auto_join_from_url(mut join_writer: MessageWriter<JoinWebrtcLobbyByCode>) {
    if let Some(room) = extract_query_param("room") {
        info!("room code in url: {}", room);
        if !room.trim().is_empty() {
            join_writer.write(JoinWebrtcLobbyByCode(room));
        }
    }
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
) {
    let Some(local_player) = local_player else {
        return;
    };
    if let Ok((lobby_entity, code)) = host_lobbies.single() {
        if server_player.is_none() {
            info!("Hosting room: {}", code.0);
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
