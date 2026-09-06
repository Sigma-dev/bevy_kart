//! Small shared UI pieces.
//!
//! `buttons.png` is a 2x4 grid of 64x16 cells with the label baked into the art,
//! and all four rows are taken (Host, Join, Start, Leave). Anything that needs a
//! new label therefore needs new art -- so the pieces here are built from `Text`
//! instead, which the lobby and menu already use for the ping readout, the chat
//! log and "Connecting...". Pixel-art buttons for these can come later without
//! anything else changing.

use bevy::prelude::*;

use crate::AppColors;
use crate::scene_util::insert;

/// A flat text button. The caller composes the click behaviour, exactly as with
/// [`animated_button`](super::animated_button):
///
/// ```ignore
/// bsn! { text_button("NEXT MAP") on(|_: On<Pointer<Press>>, ...| { ... }) }
/// ```
pub(crate) fn text_button(label: &str) -> impl Scene {
    button_of_size(label, 20.0)
}

/// A smaller one, for a panel with a lot of them.
pub(crate) fn small_button(label: &str) -> impl Scene {
    button_of_size(label, 16.0)
}

fn button_of_size(label: &str, font_size: f32) -> impl Scene {
    let label = label.to_string();
    bsn! {
        {insert((
            Text::new(label),
            TextFont { font_size: FontSize::Px(font_size), ..default() },
            BackgroundColor(AppColors::Grass.color().darker(0.28)),
        ))}
        Button
        Node {
            padding: {UiRect::axes(px(8), px(4))},
            justify_content: JustifyContent::Center,
        }
    }
}
