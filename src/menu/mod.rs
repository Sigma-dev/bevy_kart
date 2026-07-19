use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::EditableText;

use crate::scene_util::insert;

pub struct MenuPlugin;

pub mod lobby;
pub mod start;

/// Emitted when a focused single-line text input is submitted with Enter.
///
/// Replaces `bevy_ui_text_input::SubmitText`; bevy's built-in `EditableText`
/// has no submit concept, so we detect Enter on the focused input ourselves.
#[derive(Message)]
pub(crate) struct TextSubmit {
    pub entity: Entity,
    pub text: String,
}

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((lobby::LobbyPlugin, start::StartPlugin))
            .add_message::<TextSubmit>()
            .add_systems(Update, (animate_button, emit_text_submit));
    }
}

/// Detect Enter on the currently focused text input and emit a [`TextSubmit`].
fn emit_text_submit(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    inputs: Query<&EditableText>,
    mut writer: MessageWriter<TextSubmit>,
) {
    if !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    let Some(entity) = focus.get() else {
        return;
    };
    if let Ok(input) = inputs.get(entity) {
        writer.write(TextSubmit {
            entity,
            text: input.value().to_string(),
        });
    }
}
#[derive(Component, Default, Clone)]
pub(crate) struct AnimatedButton(pub usize);

fn animate_button(time: Res<Time>, mut query: Query<(&mut ImageNode, &AnimatedButton)>) {
    for (mut image, button) in query.iter_mut() {
        let offset = (time.elapsed_secs() * 2.0) as usize % 2;
        let new_index = button.0 + offset;
        image.texture_atlas.as_mut().unwrap().index = new_index;
    }
}

/// A reusable animated menu button as a [`Scene`]. Callers compose an `on(...)`
/// observer for the click behaviour, e.g.
/// `bsn! { animated_button(0, tex, atlas) on(|_: On<Pointer<Press>>, ...| { ... }) }`.
///
/// Takes owned handles because a [`Scene`] is `'static` and cannot borrow.
pub(crate) fn animated_button(
    index: usize,
    buttons_texture: Handle<Image>,
    atlas: Handle<TextureAtlasLayout>,
) -> impl Scene {
    bsn! {
        {insert(ImageNode::from_atlas_image(buttons_texture, TextureAtlas::from(atlas)))}
        Button
        AnimatedButton({index})
        Node {
            height: px(16. * 4.),
            width: px(64. * 4.),
            border: {UiRect::all(px(5))},
        }
    }
}
