use bevy::prelude::*;

use crate::AssetHandles;

pub struct MenuPlugin;

pub mod lobby;
pub mod start;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((lobby::LobbyPlugin, start::StartPlugin))
            .add_systems(Update, animate_button);
    }
}
#[derive(Component)]
pub(crate) struct AnimatedButton(usize);

fn animate_button(time: Res<Time>, mut query: Query<(&mut ImageNode, &AnimatedButton)>) {
    for (mut image, button) in query.iter_mut() {
        let offset = (time.elapsed_secs() * 2.0) as usize % 2;
        let new_index = button.0 + offset;
        image.texture_atlas.as_mut().unwrap().index = new_index;
    }
}

fn animated_button_bundle(
    animated_button: AnimatedButton,
    handles: &AssetHandles,
    texture_atlas_handle: Handle<TextureAtlasLayout>,
) -> impl Bundle {
    (
        Button,
        ImageNode::from_atlas_image(
            handles.buttons_texture.clone(),
            TextureAtlas::from(texture_atlas_handle.clone()),
        ),
        animated_button,
        Node {
            height: px(16. * 4.),
            width: px(64. * 4.),
            border: UiRect::all(px(5)),
            ..default()
        },
    )
}
