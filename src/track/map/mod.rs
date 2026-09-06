//! Tracks as data: the format, and the geometry derived from it.

pub mod build;
pub mod builtin;
pub mod data;
pub mod file;
pub mod mesh;
pub mod scatter;
pub mod share;
pub mod starter;
pub mod store;

pub use build::{BuildLevel, BuiltTrack, Pose, Sample, TrackWarning, build};
pub use builtin::{BUILTINS, by_slug, default_map};
pub use mesh::{road_mesh, start_line_mesh};
pub use starter::starter_map;
pub use data::{
    DecorSettings, GridLayout, MAP_FORMAT_VERSION, MapData, MapError, RoadShape, StartLine,
    TrackAnchor, TrackNode,
};
