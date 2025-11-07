use bevy::input::{ButtonInput, keyboard::Key};
use bevy::prelude::*;
use bevy_easy_p2p::prelude::*;
use bevy_easy_p2p::{NetworkedEventsExt, NetworkedStatesExt};
use bevy_firestore_p2p::{FirestoreP2PPlugin, FirestoreWebRtcTransport};
use serde::{Deserialize, Serialize};
use web_sys::window;

type DemoTransport = FirestoreWebRtcTransport;
type DemoInput = ();
type DemoSpawn = ();

#[derive(States, Default, Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
enum DemoSyncedState {
    #[default]
    Waiting,
    Ready,
}

#[derive(Message, Clone, Debug, Serialize, Deserialize)]
struct DemoSyncedEvent {
    text: String,
}

#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct DemoPlayerData {
    name: String,
}

impl DemoPlayerData {
    fn display_name(&self) -> &str {
        if self.name.trim().is_empty() {
            "(unnamed)"
        } else {
            &self.name
        }
    }
}

#[derive(Resource, Default)]
struct PingDisplay(Option<u128>);

#[derive(Resource, Default)]
struct StatusLog(Vec<String>);

impl StatusLog {
    fn push(&mut self, message: impl Into<String>) {
        let message = message.into();
        if message.is_empty() {
            return;
        }
        self.0.push(message);
        const MAX_LINES: usize = 6;
        if self.0.len() > MAX_LINES {
            let overflow = self.0.len() - MAX_LINES;
            self.0.drain(0..overflow);
        }
    }
}

#[derive(Resource, Default)]
struct EventLog(Vec<String>);

impl EventLog {
    fn push(&mut self, message: impl Into<String>) {
        let message = message.into();
        if message.is_empty() {
            return;
        }
        self.0.push(message);
        const MAX_LINES: usize = 4;
        if self.0.len() > MAX_LINES {
            let overflow = self.0.len() - MAX_LINES;
            self.0.drain(0..overflow);
        }
    }
}

#[derive(Component)]
struct UIRoot;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((
            EasyP2PPlugin::<DemoTransport, DemoPlayerData, DemoInput, DemoSpawn>::default(),
            FirestoreP2PPlugin::<DemoPlayerData, DemoInput, DemoSpawn>::default(),
        ))
        .init_state::<DemoSyncedState>()
        .init_networked_state::<DemoSyncedState>()
        .init_networked_event::<DemoSyncedEvent>()
        .init_resource::<PingDisplay>()
        .insert_resource(StatusLog::default())
        .insert_resource(EventLog::default())
        .add_systems(Startup, setup_ui)
        .add_systems(
            Update,
            (
                handle_keyboard_input,
                process_easy_p2p_updates,
                consume_ping_updates,
                record_synced_events,
                track_synced_state,
                update_ui,
            ),
        )
        .run();
}

fn setup_ui(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn alert(message: &str) {
    if let Some(window) = window() {
        let _ = window.alert_with_message(message);
    }
}

fn prompt(message: &str) -> Option<String> {
    window()?.prompt_with_message(message).ok().flatten()
}

fn just_pressed_char(keys: &ButtonInput<Key>, ch: char) -> bool {
    let lower = ch.to_ascii_lowercase();
    let upper = ch.to_ascii_uppercase();
    let lower_key = Key::Character(lower.to_string().into());
    if lower == upper {
        keys.just_pressed(lower_key)
    } else {
        let upper_key = Key::Character(upper.to_string().into());
        keys.just_pressed(lower_key) || keys.just_pressed(upper_key)
    }
}

fn handle_keyboard_input(
    keyboard: Res<ButtonInput<Key>>,
    lobby_state: Res<State<P2PLobbyState>>,
    mut easy_p2p: EasyP2P<'_, '_, DemoTransport, DemoPlayerData, DemoInput, DemoSpawn>,
    mut status: ResMut<StatusLog>,
    mut event_writer: MessageWriter<DemoSyncedEvent>,
    synced_state: Res<State<DemoSyncedState>>,
    mut next_synced_state: ResMut<NextState<DemoSyncedState>>,
) {
    if just_pressed_char(&keyboard, 'h') && matches!(lobby_state.get(), P2PLobbyState::OutOfLobby) {
        easy_p2p.create_lobby();
        status.push("Creating lobby…");
    }

    if just_pressed_char(&keyboard, 'j') && matches!(lobby_state.get(), P2PLobbyState::OutOfLobby) {
        if let Some(code) = prompt("Enter lobby code (4 letters):") {
            let sanitized = code.trim().to_uppercase();
            if sanitized.is_empty() {
                status.push("Join cancelled (empty code).");
            } else {
                easy_p2p.join_lobby(&sanitized);
                status.push(format!("Joining lobby {sanitized}…"));
            }
        }
    }

    if just_pressed_char(&keyboard, 'l') && !matches!(lobby_state.get(), P2PLobbyState::OutOfLobby)
    {
        easy_p2p.exit_lobby();
        status.push("Leaving lobby…");
    }

    if just_pressed_char(&keyboard, 'n') {
        if let Some(name) = prompt("Enter a display name:") {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                status.push("Name unchanged (empty input).");
            } else {
                easy_p2p.set_local_player_data(DemoPlayerData {
                    name: trimmed.to_string(),
                });
                status.push(format!("Display name set to {trimmed}"));
            }
        }
    }

    if just_pressed_char(&keyboard, 'm') && !matches!(lobby_state.get(), P2PLobbyState::OutOfLobby)
    {
        if let Some(input) = prompt("Enter chat message:") {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                status.push("Chat message cancelled (empty input).");
            } else if easy_p2p.is_host() {
                easy_p2p.send_message_all(trimmed.to_string());
                status.push(format!("Host (you): {trimmed}"));
            } else {
                easy_p2p.send_message_to_host(trimmed.to_string());
                status.push(format!("You: {trimmed}"));
            }
        }
    }

    if just_pressed_char(&keyboard, 'e') {
        if !easy_p2p.is_host() {
            status.push("Only the host can broadcast synced events.");
        } else if let Some(input) = prompt("Enter synced event text:") {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                status.push("Synced event cancelled (empty input).");
            } else {
                event_writer.write(DemoSyncedEvent {
                    text: trimmed.to_string(),
                });
                status.push(format!("Broadcasting synced event: {trimmed}"));
            }
        }
    }

    if just_pressed_char(&keyboard, 't') {
        if !easy_p2p.is_host() {
            status.push("Only the host can change the synced state.");
        } else {
            let next = match synced_state.get() {
                DemoSyncedState::Waiting => DemoSyncedState::Ready,
                DemoSyncedState::Ready => DemoSyncedState::Waiting,
            };
            next_synced_state.set(next.clone());
            status.push(format!("Synced state set to {:?}", next));
        }
    }
}

fn process_easy_p2p_updates(
    mut updates: MessageReader<EasyP2PUpdate<DemoPlayerData, DemoInput, DemoSpawn>>,
    mut status: ResMut<StatusLog>,
) {
    for update in updates.read() {
        match update {
            EasyP2PUpdate::LobbyCreated { code } => {
                status.push(format!("Lobby created with code {code}"));
                alert(&format!(
                    "Share this lobby code with your friends:\n\n{code}"
                ));
            }
            EasyP2PUpdate::LobbyJoined { code } => {
                status.push(format!("Joining lobby {code} (waiting for connection)"));
            }
            EasyP2PUpdate::LobbyEntered { code } => {
                status.push(format!("Entered lobby {code}"));
            }
            EasyP2PUpdate::LobbyExited { reason } => {
                status.push(format!("Exited lobby ({reason:?})"));
            }
            EasyP2PUpdate::HostChat { text } => {
                status.push(format!("Host: {text}"));
            }
            EasyP2PUpdate::ClientChat { client_id, text } => {
                status.push(format!("Client {client_id}: {text}"));
            }
            EasyP2PUpdate::RosterUpdated { players } => {
                status.push(format!("Roster updated ({} player(s))", players.len()));
            }
            EasyP2PUpdate::ClientInput { .. } | EasyP2PUpdate::Instantiated { .. } => {}
        }
    }
}

fn consume_ping_updates(mut reader: MessageReader<PingUpdate>, mut display: ResMut<PingDisplay>) {
    for PingUpdate(duration) in reader.read() {
        display.0 = Some(duration.as_millis());
    }
}

fn record_synced_events(
    mut events: MessageReader<DemoSyncedEvent>,
    mut event_log: ResMut<EventLog>,
    mut status: ResMut<StatusLog>,
) {
    for DemoSyncedEvent { text } in events.read() {
        event_log.push(text.clone());
        status.push(format!("Synced event received: {text}"));
    }
}

fn track_synced_state(
    state: Res<State<DemoSyncedState>>,
    mut last: Local<Option<DemoSyncedState>>,
    mut status: ResMut<StatusLog>,
) {
    let current = state.get().clone();
    if last.as_ref() != Some(&current) {
        status.push(format!("Synced state changed to {:?}", current));
        *last = Some(current);
    }
}

fn update_ui(
    mut commands: Commands,
    lobby_state: Res<State<P2PLobbyState>>,
    easy_state: Res<EasyP2PState<DemoPlayerData>>,
    ping: Res<PingDisplay>,
    status: Res<StatusLog>,
    synced_state: Res<State<DemoSyncedState>>,
    events: Res<EventLog>,
    ui_root: Query<Entity, With<UIRoot>>,
) {
    for entity in ui_root.iter() {
        commands.entity(entity).despawn();
    }

    let mut text_content = String::new();
    text_content.push_str("Firestore P2P Lobby Demo\n\n");
    text_content.push_str(&format!(
        "Display Name: {}\n",
        easy_state.local_player_data.display_name()
    ));
    text_content.push_str(&format!("Synced State: {:?}\n", synced_state.get()));

    match lobby_state.get() {
        P2PLobbyState::OutOfLobby => {
            text_content.push_str("\nStatus: Not in a lobby\n");
        }
        P2PLobbyState::InLobby => {
            text_content.push_str("\nStatus: In lobby\n");
            text_content.push_str(&format!(
                "Role: {}\n",
                if easy_state.is_host { "Host" } else { "Client" }
            ));
            text_content.push_str(&format!("Lobby Code: {}\n", easy_state.lobby_code));
            if !easy_state.is_host {
                if let Some(ms) = ping.0 {
                    text_content.push_str(&format!("Ping: {ms} ms\n"));
                } else {
                    text_content.push_str("Ping: measuring…\n");
                }
            }
            text_content.push_str("\nPlayers:\n");
            for player in easy_state.get_players(easy_state.is_host) {
                text_content.push_str(&format!(
                    "- {} ({:?})\n",
                    player.data.display_name(),
                    player.id
                ));
            }
        }
        P2PLobbyState::JoiningLobby => {
            text_content.push_str("\nStatus: Joining lobby…\n");
            text_content.push_str("Waiting for host…\n");
        }
    }

    let mut controls: Vec<String> = Vec::new();
    match lobby_state.get() {
        P2PLobbyState::OutOfLobby => {
            controls.push("H - Host (create lobby code)".to_string());
            controls.push("J - Join (enter lobby code)".to_string());
            controls.push("N - Set display name".to_string());
        }
        P2PLobbyState::InLobby => {
            controls.push("L - Leave lobby".to_string());
            controls.push("N - Set display name".to_string());
            controls.push("M - Send chat message".to_string());
            if easy_state.is_host {
                controls.push("E - Broadcast synced event".to_string());
                controls.push("T - Toggle synced state".to_string());
            }
        }
        P2PLobbyState::JoiningLobby => {
            controls.push("L - Cancel join".to_string());
            controls.push("N - Set display name".to_string());
        }
    }

    if !controls.is_empty() {
        text_content.push_str("\nControls:\n");
        for control in controls {
            text_content.push_str(&control);
            text_content.push('\n');
        }
    }

    if !status.0.is_empty() {
        text_content.push_str("\nRecent Events:\n");
        for line in status.0.iter().rev() {
            text_content.push_str("- ");
            text_content.push_str(line);
            text_content.push('\n');
        }
    }

    if !events.0.is_empty() {
        text_content.push_str("\nSynced Event Log:\n");
        for line in events.0.iter().rev() {
            text_content.push_str("- ");
            text_content.push_str(line);
            text_content.push('\n');
        }
    }

    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            UIRoot,
        ))
        .with_children(|parent| {
            parent.spawn(Text(text_content));
        });
}
