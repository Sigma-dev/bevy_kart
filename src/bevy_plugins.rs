use bevy::a11y::AccessibilityPlugin;
use bevy::app::{PanicHandlerPlugin, TaskPoolPlugin};
use bevy::asset::AssetMetaCheck;
use bevy::audio::AudioPlugin;
use bevy::camera::CameraPlugin;
use bevy::core_pipeline::CorePipelinePlugin;
use bevy::diagnostic::{DiagnosticsPlugin, FrameCountPlugin};
use bevy::gizmos::GizmoPlugin;
use bevy::input::InputPlugin;
use bevy::input_focus::{InputDispatchPlugin, InputFocusPlugin};
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
use bevy::ui_widgets::EditableTextInputPlugin;
use bevy::winit::WinitPlugin;

/// Replaces DefaultPlugins with only the Bevy plugins this game actually needs.
pub struct NecessaryBevyPlugins;

impl Plugin for NecessaryBevyPlugins {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            PanicHandlerPlugin,
            LogPlugin {
                // `webrtc_ice` warns a dozen times per connection about link-local
                // IPv6 addresses it cannot bind and STUN hosts it cannot resolve;
                // none of it means anything, and it buries the lines that do.
                // The directives are the transport crate's own, so they follow its
                // dependencies rather than a copy of their module paths kept here.
                filter: format!(
                    "{},{}",
                    bevy::log::DEFAULT_FILTER,
                    bevy_ensemble_webrtc::QUIET_ICE_LOG_FILTER
                ),
                ..default()
            },
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
        // Focus tracking + keyboard dispatch + the built-in editable text widget
        // (these come from DefaultPlugins normally; add them explicitly here).
        app.add_plugins((
            InputFocusPlugin,
            InputDispatchPlugin,
            EditableTextInputPlugin,
        ));
    }
}
