//! Choosing the track, in the lobby.
//!
//! Only the host picks, because only the host's choice is the one that travels.
//! A client is shown the name it was sent and nothing else -- deliberately, not
//! as a simplification: a client's saved-map ids are its own and mean nothing
//! about the host's, so offering it its own list would be offering it a choice
//! it cannot make.

use bevy::prelude::*;
use bevy_ticked_networking::prelude::LocalServerPlayer;

use crate::menu::widgets::text_button;
use crate::scene_util::insert;
use crate::track::SelectedMap;
use crate::track::map::build::{BuildLevel, build};
use crate::track::map::builtin::{BUILTINS, by_slug};
use crate::track::map::store;
use crate::AppColors;

/// Where the little map drawing sits, in world units.
const PREVIEW_CENTRE: Vec2 = Vec2::new(78.0, -34.0);
const PREVIEW_SIZE: Vec2 = Vec2::new(86.0, 46.0);

#[derive(Component, Default, Clone)]
pub(crate) struct MapPickerList;

/// What the list currently shows, so it is rebuilt only when it would differ.
#[derive(Resource, Default)]
pub struct ListedTracks(Vec<Entry>);

#[derive(Clone, PartialEq, Eq)]
struct Entry {
    /// `Some` for a built-in, by slug; `None` for one out of storage.
    builtin: Option<String>,
    id: String,
    label: String,
}

/// The selected map, reduced to something cheap to draw.
///
/// Rebuilt when the selection changes rather than every frame: building a track
/// is not free, and the lobby draws this sixty times a second.
#[derive(Resource, Default)]
pub struct PreviewOutline {
    name: String,
    left: Vec<Vec2>,
    right: Vec<Vec2>,
    start: Option<(Vec2, Vec2)>,
    bounds: Rect,
}

/// The picker's own column, added to the lobby's right-hand side.
pub(crate) fn spawn_picker(commands: &mut Commands, is_host: bool) -> Entity {
    let panel = commands
        .spawn_scene(bsn! {
            Node {
                position_type: PositionType::Absolute,
                top: px(50),
                right: px(5),
                width: px(260),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                align_items: AlignItems::FlexEnd,
            }
            Children [
                (
                    Text::new("MAP")
                    TextFont { font_size: {FontSize::Px(18.)} }
                ),
                (
                    Text::new("")
                    TextFont { font_size: {FontSize::Px(24.)} }
                    SelectedMapName
                ),
            ]
        })
        .id();

    if is_host {
        let list = commands
            .spawn_scene(bsn! {
                MapPickerList
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    align_items: AlignItems::FlexEnd,
                    max_height: px(220),
                    overflow: {Overflow::scroll_y()},
                }
            })
            .id();
        commands.entity(list).insert(ChildOf(panel));
    } else {
        let note = commands
            .spawn_scene(bsn! {
                {insert((
                    Text::new("chosen by the host"),
                    TextFont { font_size: FontSize::Px(14.), ..default() },
                    TextColor(AppColors::Grass.color().lighter(0.25)),
                ))}
            })
            .id();
        commands.entity(note).insert(ChildOf(panel));
    }
    panel
}

#[derive(Component, Default, Clone)]
pub(crate) struct SelectedMapName;

/// Keep the name in step with the selection.
///
/// Reads the same resource on the host and on a client: the host writes it from
/// this list, and a client has it written by `map_sync` when the host's choice
/// arrives. Neither knows or cares which it is.
pub(crate) fn show_selected_name(
    selected: Res<SelectedMap>,
    mut texts: Query<&mut Text, With<SelectedMapName>>,
) {
    for mut text in texts.iter_mut() {
        if text.0 != selected.0.name {
            *text = Text::new(selected.0.name.clone());
        }
    }
}

/// Host only: fill the list with the built-ins and whatever has been saved.
pub(crate) fn refresh_track_list(
    mut commands: Commands,
    mut listed: ResMut<ListedTracks>,
    server_player: Option<Res<LocalServerPlayer>>,
    lists: Query<Entity, With<MapPickerList>>,
) {
    if server_player.is_none() {
        return;
    }
    let wanted: Vec<Entry> = BUILTINS
        .iter()
        .map(|builtin| Entry {
            builtin: Some(builtin.slug.to_string()),
            id: builtin.slug.to_string(),
            label: builtin.load().name,
        })
        .chain(store::list().into_iter().map(|meta| Entry {
            builtin: None,
            id: meta.id,
            label: meta.name,
        }))
        .collect();
    if wanted == listed.0 {
        return;
    }
    listed.0 = wanted.clone();

    for list in lists.iter() {
        commands.entity(list).despawn_children();
        for entry in &wanted {
            let builtin = entry.builtin.clone();
            let id = entry.id.clone();
            let row = commands
                .spawn_scene(bsn! {
                    text_button(entry.label.as_str())
                    on(move |_: On<Pointer<Press>>,
                        mut selected: ResMut<SelectedMap>,
                        mut commands: Commands| {
                        let loaded = match &builtin {
                            Some(slug) => by_slug(slug),
                            None => store::load(&id).ok(),
                        };
                        match loaded {
                            // Changing the resource is the whole action: the
                            // announce system notices and tells everybody.
                            Some(map) => selected.0 = map,
                            None => {
                                commands.queue(|_: &mut World| {
                                    warn!("that map could not be loaded");
                                });
                            }
                        }
                    })
                })
                .id();
            commands.entity(row).insert(ChildOf(list));
        }
    }
}

/// Rebuild the drawing when the selection changes.
pub(crate) fn refresh_preview(selected: Res<SelectedMap>, mut preview: ResMut<PreviewOutline>) {
    if !selected.is_changed() && preview.name == selected.0.name {
        return;
    }
    let built = build(&selected.0, BuildLevel::Preview);
    let start = built.centre.first().map(|sample| {
        let across = sample.normal * sample.half_width;
        (sample.position + across, sample.position - across)
    });
    *preview = PreviewOutline {
        name: selected.0.name.clone(),
        left: built.left_wall,
        right: built.right_wall,
        start,
        bounds: built.bounds,
    };
}

/// Draw the selected map, small, beside the list.
///
/// Gizmos rather than a second camera or a render target: the lobby only needs a
/// shape, it changes about once a minute, and this is the same geometry the race
/// will build, so it cannot be a picture of a different track.
pub(crate) fn draw_preview(mut gizmos: Gizmos, preview: Res<PreviewOutline>) {
    if preview.left.is_empty() && preview.right.is_empty() {
        return;
    }
    let size = preview.bounds.size().max(Vec2::splat(1.0));
    let scale = (PREVIEW_SIZE / size).min_element();
    let centre = preview.bounds.center();
    let place = |point: Vec2| PREVIEW_CENTRE + (point - centre) * scale;

    for wall in [&preview.left, &preview.right] {
        if wall.len() > 1 {
            gizmos.linestrip_2d(
                wall.iter().chain(wall.first()).map(|point| place(*point)),
                AppColors::Road.color().lighter(0.35),
            );
        }
    }
    if let Some((a, b)) = preview.start {
        gizmos.line_2d(place(a), place(b), Color::srgb(1.0, 1.0, 0.4));
    }
}
