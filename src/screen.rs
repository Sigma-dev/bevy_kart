//! What the player is looking at.
//!
//! The game has always had four screens, but no type for them: they were the
//! cross-product of [`LobbyState`] and [`AppState`], spelled out at every use
//! site. That cost showed up as a pair of `DespawnOnExit`s on every menu root,
//! a `spawn_lobby` registered on two different transitions because neither one
//! alone meant "the lobby screen", and run conditions that had to name both
//! states to say one thing.
//!
//! [`Screen`] names what was already there. It is a [`ComputedStates`], so it
//! can never disagree with the two states it is derived from, and
//! `bevy_state`'s blanket `impl<S: ComputedStates> States for S` means
//! `DespawnOnExit(Screen::X)` and `in_state(Screen::X)` both work.
//!
//! [`AppState`] deliberately gains nothing here: it is the payload of
//! [`GameStateChanged`](crate::GameStateChanged) and part of the wire format, so
//! a screen that only exists locally must not live in it.

use bevy::prelude::*;

use crate::{AppState, LobbyState};

/// Whether the level editor is open.
///
/// Freely mutable, because buttons open and close it. Meaningful only out of a
/// lobby -- see [`Screen::compute`], which ignores it otherwise.
#[derive(States, Default, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EditorState {
    #[default]
    Closed,
    Open,
}

/// The screen in front of the player. Derived, never set.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Screen {
    StartMenu,
    Lobby,
    Race,
    Editor,
}

impl ComputedStates for Screen {
    type SourceStates = (LobbyState, AppState, EditorState);

    fn compute((lobby, app, editor): Self::SourceStates) -> Option<Self> {
        Some(match (lobby, app, editor) {
            // A session always wins. An `Open` editor underneath a lobby is
            // ignored rather than being a state the rest of the game has to
            // defend against -- and `OnExit(Screen::Editor)` puts `EditorState`
            // back to `Closed`, so it cannot come back when the lobby ends.
            (LobbyState::OutOfLobby, AppState::OutOfGame, EditorState::Open) => Screen::Editor,
            (_, AppState::Game, _) => Screen::Race,
            (LobbyState::InLobby, _, _) => Screen::Lobby,
            _ => Screen::StartMenu,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spelled out rather than derived, so a change to `compute` has to disagree
    /// with a list somebody wrote down on purpose.
    #[test]
    fn every_combination_lands_on_the_screen_it_should() {
        use AppState::*;
        use EditorState::*;
        use LobbyState::*;
        let cases = [
            ((OutOfLobby, OutOfGame, Closed), Screen::StartMenu),
            ((OutOfLobby, OutOfGame, Open), Screen::Editor),
            // Racing outranks the editor flag, and racing outranks the lobby.
            ((OutOfLobby, Game, Closed), Screen::Race),
            ((OutOfLobby, Game, Open), Screen::Race),
            ((InLobby, OutOfGame, Closed), Screen::Lobby),
            // The editor is unreachable from inside a lobby.
            ((InLobby, OutOfGame, Open), Screen::Lobby),
            ((InLobby, Game, Closed), Screen::Race),
            ((InLobby, Game, Open), Screen::Race),
        ];
        for (sources, expected) in cases {
            assert_eq!(
                Screen::compute(sources.clone()),
                Some(expected.clone()),
                "{sources:?} should be {expected:?}"
            );
        }
    }

    /// The property the rest of the game leans on: `Screen::Race` and
    /// `AppState::Game` are the same fact, which is why everything under
    /// `src/track/` and `src/items/` can keep its `DespawnOnExit(AppState::Game)`.
    #[test]
    fn race_is_exactly_app_state_game() {
        for lobby in [LobbyState::OutOfLobby, LobbyState::InLobby] {
            for editor in [EditorState::Closed, EditorState::Open] {
                for app in [AppState::OutOfGame, AppState::Game] {
                    let screen = Screen::compute((lobby.clone(), app.clone(), editor.clone()));
                    assert_eq!(
                        screen == Some(Screen::Race),
                        app == AppState::Game,
                        "{lobby:?}/{app:?}/{editor:?}"
                    );
                }
            }
        }
    }
}
