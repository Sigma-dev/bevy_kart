use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_ensemble::prelude::{Lobby, NetDebugExtras, PeerRtt};
use bevy_ticked_networking::prelude::{ClientTickBuffer, LocalClientPlayer};

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_fps_text)
            .add_systems(Update, (update_fps, cursor_position_log, report_tick_buffer));
    }
}

#[derive(Component)]
#[require(Text)]
struct FpsText {
    current: f64,
}

fn spawn_fps_text(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(5),
            ..default()
        },
        FpsText { current: 0.0 },
    ));
}

fn update_fps(time: Res<Time>, mut texts: Query<(&mut Text, &mut FpsText)>) {
    let delta = time.delta_secs_f64();
    if delta <= 0.0 || delta.is_nan() {
        return;
    }
    let current = 1.0 / delta;
    if current.is_infinite() || current.is_nan() {
        return;
    }
    for (mut text, mut fps) in texts.iter_mut() {
        fps.current = fps.current * 0.9 + current * 0.1;
        *text = Text::new(format!("FPS: {:.0}", fps.current));
    }
}

/// Show the adaptive prediction lead in the F3 net-debug overlay (clients only).
fn report_tick_buffer(
    buffer: Option<Res<ClientTickBuffer>>,
    local_client: Option<Res<LocalClientPlayer>>,
    rtt: Query<&PeerRtt, With<Lobby>>,
    extras: Option<ResMut<NetDebugExtras>>,
) {
    let Some(mut extras) = extras else {
        return;
    };
    match (local_client.is_some(), buffer) {
        (true, Some(buffer)) => {
            let ping = rtt.iter().next().map(|r| r.0 * 1000.0).unwrap_or(0.0);
            extras.set(
                "prediction",
                format!("prediction lead: {} ticks ({ping:.0}ms ping)", buffer.target_ticks),
            );
        }
        _ => extras.remove("prediction"),
    }
}

fn cursor_position_log(
    q_window: Query<&Window, With<PrimaryWindow>>,
    q_camera: Query<(&Camera, &GlobalTransform)>,
    button_input: Res<ButtonInput<MouseButton>>,
) {
    let (camera, camera_transform) = q_camera.single().unwrap();
    let window = q_window.single().unwrap();
    if let Some(world_position) = window
        .cursor_position()
        .and_then(|cursor| Some(camera.viewport_to_world(camera_transform, cursor)))
        .map(|ray| ray.unwrap().origin.truncate())
    {
        if button_input.just_pressed(MouseButton::Left) {
            info!(
                "World coords: Vec2::new({},{})",
                world_position.x, world_position.y
            );
        }
    }
}
