use bevy::prelude::*;

pub struct MenuPlugin;

pub mod lobby;
pub mod start;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(lobby::LobbyPlugin);
    }
}
