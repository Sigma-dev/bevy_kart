use avian2d::prelude::{LinearVelocity, Position};
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::Screen;
use crate::camera::MainCamera;
use bevy_ensemble::prelude::{Lobby, NetDebugExtras, PeerRtt, PeerRttJitter};
use bevy_ticked::prelude::{CurrentTick, TickRateDilation, TickedLoop, TickedSystems};
use bevy_ticked_networking::client::SnapshotApplied;
use bevy_ticked_networking::prelude::{ClientTickBuffer, LocalClientPlayer, LocalServerPlayer};

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_fps_text)
            .add_systems(
                Update,
                (
                    update_fps,
                    // This logs a line on every left click. In the editor every
                    // click is a drag on a node or a handle, which would bury
                    // every line that is trying to explain something.
                    cursor_position_log.run_if(not(in_state(Screen::Editor))),
                    report_tick_buffer,
                ),
            )
            .init_resource::<PerfStats>()
            .add_systems(First, perf_frame_begin)
            .add_systems(
                TickedLoop,
                (
                    perf_tick_begin.before(TickedSystems::PreTick),
                    perf_tick_end.after(TickedSystems::PostTick),
                ),
            )
            .add_systems(Update, perf_count_replay)
            .add_systems(Last, perf_frame_end);
    }
}

/// Where a frame's CPU time goes, sampled over a two second window.
///
/// `main` is the busy time of the main schedule, first system to last, which
/// is what the simulation, rollback and UI cost; on the web it is the number
/// that matters, because the frame itself is pinned to the display by
/// `requestAnimationFrame` and reads 16.7 ms no matter what. `tick` is the part
/// of `main` spent inside the tick loop, so `main - tick` is everything else,
/// UI layout included. `replay` is how many ticks each snapshot made the
/// client re-simulate.
#[derive(Resource, Default)]
struct PerfStats {
    frame_start: Option<Instant>,
    tick_start: Option<Instant>,
    window_start: Option<Instant>,
    frames: u32,
    frame_ms_sum: f64,
    frame_ms_max: f64,
    main_ms_sum: f64,
    main_ms_max: f64,
    tick_ms_frame: f64,
    tick_ms_sum: f64,
    tick_ms_max: f64,
    ticks: u32,
    snapshots: u32,
    replay_ticks: u64,
}

const PERF_WINDOW_SECS: f64 = 2.0;

fn perf_frame_begin(mut stats: ResMut<PerfStats>) {
    let now = Instant::now();
    if let Some(prev) = stats.frame_start {
        let ms = now.duration_since(prev).as_secs_f64() * 1000.0;
        stats.frame_ms_sum += ms;
        stats.frame_ms_max = stats.frame_ms_max.max(ms);
    }
    stats.frame_start = Some(now);
    stats.tick_ms_frame = 0.0;
    if stats.window_start.is_none() {
        stats.window_start = Some(now);
    }
}

fn perf_tick_begin(mut stats: ResMut<PerfStats>) {
    stats.tick_start = Some(Instant::now());
}

fn perf_tick_end(mut stats: ResMut<PerfStats>) {
    if let Some(start) = stats.tick_start.take() {
        stats.tick_ms_frame += start.elapsed().as_secs_f64() * 1000.0;
        stats.ticks += 1;
    }
}

fn perf_count_replay(
    mut applied: MessageReader<SnapshotApplied>,
    tick: Res<CurrentTick>,
    mut stats: ResMut<PerfStats>,
) {
    for snapshot in applied.read() {
        stats.snapshots += 1;
        // The tick loop advanced once more after the replay, hence the `+ 1`.
        stats.replay_ticks += tick.0.saturating_sub(snapshot.tick + 1);
    }
}

fn perf_frame_end(
    mut stats: ResMut<PerfStats>,
    server: Option<Res<LocalServerPlayer>>,
    client: Option<Res<LocalClientPlayer>>,
    extras: Option<ResMut<NetDebugExtras>>,
    params: Option<Res<crate::lobby::SessionParams>>,
    local_kart: Query<(&Position, &LinearVelocity), With<crate::kart::LocalKart>>,
) {
    let now = Instant::now();
    let Some(frame_start) = stats.frame_start else { return };
    let main_ms = now.duration_since(frame_start).as_secs_f64() * 1000.0;
    stats.frames += 1;
    stats.main_ms_sum += main_ms;
    stats.main_ms_max = stats.main_ms_max.max(main_ms);
    let tick_ms = stats.tick_ms_frame;
    stats.tick_ms_sum += tick_ms;
    stats.tick_ms_max = stats.tick_ms_max.max(tick_ms);

    let Some(window_start) = stats.window_start else { return };
    let elapsed = now.duration_since(window_start).as_secs_f64();
    if elapsed < PERF_WINDOW_SECS {
        return;
    }
    let frames = stats.frames.max(1) as f64;
    let role = if server.is_some() {
        "host"
    } else if client.is_some() {
        "client"
    } else {
        "solo"
    };
    let kart = local_kart
        .single()
        .map(|(pos, vel)| format!("kart=({:.1},{:.1}) speed={:.1}", pos.x, pos.y, vel.length()))
        .unwrap_or_else(|_| "kart=none".to_string());
    let line = format!(
        "PERF role={role} fps={:.1} frame_ms avg={:.2} max={:.2} | main_ms avg={:.2} max={:.2} \
         | tick_ms avg={:.2} max={:.2} ticks/frame={:.2} | replay/snapshot={:.1} snapshots/s={:.1} | {kart}",
        frames / elapsed,
        stats.frame_ms_sum / frames,
        stats.frame_ms_max,
        stats.main_ms_sum / frames,
        stats.main_ms_max,
        stats.tick_ms_sum / frames,
        stats.tick_ms_max,
        stats.ticks as f64 / frames,
        stats.replay_ticks as f64 / stats.snapshots.max(1) as f64,
        stats.snapshots as f64 / elapsed,
    );
    // Only when asked. The readout is one line every two seconds, for ever, and an
    // unasked-for line every two seconds is what a browser console looks like when
    // something else is trying to tell you why a join failed. `warn` because that is
    // the only level a release build keeps (see `SessionParams::perf`); the overlay
    // below gets the same numbers either way, which is the better way to read them.
    if params.is_some_and(|p| p.perf) {
        warn!("{line}");
    }
    if let Some(mut extras) = extras {
        extras.set("perf", line);
    }
    *stats = PerfStats {
        frame_start: Some(frame_start),
        window_start: Some(now),
        ..default()
    };
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

/// Show the adaptive replay distance in the F3 net-debug overlay (clients only).
///
/// Not the prediction lead, which this used to claim: a snapshot is one one-way
/// trip old when it is read, so the lead is this minus that. What actually
/// matters is `target_margin`, the ticks of headroom the client is trying to
/// leave for an unlucky packet, which the transport now sizes from measured
/// jitter -- so show that next to the jitter it was sized from.
fn report_tick_buffer(
    buffer: Option<Res<ClientTickBuffer>>,
    dilation: Option<Res<TickRateDilation>>,
    local_client: Option<Res<LocalClientPlayer>>,
    rtt: Query<(&PeerRtt, Option<&PeerRttJitter>), With<Lobby>>,
    extras: Option<ResMut<NetDebugExtras>>,
) {
    let Some(mut extras) = extras else {
        return;
    };
    match (local_client.is_some(), buffer) {
        (true, Some(buffer)) => {
            // The transport measures the spread as well as the mean; the mean
            // says where packets land on average and the spread says how late
            // the unlucky ones are, which is what the lead has to cover.
            let (ping, jitter) = rtt
                .iter()
                .next()
                .map(|(rtt, jitter)| (rtt.0 * 1000.0, jitter.map_or(0.0, |j| j.0 * 1000.0)))
                .unwrap_or((0.0, 0.0));
            // The dilation is how hard the client is currently steering toward
            // that lead; sustained non-zero drift is the signal that the
            // controller is hunting rather than settling.
            let drift = dilation.map(|d| (d.0 - 1.0) * 100.0).unwrap_or(0.0);
            extras.set(
                "prediction",
                format!(
                    "replay: {} ticks, margin {} ({ping:.0}ms ping, \u{b1}{jitter:.0}ms jitter, {drift:+.1}% rate)",
                    buffer.target_replay_distance, buffer.target_margin
                ),
            );
        }
        _ => extras.remove("prediction"),
    }
}

fn cursor_position_log(
    q_window: Query<&Window, With<PrimaryWindow>>,
    // Filtered to the player's camera: the minimap is a second one, and an
    // unfiltered `single()` panics the moment it exists.
    q_camera: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    button_input: Res<ButtonInput<MouseButton>>,
) {
    let (Ok((camera, camera_transform)), Ok(window)) = (q_camera.single(), q_window.single())
    else {
        return;
    };
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
