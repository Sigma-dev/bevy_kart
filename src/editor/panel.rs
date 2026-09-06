//! The editor's side panel.
//!
//! Text buttons rather than the game's pixel-art ones: `buttons.png` is a 2x4
//! grid with the label baked into the art and all four rows spoken for, so every
//! label here would need new art. `Text` is already how these screens say
//! "Connecting..." and show the ping, so this is inside the existing idiom, and
//! art can replace it later without anything else moving.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter};

use crate::menu::widgets::{small_button, text_button};
use crate::scene_util::insert;
use crate::track::map::build::TrackWarning;
use crate::track::map::builtin::BUILTINS;
use crate::track::map::data::scalar_to_world;
use crate::track::map::file::{self, PendingImport};
use crate::track::map::{share, store};
use crate::{AppColors, EditorState, Screen};

use super::{EditorMap, History, Status, Tool};

#[derive(Component, Default, Clone)]
pub(crate) struct NameField;

#[derive(Component, Default, Clone)]
pub(crate) struct CodeField;

#[derive(Component, Default, Clone)]
pub(crate) struct StatusText;

#[derive(Component, Default, Clone)]
pub(crate) struct ValidationText;

#[derive(Component, Default, Clone)]
pub(crate) struct ToolText;

#[derive(Component, Default, Clone)]
pub(crate) struct MapList;

/// The saved-map list as it was last drawn, so the rows are only rebuilt when
/// they would actually differ.
#[derive(Resource, Default)]
pub struct ListedMaps(Vec<String>);

pub(crate) fn spawn_panel(mut commands: Commands, editor: Res<EditorMap>) {
    commands.init_resource::<ListedMaps>();
    let name = editor.data.name.clone();
    commands.spawn_scene(bsn! {
        {insert(DespawnOnExit(Screen::Editor))}
        Node {
            position_type: PositionType::Absolute,
            top: px(0),
            left: px(0),
            bottom: px(0),
            width: px(240),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            // Extra at the top so the title clears the FPS readout, which the
            // debug overlay pins to the same corner.
            padding: {UiRect::new(px(8), px(8), px(26), px(8))},
        }
        BackgroundColor({AppColors::Dark.color()})
        // So the pointer knows it is over the panel and the canvas leaves it alone.
        Pickable
        Children [
            (
                Text::new("TRACK EDITOR")
                TextFont { font_size: {FontSize::Px(22.)} }
            ),
            (
                EditableText::new(name)
                EditableText { max_characters: {Some(24)}, allow_newlines: false }
                EditableTextFilter::new(|c: char| c.is_alphanumeric() || " -_'".contains(c))
                TextFont { font_size: {FontSize::Px(20.)} }
                BackgroundColor({AppColors::Grass.color().darker(0.25)})
                Node { height: px(28), width: percent(100) }
                NameField
            ),
            (
                Node { column_gap: px(4), flex_wrap: FlexWrap::Wrap, row_gap: px(4) }
                Children [
                    (
                        small_button("NEW")
                        on(|_: On<Pointer<Press>>,
                           mut editor: ResMut<EditorMap>,
                           mut history: ResMut<History>,
                           mut status: ResMut<Status>| {
                            *editor = EditorMap::fresh();
                            history.clear();
                            status.say("New track.");
                        })
                    ),
                    (
                        small_button("SAVE")
                        on(|_: On<Pointer<Press>>,
                           mut editor: ResMut<EditorMap>,
                           mut status: ResMut<Status>| {
                            save_current(&mut editor, &mut status);
                        })
                    ),
                    (
                        small_button("DELETE")
                        on(|_: On<Pointer<Press>>,
                           mut editor: ResMut<EditorMap>,
                           mut status: ResMut<Status>| {
                            match editor.source.clone() {
                                Some(id) => match store::delete(&id) {
                                    Ok(()) => {
                                        editor.source = None;
                                        editor.dirty = true;
                                        status.say("Deleted.");
                                    }
                                    Err(error) => status.say(error),
                                },
                                None => status.say("This track has not been saved."),
                            }
                        })
                    ),
                ]
            ),
            (
                Node { column_gap: px(4), flex_wrap: FlexWrap::Wrap, row_gap: px(4) }
                Children [
                    (
                        small_button("EXPORT")
                        on(|_: On<Pointer<Press>>,
                           editor: Res<EditorMap>,
                           mut status: ResMut<Status>| {
                            match serde_json::to_string_pretty(&editor.data) {
                                Ok(json) => match file::export(&editor.data, &json) {
                                    Ok(where_it_went) => status.say(where_it_went),
                                    Err(error) => status.say(error),
                                },
                                Err(error) => status.say(format!("could not encode: {error}")),
                            }
                        })
                    ),
                    (
                        small_button("IMPORT")
                        on(|_: On<Pointer<Press>>,
                           pending: Res<PendingImport>,
                           mut status: ResMut<Status>| {
                            // Must be inside the click, so a browser counts it as
                            // a real gesture and lets the picker open.
                            file::request_import(&pending);
                            status.say(file::import_hint());
                        })
                    ),
                    (
                        small_button("COPY")
                        on(|_: On<Pointer<Press>>,
                           editor: Res<EditorMap>,
                           mut status: ResMut<Status>,
                           mut fields: Query<&mut EditableText, With<CodeField>>| {
                            match share::to_share_code(&editor.data) {
                                Ok(code) => {
                                    // Into the field rather than the clipboard:
                                    // the browser clipboard API is async and
                                    // permission-gated, and Ctrl+C works here.
                                    for mut field in fields.iter_mut() {
                                        field.editor_mut().set_text(&code);
                                    }
                                    status.say("Code ready below: select it and copy.");
                                }
                                Err(error) => status.say(error),
                            }
                        })
                    ),
                ]
            ),
            (
                EditableText::new("")
                EditableText { max_characters: {Some(4096)}, allow_newlines: false }
                TextFont { font_size: {FontSize::Px(12.)} }
                BackgroundColor({AppColors::Grass.color().darker(0.25)})
                Node { height: px(34), width: percent(100) }
                CodeField
            ),
            (
                text_button("PASTE CODE")
                on(|_: On<Pointer<Press>>,
                   mut editor: ResMut<EditorMap>,
                   mut history: ResMut<History>,
                   mut status: ResMut<Status>,
                   fields: Query<&EditableText, With<CodeField>>| {
                    let Some(field) = fields.iter().next() else { return };
                    match share::from_share_code(&field.value().to_string()) {
                        Ok(map) => {
                            *editor = EditorMap::from_loaded(map, None);
                            history.clear();
                            status.say("Loaded from a code.");
                        }
                        Err(error) => status.say(error),
                    }
                })
            ),
            (
                Text::new("TRACKS")
                TextFont { font_size: {FontSize::Px(18.)} }
            ),
            (
                MapList
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    max_height: px(180),
                    overflow: {Overflow::scroll_y()},
                }
            ),
            (
                ToolText
                Text::new("")
                TextFont { font_size: {FontSize::Px(15.)} }
            ),
            (
                {insert((
                    Text::new(concat!(
                        "drag node or handle\n",
                        "right-click road: add node\n",
                        "drag empty space: pan\n",
                        "wheel: zoom   F: fit\n",
                        "Del: remove node\n",
                        "[ ]: width   Backspace: clear\n",
                        "Alt-drag handle: break\n",
                        "Shift-drag: snap to grid\n",
                        "Tab: tool   Ctrl+Z: undo",
                    )),
                    TextFont { font_size: FontSize::Px(12.), ..default() },
                    TextColor(AppColors::Grass.color().lighter(0.3)),
                ))}
            ),
            (
                ValidationText
                Text::new("")
                TextFont { font_size: {FontSize::Px(14.)} }
            ),
            (
                StatusText
                Text::new("")
                TextFont { font_size: {FontSize::Px(14.)} }
            ),
            (
                Node { margin: {UiRect::top(px(6))} }
                Children [
                    (
                        text_button("BACK")
                        on(|_: On<Pointer<Press>>,
                           editor: Res<EditorMap>,
                           mut status: ResMut<Status>,
                           mut confirmed: Local<bool>,
                           mut next: ResMut<NextState<EditorState>>| {
                            // No modal idiom exists in this game and inventing one
                            // for this is not worth it, so unsaved work costs a
                            // second click instead.
                            if editor.dirty && !*confirmed {
                                *confirmed = true;
                                status.say("Unsaved. Press BACK again to discard.");
                                return;
                            }
                            *confirmed = false;
                            super::close_editor(&mut next);
                        })
                    ),
                ]
            ),
        ]
    });
}

fn save_current(editor: &mut EditorMap, status: &mut Status) {
    if let Err(error) = editor.data.validate() {
        status.say(format!("Cannot save: {error}"));
        return;
    }
    let existing = store::list();
    let id = editor
        .source
        .clone()
        .unwrap_or_else(|| store::unique_id(&editor.data.name, &existing));
    match store::save(&id, &editor.data) {
        Ok(()) => {
            editor.source = Some(id);
            editor.dirty = false;
            status.say(format!("Saved to {}.", store::storage_hint()));
        }
        Err(error) => status.say(error),
    }
}

/// Everything the panel shows that follows from state elsewhere.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_panel(
    mut commands: Commands,
    mut editor: ResMut<EditorMap>,
    mut history: ResMut<History>,
    mut status: ResMut<Status>,
    mut listed: ResMut<ListedMaps>,
    pending: Res<PendingImport>,
    tool: Res<Tool>,
    selection: Res<crate::editor::tools::Selection>,
    focus: Res<InputFocus>,
    names: Query<&EditableText, (With<NameField>, Changed<EditableText>)>,
    lists: Query<Entity, With<MapList>>,
    mut status_texts: Query<
        &mut Text,
        (With<StatusText>, Without<ValidationText>, Without<ToolText>),
    >,
    mut validation_texts: Query<
        (&mut Text, &mut TextColor),
        (With<ValidationText>, Without<ToolText>),
    >,
    mut tool_texts: Query<&mut Text, With<ToolText>>,
) {
    // Typing in the name field *is* renaming; there is no separate button.
    for field in names.iter() {
        let typed = field.value().to_string();
        if typed != editor.data.name {
            let name = typed;
            editor.edit(&mut history, |map| map.name = name);
        }
    }

    // A file dropped on the window, or chosen in a browser picker, lands here.
    if let Some(result) = pending.take() {
        match result.and_then(|text| {
            serde_json::from_str::<crate::track::map::MapData>(&text)
                .map_err(|error| format!("that file is not a map: {error}"))
        }) {
            Ok(map) => match map.validate() {
                Ok(()) => {
                    *editor = EditorMap::from_loaded(map, None);
                    history.clear();
                    status.say("Imported.");
                }
                Err(error) => status.say(format!("that map is not usable: {error}")),
            },
            Err(error) => status.say(error),
        }
    }

    for mut text in tool_texts.iter_mut() {
        // Say *which* width `[` and `]` would change: the selected node's, or the
        // map's default when nothing is selected. Showing only the default while
        // editing a node's own width is how you conclude the keys do nothing.
        let width = match selection.node.and_then(|index| editor.data.nodes.get(index)) {
            Some(node) => match node.half_width {
                Some(own) => format!("node width {:.1}", scalar_to_world(own)),
                None => format!(
                    "node width {:.1} (from map)",
                    scalar_to_world(editor.data.road.half_width)
                ),
            },
            None => format!(
                "map width {:.1}",
                scalar_to_world(editor.data.road.half_width)
            ),
        };
        let wanted = format!("Tool: {}   (Tab)\n{width}", tool.label());
        if text.0 != wanted {
            *text = Text::new(wanted);
        }
    }

    for mut text in status_texts.iter_mut() {
        if text.0 != status.0 {
            *text = Text::new(status.0.clone());
        }
    }

    for (mut text, mut colour) in validation_texts.iter_mut() {
        let (wanted, wanted_colour) = describe(&editor);
        if text.0 != wanted {
            *text = Text::new(wanted);
        }
        colour.0 = wanted_colour;
    }

    // The list is rebuilt only when what it would show has changed, the same
    // discipline `spawn_lobby_players_buttons` uses.
    let wanted: Vec<String> = BUILTINS
        .iter()
        .map(|builtin| format!("* {}", builtin.slug))
        .chain(store::list().into_iter().map(|meta| meta.id))
        .collect();
    if wanted != listed.0 {
        listed.0 = wanted.clone();
        for list in lists.iter() {
            commands.entity(list).despawn_children();
            for entry in &wanted {
                let entry = entry.clone();
                let row = commands
                    .spawn_scene(bsn! {
                        small_button(entry.as_str())
                        on(move |_: On<Pointer<Press>>,
                            mut editor: ResMut<EditorMap>,
                            mut history: ResMut<History>,
                            mut status: ResMut<Status>| {
                            load_entry(&entry, &mut editor, &mut history, &mut status);
                        })
                    })
                    .id();
                commands.entity(list).add_child(row);
            }
        }
    }

    // Keeps the shortcut handler from firing while a field has the keyboard.
    let _ = focus;
}

/// A built-in (`* slug`) or a saved map (`id`).
fn load_entry(entry: &str, editor: &mut EditorMap, history: &mut History, status: &mut Status) {
    let loaded = match entry.strip_prefix("* ") {
        Some(slug) => crate::track::map::by_slug(slug)
            .map(|map| (map, None))
            .ok_or_else(|| format!("no built-in called {slug}")),
        None => store::load(entry).map(|map| (map, Some(entry.to_string()))),
    };
    match loaded {
        Ok((map, source)) => {
            let name = map.name.clone();
            *editor = EditorMap::from_loaded(map, source);
            history.clear();
            status.say(format!("Loaded {name}."));
        }
        Err(error) => status.say(error),
    }
}

/// The one-line verdict on the map, and what colour to say it in.
fn describe(editor: &EditorMap) -> (String, Color) {
    if let Err(error) = editor.data.validate() {
        return (error.to_string(), Color::srgb(1.0, 0.4, 0.4));
    }
    let mut lines: Vec<String> = Vec::new();
    for warning in &editor.built.warnings {
        lines.push(match warning {
            TrackWarning::CornerTooTight { half_width, radius, .. } => format!(
                "A corner of radius {radius:.0} is too tight for a road {:.0} wide.",
                half_width * 2.0
            ),
            TrackWarning::LapPassesItself { gap, .. } => format!(
                "The lap passes within {gap:.0} of itself: laps may not count there."
            ),
            TrackWarning::AnchorOffRoad { .. } => "Something is off the road.".to_string(),
            TrackWarning::WidthClamped { .. } => {
                "A node is narrower than a road can be.".to_string()
            }
        });
    }
    if editor.data.item_boxes.is_empty() {
        lines.push("No item boxes.".to_string());
    }
    if lines.is_empty() {
        (
            format!(
                "{} nodes, {:.0} units round. Good to race.",
                editor.data.nodes.len(),
                editor.built.length
            ),
            Color::srgb(0.5, 1.0, 0.6),
        )
    } else {
        (lines.join("\n"), Color::srgb(1.0, 0.85, 0.4))
    }
}
