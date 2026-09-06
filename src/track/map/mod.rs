//! Tracks as data: the format, and the geometry derived from it.

pub mod build;
pub mod builtin;
pub mod data;
pub mod mesh;

pub use build::{BuildLevel, BuiltTrack, Pose, Sample, TrackWarning, build};
pub use builtin::{BUILTINS, by_slug, default_map};
pub use mesh::{road_mesh, start_line_mesh};
pub use data::{
    DecorSettings, GridLayout, MAP_FORMAT_VERSION, MapData, MapError, RoadShape, StartLine,
    TrackAnchor, TrackNode,
};
