//! Grabbing things on the canvas and moving them.
//!
//! Hit-testing is done by hand rather than through a picking backend, for four
//! reasons that all point the same way. The editor zooms from a quarter to four
//! times and a grab radius has to stay constant in *screen pixels* -- a backend
//! tests world-space bounds, so a fixed-size handle becomes a one-pixel target
//! exactly when you are zoomed out and want to grab it. Handles are gizmos, and
//! gizmos cannot be picked at all. The priority wanted is handle over node over
//! start line over item box with nearest-within-radius as the tie-break, where a
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
///
/// Every variant that moves something carries the offset between the thing and
/// the cursor at the moment it was grabbed. Without it, picking a node up puts
/// it exactly under the cursor -- so the first pixel of every drag teleports it,
/// and nudging a node by two units means clicking it precisely and not moving.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub enum Drag {
    #[default]
    None,
    Node(usize, Vec2),
    HandleIn(usize, Vec2),
    HandleOut(usize, Vec2),
    /// A road-edge handle: which node, which side, and the offset.
    Width(usize, f32, Vec2),
    StartLine(Vec2),
    ItemBox(usize, Vec2),
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
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hovered {
    Node(usize),
    HandleIn(usize),
    HandleOut(usize),
    Width(usize, f32),
    StartLine,
    ItemBox(usize),
}

/// What is under the cursor, in priority order.
#[derive(Clone, Copy)]
enum Grab {
    Node(usize),
    HandleIn(usize),
    HandleOut(usize),
    /// Which node, and which side of the road: +1 left of travel, -1 right.
    Width(usize, f32),
    StartLine,
    ItemBox(usize),
}

/// The direction the road runs at a node, and the normal across it.
///
/// Taken from the node's own handles where it has them, so it matches the curve
/// the road is actually built from, and from the neighbours otherwise.
pub fn node_frame(editor: &EditorMap, index: usize) -> (Vec2, Vec2) {
    let map = &editor.data;
    let count = map.nodes.len();
    let node = map.nodes[index];
    let out = to_world(node.out_handle);
    let into = -to_world(node.in_handle);
    let mut tangent = out + into;
    if tangent.length_squared() < 1e-6 {
        let next = to_world(map.nodes[(index + 1) % count].position);
        let previous = to_world(map.nodes[(index + count - 1) % count].position);
        tangent = next - previous;
    }
    let tangent = tangent.normalize_or_zero();
    (tangent, Vec2::new(-tangent.y, tangent.x))
}

/// Where the two road-edge handles for a node sit.
pub fn width_handles(editor: &EditorMap, index: usize) -> [(Vec2, f32); 2] {
    let position = to_world(editor.data.nodes[index].position);
    let (_, normal) = node_frame(editor, index);
    let half = editor.data.node_half_width(index);
    [
        (position + normal * half, 1.0),
        (position - normal * half, -1.0),
    ]
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
        let mut distance = at
            .distance(position)
            .min(at.distance(position + to_world(node.in_handle)))
            .min(at.distance(position + to_world(node.out_handle)));
        for (edge, _) in width_handles(editor, index) {
            distance = distance.min(at.distance(edge));
        }
        if distance <= reach && best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, index));
        }
    }
    best.map(|(_, index)| index)
}

fn find_grab(
    editor: &EditorMap,
    at: Vec2,
    radius: f32,
    focus: Option<usize>,
    tool: Tool,
) -> Option<Grab> {
    let map = &editor.data;
    let mut best: Option<(f32, Grab)> = None;
    let mut consider = |distance: f32, grab: Grab| {
        if distance <= radius && best.as_ref().is_none_or(|(d, _)| distance < *d) {
            best = Some((distance, grab));
        }
    };

    // Item boxes are grabbable in both tools; everything else belongs to Edit.
    for (index, position) in editor.built.item_boxes.iter().enumerate() {
        consider(at.distance(*position), Grab::ItemBox(index));
    }
    if tool == Tool::Items {
        return best.map(|(_, grab)| grab);
    }

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
        for (edge, side) in width_handles(editor, index) {
            consider(at.distance(edge), Grab::Width(index, side));
        }
    }
    for (index, node) in map.nodes.iter().enumerate() {
        consider(at.distance(to_world(node.position)), Grab::Node(index));
    }
    if let Some(start) = editor.built.centre.first() {
        consider(at.distance(start.position), Grab::StartLine);
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
    selection.focus = if cursor.over_ui || *tool == Tool::Items {
        None
    } else {
        focus_node(&editor, at, FOCUS_PX * cursor.world_per_px)
    };
    let focus = selection.focus;

    // What a click would grab, every frame, so the overlay can say so.
    selection.hovered = if cursor.over_ui {
        None
    } else {
        find_grab(&editor, at, radius, focus, *tool).map(|grab| match grab {
            Grab::Node(i) => Hovered::Node(i),
            Grab::HandleIn(i) => Hovered::HandleIn(i),
            Grab::HandleOut(i) => Hovered::HandleOut(i),
            Grab::Width(i, side) => Hovered::Width(i, side),
            Grab::StartLine => Hovered::StartLine,
            Grab::ItemBox(i) => Hovered::ItemBox(i),
        })
    };

    if buttons.just_released(MouseButton::Left) || buttons.just_released(MouseButton::Middle) {
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
        let grabbed = find_grab(&editor, at, radius, focus, *tool);
        // One undo step per drag, taken as it starts. The drag itself writes the
        // map every frame it moves and records none of them.
        if grabbed.is_some() {
            history.push(editor.data.clone());
        }
        match grabbed {
            Some(Grab::Node(index)) => {
                selection.node = Some(index);
                let offset = to_world(editor.data.nodes[index].position) - at;
                *drag = Drag::Node(index, offset);
            }
            Some(Grab::HandleIn(index)) => {
                let node = editor.data.nodes[index];
                let tip = to_world(node.position) + to_world(node.in_handle);
                *drag = Drag::HandleIn(index, tip - at);
            }
            Some(Grab::HandleOut(index)) => {
                let node = editor.data.nodes[index];
                let tip = to_world(node.position) + to_world(node.out_handle);
                *drag = Drag::HandleOut(index, tip - at);
            }
            Some(Grab::Width(index, side)) => {
                selection.node = Some(index);
                let edge = width_handles(&editor, index)
                    .into_iter()
                    .find(|(_, s)| *s == side)
                    .map(|(point, _)| point)
                    .unwrap_or(at);
                *drag = Drag::Width(index, side, edge - at);
            }
            Some(Grab::StartLine) => {
                let start = editor.built.start_pose.position;
                *drag = Drag::StartLine(start - at);
            }
            Some(Grab::ItemBox(index)) => {
                selection.item_box = Some(index);
                let offset = editor.built.item_boxes[index] - at;
                *drag = Drag::ItemBox(index, offset);
            }
            None => match *tool {
                // In item mode a click on empty road drops a box; otherwise the
                // most common gesture on empty space is to pan, so that is what
                // it does.
                Tool::Items => {
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
                Tool::Edit => *drag = Drag::Pan(at),
            },
        }
    }

    // Right-click: insert a node on the road, or remove an item box.
    if buttons.just_pressed(MouseButton::Right) && !cursor.over_ui {
        match find_grab(&editor, at, radius, focus, *tool) {
            Some(Grab::ItemBox(index)) => {
                editor.edit(&mut history, |map| {
                    map.item_boxes.remove(index);
                });
                selection.item_box = None;
                status.say("Item box removed.");
            }
            _ if *tool == Tool::Edit => {
                if let Some((anchor, distance)) = nearest_anchor(&editor, at)
                    && distance < road_reach(&editor)
                {
                    insert_node(&mut editor, &mut history, anchor);
                    selection.node = Some(anchor.segment as usize + 1);
                    status.say("Node inserted.");
                }
            }
            _ => {}
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
        Drag::Node(index, offset) => {
            let moved = at + offset;
            let target = if snap { moved.round() } else { moved };
            editor.edit_in_progress(|map| {
                if let Some(node) = map.nodes.get_mut(index) {
                    node.position = to_map(target);
                }
            });
        }
        Drag::HandleIn(index, offset) | Drag::HandleOut(index, offset) => {
            let outgoing = matches!(*drag, Drag::HandleOut(i, _) if i == index);
            let tip = at + offset;
            editor.edit_in_progress(|map| {
                let Some(node) = map.nodes.get_mut(index) else {
                    return;
                };
                let handle = tip - to_world(node.position);
                if break_mirror {
                    node.mirrored = false;
                }
                if outgoing {
                    node.out_handle = to_map(handle);
                    if node.mirrored {
                        node.in_handle = to_map(mirror(handle, to_world(node.in_handle)));
                    }
                } else {
                    node.in_handle = to_map(handle);
                    if node.mirrored {
                        node.out_handle = to_map(mirror(handle, to_world(node.out_handle)));
                    }
                }
            });
        }
        Drag::Width(index, side, offset) => {
            // How far the dragged edge now sits from the node, measured across
            // the road rather than as a straight distance -- so sliding along the
            // road does not change the width.
            let (_, normal) = node_frame(&editor, index);
            let across = ((at + offset) - to_world(editor.data.nodes[index].position)).dot(normal);
            let half = (across * side).max(0.5);
            editor.edit_in_progress(|map| {
                map.nodes[index].half_width = Some(scalar_to_map(half));
            });
        }
        Drag::StartLine(offset) => {
            if let Some((anchor, _)) = nearest_anchor(&editor, at + offset) {
                editor.edit_in_progress(|map| {
                    // The line spans the road, so only where it sits along the
                    // lap is editable -- never how far across.
                    map.start.at = TrackAnchor::new(anchor.segment, anchor.t_fraction(), 0);
                });
            }
        }
        Drag::ItemBox(index, offset) => {
            if let Some((anchor, _)) = nearest_anchor(&editor, at + offset) {
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
        && let Some(index) = selection.node
    {
        {
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
        && let Some(index) = selection.node
    {
        editor.edit(&mut history, |map| map.nodes[index].half_width = None);
        status.say("Node width follows the map again.");
    }

    if keys.just_pressed(KeyCode::KeyF)
        && let Ok((mut transform, mut projection)) = camera.single_mut()
    {
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

    if scroll.delta.y != 0.0
        && !cursor.over_ui
        && let Projection::Orthographic(orthographic) = projection.as_mut()
    {
        let before = at;
        let ratio = 1.1f32.powf(-scroll.delta.y);
        orthographic.scale = (orthographic.scale * ratio).clamp(MIN_ZOOM, MAX_ZOOM);
        // Correct the translation so the world point under the cursor is the
        // one still under it after the zoom.
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

    /// The protocol a drag follows: record once as it starts, then write the map
    /// on every frame it moves without recording any of those.
    ///
    /// Recording each frame is what it used to do, and it filled a sixty-four
    /// deep history in about a second -- so undo meant "a moment ago" rather than
    /// "before that drag", which is the only thing anybody wants it to mean.
    #[test]
    fn a_drag_is_one_undo_step_however_far_it_moves() {
        let mut editor = editor_for(circle(80.0, 11.0));
        let mut history = History::default();
        let before = to_world(editor.data.nodes[1].position);

        // Grab.
        history.push(editor.data.clone());
        // Sixty frames of movement.
        for frame in 0..60 {
            let moved = before + Vec2::new(frame as f32 * 0.5, 0.0);
            editor.edit_in_progress(|map| map.nodes[1].position = to_map(moved));
        }
        assert_eq!(history.past.len(), 1, "a drag is one step, not sixty");
        assert!(editor.dirty);

        let current = editor.data.clone();
        editor.data = history.undo(&current).unwrap();
        assert_eq!(
            to_world(editor.data.nodes[1].position),
            before,
            "undo goes back to before the drag, not to part-way through it"
        );
    }

    /// Width is dragged on the road edge, and the edge has to be where the road
    /// actually is or the grip sits in the grass.
    #[test]
    fn the_width_grips_sit_on_the_road_edges() {
        let mut map = circle(80.0, 11.0);
        map.nodes[2].half_width = Some(scalar_to_map(6.0));
        let editor = editor_for(map);
        for index in 0..editor.data.nodes.len() {
            let centre = to_world(editor.data.nodes[index].position);
            let half = editor.data.node_half_width(index);
            let [(left, left_side), (right, right_side)] = width_handles(&editor, index);
            assert_eq!((left_side, right_side), (1.0, -1.0));
            assert!(
                (centre.distance(left) - half).abs() < 1e-3,
                "node {index}: grip is {} from the centre, half-width is {half}",
                centre.distance(left)
            );
            assert!((centre.distance(right) - half).abs() < 1e-3);
            // And on opposite sides of it.
            assert!((left - centre).dot(right - centre) < 0.0);
        }
    }

    /// The two tools have to differ, or the selector is decoration. They did not:
    /// of the three there used to be, two behaved identically.
    #[test]
    fn the_tools_grab_different_things() {
        let mut map = circle(80.0, 11.0);
        // Well away from node 0, or the two are the same point and the test is
        // asking which of two things at one place is nearest.
        map.item_boxes = vec![TrackAnchor::new(2, 0.5, 0)];
        let editor = editor_for(map);
        let node = to_world(editor.data.nodes[0].position);

        // A node is grabbable while editing the track, and not while placing items.
        assert!(matches!(
            find_grab(&editor, node, 2.0, Some(0), Tool::Edit),
            Some(Grab::Node(0) | Grab::HandleIn(0) | Grab::HandleOut(0) | Grab::Width(0, _))
        ));
        assert!(
            find_grab(&editor, node, 2.0, Some(0), Tool::Items).is_none(),
            "the track is left alone in Items, so a click cannot move it by accident"
        );

        // An item box is grabbable in both.
        let box_at = editor.built.item_boxes[0];
        assert!(matches!(
            find_grab(&editor, box_at, 2.0, None, Tool::Items),
            Some(Grab::ItemBox(0))
        ));
        assert!(matches!(
            find_grab(&editor, box_at, 2.0, None, Tool::Edit),
            Some(Grab::ItemBox(0))
        ));
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
