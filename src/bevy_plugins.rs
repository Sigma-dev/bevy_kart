use bevy::a11y::AccessibilityPlugin;
use bevy::app::{PanicHandlerPlugin, TaskPoolPlugin};
use bevy::asset::AssetMetaCheck;
use bevy::audio::AudioPlugin;
use bevy::camera::CameraPlugin;
use bevy::core_pipeline::CorePipelinePlugin;
use bevy::diagnostic::{DiagnosticsPlugin, FrameCountPlugin};
use bevy::gizmos::GizmoPlugin;
use bevy::input::InputPlugin;
use bevy::log::LogPlugin;
use bevy::mesh::MeshPlugin;
use bevy::picking::{InteractionPlugin, PickingPlugin, input::PointerInputPlugin};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::scene::ScenePlugin;
use bevy::sprite::SpritePlugin;
use bevy::sprite_render::SpriteRenderPlugin;
use bevy::state::app::StatesPlugin;
use bevy::text::TextPlugin;
use bevy::time::TimePlugin;
use bevy::transform::TransformPlugin;
use bevy::ui::UiPlugin;
use bevy::ui_render::UiRenderPlugin;
use bevy::winit::WinitPlugin;

/// Replaces DefaultPlugins with only the Bevy plugins this game actually needs.
pub struct NecessaryBevyPlugins;

impl Plugin for NecessaryBevyPlugins {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            PanicHandlerPlugin,
            LogPlugin::default(),
            TaskPoolPlugin::default(),
            FrameCountPlugin,
            TimePlugin,
            TransformPlugin,
            DiagnosticsPlugin,
            InputPlugin,
            WindowPlugin::default(),
            AccessibilityPlugin,
        ));
        app.add_plugins((
            AssetPlugin {
                meta_check: AssetMetaCheck::Never,
                ..default()
            },
            ScenePlugin,
            WinitPlugin::default(),
            RenderPlugin::default(),
            ImagePlugin::default_nearest(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
        ));
        app.add_plugins((
            SpritePlugin,
            SpriteRenderPlugin,
            TextPlugin,
            UiPlugin,
            UiRenderPlugin,
            AudioPlugin::default(),
            GizmoPlugin,
            StatesPlugin,
        ));
        app.add_plugins((PointerInputPlugin, PickingPlugin, InteractionPlugin));
    }
}
