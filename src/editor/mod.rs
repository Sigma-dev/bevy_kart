//! The level editor.
//!
//! Edits a [`MapData`] directly and previews it through the very same builder
//! the race uses, so what is drawn here cannot drift from what is raced. The
//! editor owns no geometry of its own.

use bevy::prelude::*;

use crate::track::map::build::{BuildLevel, BuiltTrack, build};
use crate::track::map::builtin::default_map;
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
    /// An empty-ish track to start from: the default map, renamed, so a new
    /// track begins as something raceable rather than as three nodes in a line.
    pub fn fresh() -> Self {
        let mut data = default_map();
        data.name = "New Track".to_string();
        Self::from_map(data, None)
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
}

/// What the pointer does on the canvas.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    #[default]
    Nodes,
    StartLine,
    ItemBoxes,
}

impl Tool {
    pub fn next(self) -> Self {
        match self {
            Tool::Nodes => Tool::StartLine,
            Tool::StartLine => Tool::ItemBoxes,
            Tool::ItemBoxes => Tool::Nodes,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Nodes => "NODES",
            Tool::StartLine => "START",
            Tool::ItemBoxes => "ITEMS",
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
    selected: Option<Res<crate::track::SelectedMap>>,
) {
    // Whatever was being edited, if the editor is being re-entered; otherwise the
    // map that was about to be raced, so opening the editor from the menu starts
    // on the track the player was just looking at rather than always on the
    // default one.
    let map = existing
        .map(|editor| editor.data.clone())
        .or_else(|| selected.map(|selected| selected.0.clone()))
        .unwrap_or_else(default_map);
    let editor = EditorMap::from_map(map, None);

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
    preview: Option<Res<PreviewMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if !editor.is_changed() {
        return;
    }
    let Some(preview) = preview else { return };
    editor.built = build(&editor.data, BuildLevel::Full);
    if let Some(mut mesh) = meshes.get_mut(&preview.0) {
        *mesh = crate::track::map::mesh::road_mesh(&editor.built);
    }
}

/// Leave the editor, which the computed `Screen` turns into a screen change.
pub fn close_editor(next: &mut NextState<EditorState>) {
    next.set(EditorState::Closed);
}
