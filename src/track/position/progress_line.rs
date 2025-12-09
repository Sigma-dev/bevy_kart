pub use bevy::prelude::*;

pub struct ProgressLinePlugin;

impl Plugin for ProgressLinePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (draw_progress_line, compute_progress));
    }
}

#[derive(Component)]
pub struct ProgressLine {
    pub points: Vec<Vec2>,
}

#[derive(Component)]
pub struct DrawProgressLine;

#[derive(Component, Default)]
pub struct TrackProgress;

#[derive(Component, Debug)]
pub struct Progress {
    pub progress: f32,
}

impl ProgressLine {
    pub fn new(points: Vec<Vec2>) -> Self {
        Self { points }
    }
    pub fn measure_progress(&self, position: Vec2) -> f32 {
        if self.points.len() < 2 {
            return 0.0;
        }

        let mut min_distance = f32::MAX;
        let mut closest_segment_index = 0;
        let mut closest_point_on_segment = Vec2::ZERO;
        let num_segments = self.points.len();

        // Find the closest segment and point on that segment
        // Include the closing segment from last point back to first
        for i in 0..num_segments {
            let p1 = self.points[i];
            let p2 = self.points[(i + 1) % num_segments];

            // Find closest point on line segment
            let segment_vec = p2 - p1;
            let segment_length_sq = segment_vec.length_squared();

            if segment_length_sq < f32::EPSILON {
                // Degenerate segment (zero length), just use p1
                let dist = position.distance(p1);
                if dist < min_distance {
                    min_distance = dist;
                    closest_segment_index = i;
                    closest_point_on_segment = p1;
                }
                continue;
            }

            // Project position onto the line segment
            let t = ((position - p1).dot(segment_vec) / segment_length_sq).clamp(0.0, 1.0);
            let closest = p1 + t * segment_vec;
            let dist = position.distance(closest);

            if dist < min_distance {
                min_distance = dist;
                closest_segment_index = i;
                closest_point_on_segment = closest;
            }
        }

        // Calculate total length of progress line (including closing segment)
        let total_length: f32 = (0..num_segments)
            .map(|i| {
                let p1 = self.points[i];
                let p2 = self.points[(i + 1) % num_segments];
                p1.distance(p2)
            })
            .sum();

        if total_length < f32::EPSILON {
            return 0.0;
        }

        // Calculate cumulative distance up to the closest segment
        let cumulative_distance: f32 = (0..closest_segment_index)
            .map(|i| {
                let p1 = self.points[i];
                let p2 = self.points[(i + 1) % num_segments];
                p1.distance(p2)
            })
            .sum();

        // Add distance along the closest segment to the closest point
        let segment_start = self.points[closest_segment_index];
        let distance_along_segment = segment_start.distance(closest_point_on_segment);

        (cumulative_distance + distance_along_segment) / total_length
    }
}

fn draw_progress_line(
    mut gizmos: Gizmos,
    progress_line: Single<&ProgressLine, With<DrawProgressLine>>,
) {
    let mut last_point = progress_line.points.last().unwrap().clone();
    for point in progress_line.points.clone() {
        gizmos.line_2d(point, last_point, Color::WHITE);
        last_point = point;
    }
}

fn compute_progress(
    mut commands: Commands,
    progress: Query<(Entity, &Transform), With<TrackProgress>>,
    progress_line: Single<&ProgressLine>,
) {
    for (entity, transform) in progress.iter() {
        let progress = progress_line.measure_progress(transform.translation.xy());
        commands.entity(entity).insert(Progress { progress });
    }
}
