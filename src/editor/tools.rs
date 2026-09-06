//! Grabbing things on the canvas and moving them.
//!
//! Hit-testing is done by hand rather than through a picking backend, for four
//! reasons that all point the same way. The editor zooms from a quarter to four
//! times, and a hit radius has to stay constant in *screen pixels* -- a backend
//! tests world-space bounds, so a fixed-size handle becomes a one-pixel target
//! exactly when you are zoomed out and want to grab it. Handles are gizmos, and
//! gizmos cannot be picked at all. The priority we want is handle over node over
//! start line over item box, with nearest-within-radius as the tie-break, where a
//! backend gives a depth-sorted list. And inserting a node is a "closest point on
//! the curve" query anyway, which is most of a hit test already.

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;

use crate::camera::MainCamera;
use crate::track::map::data::{
    MIN_NODES, TrackAnchor, TrackNode, scalar_to_map, scalar_to_world, to_map, to_world,
};

use super::cursor::EditorCursor;
use super::{EditorMap, History, Status, Tool};
#[cfg(test)]
use super::HISTORY_DEPTH;

/// Grab radius, in screen pixels.
const GRAB_PX: f32 = 12.0;

/// How near a node has to be, in screen pixels, for it to be the one whose
/// handles are shown and grabbable.
///
/// Generous, because it is what makes the handles reachable at all: they stick
/// out from the node, so the pointer is never *on* the node when it goes for one.
const FOCUS_PX: f32 = 60.0;

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;

/// What the pointer is currently moving.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub enum Drag {
    #[default]
    None,
    Node(usize),
    HandleIn(usize),
    HandleOut(usize),
    StartLine,
    ItemBox(usize),
    /// Panning: the world point that was under the cursor when it was grabbed,
    /// which is the point that has to stay there.
    Pan(Vec2),
}

#[derive(Resource, Default)]
pub struct Selection {
    pub node: Option<usize>,
    pub item_box: Option<usize>,
    /// What is under the pointer right now. Drawn highlighted, so what a click
    /// would do is visible before the click.
    pub hovered: Option<Hovered>,
    /// The node whose handles are on show. See [`focus_node`].
    pub focus: Option<usize>,
}

/// The kinds of thing the overlay highlights on hover.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hovered {
    Node(usize),
    HandleIn(usize),
    HandleOut(usize),
    StartLine,
    ItemBox(usize),
}

/// What is under the cursor, in priority order.
#[derive(Clone, Copy)]
enum Grab {
    Node(usize),
    HandleIn(usize),
    HandleOut(usize),
    StartLine,
    ItemBox(usize),
}

/// The node whose handles are shown, and therefore the only ones grabbable.
///
/// One function so that what is drawn and what can be picked up cannot disagree.
/// They did: handles appeared on hover but only the *selected* node's could be
/// grabbed, so the editor drew controls it then refused to move.
///
/// Whichever node is nearest, counting its handle tips as part of it, so
/// reaching for a handle keeps the focus on the node it belongs to.
pub fn focus_node(editor: &EditorMap, at: Vec2, reach: f32) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for (index, node) in editor.data.nodes.iter().enumerate() {
        let position = to_world(node.position);
        let distance = at
            .distance(position)
            .min(at.distance(position + to_world(node.in_handle)))
            .min(at.distance(position + to_world(node.out_handle)));
        if distance <= reach && best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, index));
        }
    }
    best.map(|(_, index)| index)
}

fn find_grab(editor: &EditorMap, at: Vec2, radius: f32, focus: Option<usize>) -> Option<Grab> {
    let map = &editor.data;
    let mut best: Option<(f32, Grab)> = None;
    let mut consider = |distance: f32, grab: Grab| {
        if distance <= radius && best.as_ref().is_none_or(|(d, _)| distance < *d) {
            best = Some((distance, grab));
        }
    };

    // The focused node's handles, and only that node's: any more and a cluster of
    // nodes becomes a cloud of indistinguishable dots.
    if let Some(index) = focus.filter(|i| *i < map.nodes.len()) {
        let node = map.nodes[index];
        let position = to_world(node.position);
        consider(
            at.distance(position + to_world(node.in_handle)),
            Grab::HandleIn(index),
        );
        consider(
            at.distance(position + to_world(node.out_handle)),
            Grab::HandleOut(index),
        );
    }
    for (index, node) in map.nodes.iter().enumerate() {
        consider(at.distance(to_world(node.position)), Grab::Node(index));
    }
    if let Some(start) = editor.built.centre.first() {
        consider(at.distance(start.position), Grab::StartLine);
    }
    for (index, position) in editor.built.item_boxes.iter().enumerate() {
        consider(at.distance(*position), Grab::ItemBox(index));
    }
    best.map(|(_, grab)| grab)
}

/// The closest point on the sampled centreline, as the anchor that names it.
///
/// Used for inserting a node, for dragging the start line, and for placing an
/// item box -- all three are the same question.
pub fn nearest_anchor(editor: &EditorMap, at: Vec2) -> Option<(TrackAnchor, f32)> {
    let map = &editor.data;
    let segments = map.segment_count();
    let mut best: Option<(f32, TrackAnchor, f32)> = None;
    // Walk each segment's flattened points rather than the resampled centreline:
    // the anchor has to name a segment and a parameter within it.
    const STEPS: usize = 48;
    for segment in 0..segments {
        let control = map.segment_control_points(segment);
        let mut previous = control[0];
        for step in 1..=STEPS {
            let t = step as f32 / STEPS as f32;
            let point = bezier_point(control, t);
            let (closest, along) = closest_on_segment(at, previous, point);
            let distance = at.distance(closest);
            if best.as_ref().is_none_or(|(d, _, _)| distance < *d) {
                let u = (step as f32 - 1.0 + along) / STEPS as f32;
                // Signed offset across the road, so an item box keeps the side
                // it was dropped on.
                let tangent = (point - previous).normalize_or_zero();
                let normal = Vec2::new(-tangent.y, tangent.x);
                let lateral = (at - closest).dot(normal);
                best = Some((
                    distance,
                    TrackAnchor::new(segment as u16, u, scalar_to_map(lateral)),
                    distance,
                ));
            }
            previous = point;
        }
    }
    best.map(|(_, anchor, distance)| (anchor, distance))
}

fn bezier_point(p: [Vec2; 4], t: f32) -> Vec2 {
    let mt = 1.0 - t;
    p[0] * (mt * mt * mt)
        + p[1] * (3.0 * mt * mt * t)
        + p[2] * (3.0 * mt * t * t)
        + p[3] * (t * t * t)
}

fn closest_on_segment(point: Vec2, a: Vec2, b: Vec2) -> (Vec2, f32) {
    let ab = b - a;
    let length_squared = ab.length_squared();
    if length_squared < f32::EPSILON {
        return (a, 0.0);
    }
    let t = ((point - a).dot(ab) / length_squared).clamp(0.0, 1.0);
    (a + ab * t, t)
}

#[allow(clippy::too_many_arguments)]
pub fn handle_pointer(
    mut editor: ResMut<EditorMap>,
    mut history: ResMut<History>,
    mut drag: ResMut<Drag>,
    mut selection: ResMut<Selection>,
    mut status: ResMut<Status>,
    tool: Res<Tool>,
    cursor: Res<EditorCursor>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut input_focus: ResMut<bevy::input_focus::InputFocus>,
) {
    let Some(at) = cursor.world else { return };
    let radius = GRAB_PX * cursor.world_per_px;

    // The node the overlay shows handles for, recomputed every frame.
    selection.focus = if cursor.over_ui {
        None
    } else {
        focus_node(&editor, at, FOCUS_PX * cursor.world_per_px)
    };
    let focus = selection.focus;

    // What a click would grab, every frame, so the overlay can say so.
    selection.hovered = if cursor.over_ui {
        None
    } else {
        find_grab(&editor, at, radius, focus).map(|grab| match grab {
            Grab::Node(i) => Hovered::Node(i),
            Grab::HandleIn(i) => Hovered::HandleIn(i),
            Grab::HandleOut(i) => Hovered::HandleOut(i),
            Grab::StartLine => Hovered::StartLine,
            Grab::ItemBox(i) => Hovered::ItemBox(i),
        })
    };

    if buttons.just_released(MouseButton::Left) {
        *drag = Drag::None;
    }

    if (buttons.just_pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Right))
        && !cursor.over_ui
    {
        // Clicking the canvas takes the keyboard back off the name field, so the
        // shortcuts work again without having to know why they stopped.
        input_focus.clear();
    }
    if buttons.just_pressed(MouseButton::Middle) && !cursor.over_ui {
        *drag = Drag::Pan(at);
    }
    if buttons.just_pressed(MouseButton::Left) && !cursor.over_ui {
        let grabbed = find_grab(&editor, at, radius, focus);
        // One undo step per drag, taken as it starts. The drag itself writes the
        // map every frame it moves and records none of them.
        if grabbed.is_some() {
            history.push(editor.data.clone());
        }
        match grabbed {
            Some(Grab::Node(index)) => {
                selection.node = Some(index);
                *drag = Drag::Node(index);
            }
            Some(Grab::HandleIn(index)) => *drag = Drag::HandleIn(index),
            Some(Grab::HandleOut(index)) => *drag = Drag::HandleOut(index),
            Some(Grab::StartLine) => *drag = Drag::StartLine,
            Some(Grab::ItemBox(index)) => {
                selection.item_box = Some(index);
                *drag = Drag::ItemBox(index);
            }
            None => match *tool {
                // In item mode a click on empty road drops a box; otherwise the
                // most common gesture on empty space is to pan, so that is what
                // it does.
                Tool::ItemBoxes => {
                    if let Some((anchor, distance)) = nearest_anchor(&editor, at) {
                        if distance < road_reach(&editor) {
                            editor.edit(&mut history, |map| map.item_boxes.push(anchor));
                            status.say("Item box placed.");
                        } else {
                            *drag = Drag::Pan(at);
                        }
                    }
                }
                // Deliberately keeps the selection: nudging the view should not
                // throw away the node whose handles you were about to grab.
                // Escape clears it, and clicking another node replaces it.
                _ => *drag = Drag::Pan(at),
            },
        }
    }

    // Right-click: insert a node on the road, or remove an item box.
    if buttons.just_pressed(MouseButton::Right) && !cursor.over_ui {
        match find_grab(&editor, at, radius, focus) {
            Some(Grab::ItemBox(index)) => {
                editor.edit(&mut history, |map| {
                    map.item_boxes.remove(index);
                });
                selection.item_box = None;
                status.say("Item box removed.");
            }
            _ => {
                if let Some((anchor, distance)) = nearest_anchor(&editor, at)
                    && distance < road_reach(&editor) {
                        insert_node(&mut editor, &mut history, anchor);
                        selection.node = Some(anchor.segment as usize + 1);
                        status.say("Node inserted.");
                    }
            }
        }
    }

    if !buttons.pressed(MouseButton::Left) && !buttons.pressed(MouseButton::Middle) {
        if matches!(*drag, Drag::Pan(_)) {
            *drag = Drag::None;
        }
        return;
    }

    let snap = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let break_mirror = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    match *drag {
        Drag::Node(index) => {
            let target = if snap { at.round() } else { at };
            editor.edit_in_progress(|map| {
                if let Some(node) = map.nodes.get_mut(index) {
                    node.position = to_map(target);
                }
            });
        }
        Drag::HandleIn(index) | Drag::HandleOut(index) => {
            let outgoing = matches!(*drag, Drag::HandleOut(index2) if index2 == index);
            editor.edit_in_progress(|map| {
                let Some(node) = map.nodes.get_mut(index) else {
                    return;
                };
                let offset = at - to_world(node.position);
                if break_mirror {
                    node.mirrored = false;
                }
                if outgoing {
                    node.out_handle = to_map(offset);
                    if node.mirrored {
                        node.in_handle = to_map(mirror(offset, to_world(node.in_handle)));
                    }
                } else {
                    node.in_handle = to_map(offset);
                    if node.mirrored {
                        node.out_handle = to_map(mirror(offset, to_world(node.out_handle)));
                    }
                }
            });
        }
        Drag::StartLine => {
            if let Some((anchor, _)) = nearest_anchor(&editor, at) {
                editor.edit_in_progress(|map| {
                    // The line spans the road, so only where it sits along the
                    // lap is editable -- never how far across.
                    map.start.at = TrackAnchor::new(anchor.segment, anchor.t_fraction(), 0);
                });
            }
        }
        Drag::ItemBox(index) => {
            if let Some((anchor, _)) = nearest_anchor(&editor, at) {
                editor.edit_in_progress(|map| {
                    if let Some(slot) = map.item_boxes.get_mut(index) {
                        *slot = anchor;
                    }
                });
            }
        }
        Drag::Pan(_) | Drag::None => {}
    }
}

/// How far from the centreline still counts as "on the road", for placement.
fn road_reach(editor: &EditorMap) -> f32 {
    editor
        .built
        .centre
        .iter()
        .map(|sample| sample.half_width)
        .fold(0.0f32, f32::max)
        + 4.0
}

/// Mirror a handle's *direction* while keeping its own length.
///
/// Length independently, so dragging one side does not yank the far side of the
/// curve about -- the ordinary smooth-node behaviour.
fn mirror(dragged: Vec2, other: Vec2) -> Vec2 {
    -dragged.normalize_or_zero() * other.length()
}

/// Split a segment at an anchor, adding a node without changing the curve.
///
/// De Casteljau: subdividing a cubic bezier at `t` gives two cubics that trace
/// exactly the original, so the road does not move under the author's cursor.
/// Any other construction pops.
fn insert_node(editor: &mut EditorMap, history: &mut History, anchor: TrackAnchor) {
    let segment = anchor.segment as usize;
    let t = anchor.t_fraction();
    let control = editor.data.segment_control_points(segment);

    let p01 = control[0].lerp(control[1], t);
    let p12 = control[1].lerp(control[2], t);
    let p23 = control[2].lerp(control[3], t);
    let p012 = p01.lerp(p12, t);
    let p123 = p12.lerp(p23, t);
    let middle = p012.lerp(p123, t);

    editor.edit(history, |map| {
        let count = map.nodes.len();
        let next = (segment + 1) % count;
        // The three nodes the split touches: the one before keeps its new outgoing
        // handle, the new one gets both halves, the one after keeps its incoming.
        map.nodes[segment].out_handle = to_map(p01 - to_world(map.nodes[segment].position));
        map.nodes[next].in_handle = to_map(p23 - to_world(map.nodes[next].position));
        map.nodes.insert(
            segment + 1,
            TrackNode {
                position: to_map(middle),
                in_handle: to_map(p012 - middle),
                out_handle: to_map(p123 - middle),
                half_width: None,
                mirrored: false,
            },
        );
        // Anchors after the insertion point move up one segment.
        for anchor in core::iter::once(&mut map.start.at).chain(map.item_boxes.iter_mut()) {
            if anchor.segment as usize > segment {
                anchor.segment += 1;
            } else if anchor.segment as usize == segment {
                // Split across the two halves it became.
                let u = anchor.t_fraction();
                if u <= t {
                    *anchor = TrackAnchor::new(segment as u16, u / t.max(1e-6), anchor.lateral);
                } else {
                    *anchor = TrackAnchor::new(
                        segment as u16 + 1,
                        (u - t) / (1.0 - t).max(1e-6),
                        anchor.lateral,
                    );
                }
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub fn handle_keys(
    mut editor: ResMut<EditorMap>,
    mut history: ResMut<History>,
    mut selection: ResMut<Selection>,
    mut tool: ResMut<Tool>,
    mut status: ResMut<Status>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<bevy::input_focus::InputFocus>,
    text_fields: Query<(), With<bevy::text::EditableText>>,
    mut camera: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
) {
    // Skip only while a *text field* has the keyboard, or every shortcut here
    // would type into the map's name instead.
    //
    // Not `focus.get().is_some()`: `set_initial_focus` focuses the primary window
    // whenever nothing else claims it, so something is always focused and that
    // test silently disables every shortcut in the editor, permanently.
    if focus.get().is_some_and(|entity| text_fields.contains(entity)) {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    if control && keys.just_pressed(KeyCode::KeyZ) {
        let current = editor.data.clone();
        let restored = if shift {
            history.redo(&current)
        } else {
            history.undo(&current)
        };
        if let Some(map) = restored {
            editor.data = map;
            editor.dirty = true;
            status.say(if shift { "Redone." } else { "Undone." });
        } else {
            status.say("Nothing to undo.");
        }
        return;
    }

    if keys.just_pressed(KeyCode::Tab) {
        *tool = tool.next();
        status.say(format!("Tool: {}", tool.label()));
    }

    if (keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::KeyX))
        && let Some(index) = selection.node {
            if editor.data.nodes.len() <= MIN_NODES {
                status.say(format!("A track needs at least {MIN_NODES} nodes."));
            } else {
                editor.edit(&mut history, |map| {
                    map.remap_anchors_after_node_removal(index);
                    map.nodes.remove(index);
                });
                selection.node = None;
                status.say("Node removed.");
            }
        }

    // Width: the selected node's, or the map's default when nothing is selected.
    let widen = keys.just_pressed(KeyCode::BracketRight);
    let narrow = keys.just_pressed(KeyCode::BracketLeft);
    if widen || narrow {
        let step = scalar_to_map(if widen { 0.5 } else { -0.5 });
        match selection.node {
            Some(index) => {
                let current = editor.data.nodes[index]
                    .half_width
                    .unwrap_or(editor.data.road.half_width);
                editor.edit(&mut history, |map| {
                    map.nodes[index].half_width = Some((current + step).max(1));
                });
                let width = scalar_to_world(editor.data.nodes[index].half_width.unwrap());
                status.say(format!("Node width {width:.1}"));
            }
            None => {
                editor.edit(&mut history, |map| {
                    map.road.half_width = (map.road.half_width + step).max(1);
                });
                status.say(format!(
                    "Road width {:.1}",
                    scalar_to_world(editor.data.road.half_width)
                ));
            }
        }
    }

    // Backspace clears a node's own width, handing it back to the map default.
    if keys.just_pressed(KeyCode::Backspace)
        && let Some(index) = selection.node {
            editor.edit(&mut history, |map| map.nodes[index].half_width = None);
            status.say("Node width follows the map again.");
        }

    if keys.just_pressed(KeyCode::KeyF)
        && let Ok((mut transform, mut projection)) = camera.single_mut() {
            frame_map(&editor, &mut transform, &mut projection);
            status.say("Framed the map.");
        }

    // Escape clears the selection and nothing else. It used to leave the editor
    // once nothing was selected, which put "discard my unsaved track" one stray
    // keypress away -- and the BACK button already asks twice for that.
    if keys.just_pressed(KeyCode::Escape) {
        selection.node = None;
        selection.item_box = None;
    }
}

pub fn frame_map(editor: &EditorMap, transform: &mut Transform, projection: &mut Projection) {
    let bounds = editor.built.bounds;
    if let Projection::Orthographic(orthographic) = projection {
        let needed = (bounds.width() / crate::RESOLUTION.x)
            .max(bounds.height() / crate::RESOLUTION.y)
            * 1.1;
        orthographic.scale = needed.clamp(MIN_ZOOM, MAX_ZOOM);
    }
    let centre = bounds.center();
    transform.translation.x = centre.x;
    transform.translation.y = centre.y;
}

/// Drag the world under the cursor, and zoom about the point it is over.
pub fn pan_and_zoom(
    drag: Res<Drag>,
    cursor: Res<EditorCursor>,
    scroll: Res<AccumulatedMouseScroll>,
    mut camera: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
) {
    let Ok((mut transform, mut projection)) = camera.single_mut() else {
        return;
    };
    let Some(at) = cursor.world else { return };

    // Panning keeps the grabbed world point under the cursor, which is
    // frame-rate independent and cannot drift, unlike a delta times a speed.
    if let Drag::Pan(grabbed) = *drag {
        let delta = at - grabbed;
        transform.translation.x -= delta.x;
        transform.translation.y -= delta.y;
    }

    if scroll.delta.y != 0.0 && !cursor.over_ui
        && let Projection::Orthographic(orthographic) = projection.as_mut() {
            let before = at;
            orthographic.scale =
                (orthographic.scale * 1.1f32.powf(-scroll.delta.y)).clamp(MIN_ZOOM, MAX_ZOOM);
            // Correct the translation so the world point under the cursor is the
            // one still under it after the zoom.
            let ratio = 1.1f32.powf(-scroll.delta.y);
            let centre = transform.translation.truncate();
            let after = centre + (before - centre) * ratio;
            let correction = before - after;
            transform.translation.x += correction.x;
            transform.translation.y += correction.y;
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::map::build::{BuildLevel, build, tests::circle};
    use crate::track::map::data::MapData;

    fn editor_for(map: MapData) -> EditorMap {
        EditorMap::from_loaded(map, None)
    }

    /// Inserting a node must not move the road under the author's cursor. De
    /// Casteljau subdivision gives two cubics that trace the original exactly,
    /// which is the whole reason to use it rather than guessing handles.
    #[test]
    fn inserting_a_node_does_not_change_the_curve() {
        let mut editor = editor_for(circle(80.0, 11.0));
        let mut history = History::default();
        let before = build(&editor.data, BuildLevel::Preview);

        insert_node(&mut editor, &mut history, TrackAnchor::new(1, 0.5, 0));
        assert_eq!(editor.data.nodes.len(), 5, "one more node");

        let after = build(&editor.data, BuildLevel::Preview);
        assert!(
            (after.length - before.length).abs() < 0.5,
            "lap length moved from {} to {}",
            before.length,
            after.length
        );
        // Every point of the new centreline is still on the original circle.
        for sample in &after.centre {
            assert!(
                (sample.position.length() - 80.0).abs() < 0.5,
                "a sample moved to radius {}",
                sample.position.length()
            );
        }
    }

    /// The anchors have to follow the split, or every item box past the
    /// insertion point jumps a segment.
    #[test]
    fn inserting_a_node_carries_the_anchors_with_it() {
        let mut map = circle(80.0, 11.0);
        map.item_boxes = vec![
            TrackAnchor::new(0, 0.5, 0),
            TrackAnchor::new(1, 0.25, 0),
            TrackAnchor::new(1, 0.75, 0),
            TrackAnchor::new(3, 0.5, 0),
        ];
        let mut editor = editor_for(map);
        let before = build(&editor.data, BuildLevel::Full).item_boxes;

        let mut history = History::default();
        insert_node(&mut editor, &mut history, TrackAnchor::new(1, 0.5, 0));
        let after = build(&editor.data, BuildLevel::Full).item_boxes;

        assert_eq!(before.len(), after.len());
        for (index, (a, b)) in before.iter().zip(after.iter()).enumerate() {
            assert!(a.distance(*b) < 1.0, "item box {index} moved from {a:?} to {b:?}");
        }
    }

    #[test]
    fn the_nearest_anchor_names_the_point_it_found() {
        let editor = editor_for(circle(80.0, 11.0));
        // Straight out along +x is the start of segment 0, at (80, 0).
        let (anchor, distance) = nearest_anchor(&editor, Vec2::new(100.0, 0.0)).unwrap();
        assert!((distance - 20.0).abs() < 1.0, "distance {distance}");
        assert!(
            anchor.segment == 0 && anchor.t_fraction() < 0.05
                || anchor.segment == 3 && anchor.t_fraction() > 0.95,
            "landed on segment {} at t {}",
            anchor.segment,
            anchor.t_fraction()
        );
        // Outside the loop is a positive lateral only if left-of-travel points
        // that way; the magnitude is what matters here.
        assert!((scalar_to_world(anchor.lateral).abs() - 20.0).abs() < 1.0);
    }

    /// Mirroring keeps each handle's own length, so dragging one side does not
    /// yank the far side of the curve about.
    #[test]
    fn mirroring_a_handle_turns_it_without_resizing_the_other() {
        let dragged = Vec2::new(10.0, 0.0);
        let other = Vec2::new(-3.0, 0.0);
        let mirrored = mirror(dragged, other);
        assert!((mirrored.length() - other.length()).abs() < 1e-4, "length kept");
        assert!(
            mirrored.normalize().dot(dragged.normalize()) < -0.99,
            "direction opposed"
        );
    }

    #[test]
    fn undo_and_redo_walk_the_same_path() {
        let mut editor = editor_for(circle(80.0, 11.0));
        let mut history = History::default();
        let original = editor.data.clone();

        editor.edit(&mut history, |map| map.name = "One".into());
        editor.edit(&mut history, |map| map.name = "Two".into());
        assert!(editor.dirty);

        let current = editor.data.clone();
        editor.data = history.undo(&current).unwrap();
        assert_eq!(editor.data.name, "One");
        let current = editor.data.clone();
        editor.data = history.undo(&current).unwrap();
        assert_eq!(editor.data.name, original.name);
        assert!(history.undo(&editor.data).is_none(), "and no further");

        let current = editor.data.clone();
        editor.data = history.redo(&current).unwrap();
        assert_eq!(editor.data.name, "One");
    }

    /// A new edit after undoing throws the redo branch away, which is what
    /// everybody expects and nobody says out loud.
    #[test]
    fn editing_after_an_undo_forgets_the_redo() {
        let mut editor = editor_for(circle(80.0, 11.0));
        let mut history = History::default();
        editor.edit(&mut history, |map| map.name = "One".into());
        let current = editor.data.clone();
        editor.data = history.undo(&current).unwrap();

        editor.edit(&mut history, |map| map.name = "Other".into());
        assert!(history.redo(&editor.data).is_none());
    }

    #[test]
    fn history_does_not_grow_without_limit() {
        let mut editor = editor_for(circle(80.0, 11.0));
        let mut history = History::default();
        for i in 0..(HISTORY_DEPTH * 3) {
            editor.edit(&mut history, |map| map.name = format!("{i}"));
        }
        assert_eq!(history.past.len(), HISTORY_DEPTH);
    }
}
