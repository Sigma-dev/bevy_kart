//! The level editor.
//!
//! Edits a [`MapData`] directly and previews it through the very same builder
//! the race uses, so what is drawn here cannot drift from what is raced. The
//! editor owns no geometry of its own.

use bevy::prelude::*;

use crate::track::map::build::{BuildLevel, BuiltTrack, build};
use crate::track::map::starter::starter_map;
use crate::track::map::data::MapData;
use crate::track::map::file::PendingImport;
use crate::{EditorState, Screen};

pub mod cursor;
pub mod draw;
pub mod panel;
pub mod tools;

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingImport>()
            .init_resource::<Status>()
            // Chained: `spawn_panel` reads the `EditorMap` that `enter_editor`
            // inserts through `Commands`, which are not applied until a sync
            // point -- and an ordering is what puts one between them.
            .add_systems(
                OnEnter(Screen::Editor),
                (enter_editor, panel::spawn_panel).chain(),
            )
            .add_systems(OnExit(Screen::Editor), leave_editor)
            .add_systems(
                PreUpdate,
                cursor::track_cursor.run_if(in_state(Screen::Editor)),
            )
            .add_systems(
                Update,
                (
                    (
                        tools::handle_pointer,
                        tools::handle_keys,
                        tools::pan_and_zoom,
                    )
                        .chain(),
                    rebuild_preview,
                    draw::draw_overlay,
                    panel::run_panel,
                )
                    .chain()
                    .run_if(in_state(Screen::Editor)),
            );
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(
            Update,
            crate::track::map::file::accept_dropped_files.run_if(in_state(Screen::Editor)),
        );
    }
}

/// The map being edited, and what is known about it.
#[derive(Resource)]
pub struct EditorMap {
    pub data: MapData,
    /// Changed since it was last saved.
    pub dirty: bool,
    /// The storage id it came from, if it came from storage.
    pub source: Option<String>,
    /// The geometry, rebuilt whenever `data` changes.
    pub built: BuiltTrack,
}

impl EditorMap {
    /// A track to start from: a small oval, raceable as it stands, simple enough
    /// to read in one look and to be worth changing rather than deleting. See
    /// [`starter_map`].
    pub fn fresh() -> Self {
        Self::from_map(starter_map(), None)
    }

    pub fn from_loaded(data: MapData, source: Option<String>) -> Self {
        Self::from_map(data, source)
    }

    fn from_map(data: MapData, source: Option<String>) -> Self {
        let built = build(&data, BuildLevel::Full);
        Self {
            data,
            dirty: false,
            source,
            built,
        }
    }

    /// Edit the map, recording an undo step and marking it unsaved.
    ///
    /// Everything that changes the map goes through here, which is what makes
    /// "can I undo that" a property of the type rather than a thing each tool has
    /// to remember.
    pub fn edit(&mut self, history: &mut History, change: impl FnOnce(&mut MapData)) {
        history.push(self.data.clone());
        change(&mut self.data);
        self.dirty = true;
    }

    /// Edit without recording anything, for a change still under the cursor.
    ///
    /// A drag writes the map on every frame it moves. Recording each of those
    /// would fill a sixty-four deep history in about a second and make undo mean
    /// "a moment ago" instead of "before that drag" -- so the drag records once,
    /// when it starts, and then uses this.
    pub fn edit_in_progress(&mut self, change: impl FnOnce(&mut MapData)) {
        change(&mut self.data);
        self.dirty = true;
    }
}

/// What the pointer does on the canvas.
///
/// Two, not three. There used to be a `StartLine` mode as well, and it changed
/// nothing at all: the start line has always been draggable in every mode, so
/// selecting it did precisely nothing and made the other two look inert by
/// association.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// Nodes, handles, road width, the start line.
    #[default]
    Edit,
    /// Item boxes only, with the track itself left alone so a click cannot move
    /// a node when it meant to drop a box.
    Items,
}

impl Tool {
    pub fn next(self) -> Self {
        match self {
            Tool::Edit => Tool::Items,
            Tool::Items => Tool::Edit,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Edit => "EDIT",
            Tool::Items => "ITEMS",
        }
    }
}

/// Undo, as whole copies of the map.
///
/// A `MapData` is a few hundred bytes and `Clone`, so a ring of them is far
/// simpler than a command log and cannot get out of step with the thing it is
/// meant to reverse.
#[derive(Resource, Default)]
pub struct History {
    pub(crate) past: Vec<MapData>,
    future: Vec<MapData>,
}

pub(crate) const HISTORY_DEPTH: usize = 64;

impl History {
    pub fn push(&mut self, previous: MapData) {
        self.past.push(previous);
        if self.past.len() > HISTORY_DEPTH {
            self.past.remove(0);
        }
        // A fresh edit is a new branch: whatever was undone is not coming back.
        self.future.clear();
    }

    pub fn undo(&mut self, current: &MapData) -> Option<MapData> {
        let previous = self.past.pop()?;
        self.future.push(current.clone());
        Some(previous)
    }

    pub fn redo(&mut self, current: &MapData) -> Option<MapData> {
        let next = self.future.pop()?;
        self.past.push(current.clone());
        Some(next)
    }

    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }
}

/// A line of text under the panel, saying what just happened.
#[derive(Resource, Default)]
pub struct Status(pub String);

impl Status {
    pub fn say(&mut self, message: impl Into<String>) {
        self.0 = message.into();
    }
}

/// What the editor draws the road with. Held so the mesh can be edited in place
/// rather than replaced every frame.
#[derive(Resource)]
pub struct PreviewMesh(pub Handle<Mesh>);

fn enter_editor(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    existing: Option<Res<EditorMap>>,
    mut camera: Query<(&mut Transform, &mut Projection), With<crate::camera::MainCamera>>,
) {
    // Whatever was being edited, if the editor is being re-entered; otherwise a
    // new track. It used to open on the map that was about to be raced, which
    // meant the editor's front door was somebody else's forty-eight-node circuit
    // and a first edit began by deleting it. The track list in the panel is how
    // you reach an existing map -- including that one.
    //
    // The storage id comes back across too, so leaving the editor and returning
    // still knows which saved map is being edited and SAVE still overwrites it
    // rather than making a second copy.
    let (map, source) = existing
        .map(|editor| (editor.data.clone(), editor.source.clone()))
        .unwrap_or_else(|| (starter_map(), None));
    let editor = EditorMap::from_map(map, source);

    let mesh = meshes.add(crate::track::map::mesh::road_mesh(&editor.built));
    commands.spawn((
        DespawnOnExit(Screen::Editor),
        Mesh2d(mesh.clone()),
        // White, because the road carries its colours per vertex and the shader
        // multiplies the two.
        MeshMaterial2d(materials.add(ColorMaterial::from(Color::WHITE))),
        Transform::from_xyz(0., 0., crate::SpriteLayers::Background.to_z()),
    ));
    commands.insert_resource(PreviewMesh(mesh));
    // Frame it on the way in. The camera is shared with the race and the menus,
    // so without this the editor opens wherever the last thing left it -- which
    // for a map larger than a screen is often looking at empty grass.
    if let Ok((mut transform, mut projection)) = camera.single_mut() {
        tools::frame_map(&editor, &mut transform, &mut projection);
    }
    commands.insert_resource(editor);
    commands.init_resource::<History>();
    commands.init_resource::<Tool>();
    commands.init_resource::<tools::Drag>();
    commands.init_resource::<tools::Selection>();
    commands.init_resource::<cursor::EditorCursor>();
}

fn leave_editor(mut commands: Commands, mut camera: Query<&mut Projection>) {
    commands.remove_resource::<PreviewMesh>();
    // The camera is shared with the race and the menus, so hand it back at 1:1.
    for mut projection in camera.iter_mut() {
        if let Projection::Orthographic(orthographic) = projection.as_mut() {
            orthographic.scale = 1.0;
        }
    }
}

/// Rebuild the geometry, but only what is cheap, and only when something changed.
///
/// The road mesh is mutated **in place** rather than replaced: `meshes.add`
/// every frame while a handle is being dragged leaks a mesh per frame. The walls
/// and the scenery are not rebuilt as entities at all here -- the overlay draws
/// them as gizmos, because respawning a couple of hundred static bodies at sixty
/// hertz would have avian rebuilding its broadphase sixty times a second.
fn rebuild_preview(
    mut editor: ResMut<EditorMap>,
    drag: Option<Res<tools::Drag>>,
    preview: Option<Res<PreviewMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if !editor.is_changed() {
        return;
    }
    let Some(preview) = preview else { return };
    // `Full` runs the shape checks, one of which compares every centreline sample
    // against every other -- about eighty thousand pairs on a track this size.
    // Fine once when a drag settles; not fine sixty times a second while one is
    // still moving.
    let level = match drag.as_deref() {
        Some(tools::Drag::None) | None => BuildLevel::Full,
        Some(_) => BuildLevel::Preview,
    };
    editor.built = build(&editor.data, level);
    if let Some(mut mesh) = meshes.get_mut(&preview.0) {
        *mesh = crate::track::map::mesh::road_mesh(&editor.built);
    }
}

/// Leave the editor, which the computed `Screen` turns into a screen change.
pub fn close_editor(next: &mut NextState<EditorState>) {
    next.set(EditorState::Closed);
}
