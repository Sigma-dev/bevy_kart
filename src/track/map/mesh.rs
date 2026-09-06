//! The road, as geometry rather than as a painted sprite.
//!
//! One mesh carries the road body and both kerb bands, in a single draw call,
//! by putting the colour on the vertices. That is only sound because of one
//! detail worth stating loudly: `ColorMaterial`'s shader does
//! `output_color = material.color * mesh.color`, so **the material has to be
//! white** or every vertex colour is tinted twice.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;

use crate::AppColors;

use super::build::BuiltTrack;
use super::data::scalar_to_world;

/// Build the road surface for a track.
///
/// A `TriangleList` rather than a strip: a strip would need degenerate triangles
/// or a duplicated seam both to close the loop and to jump between the kerb
/// bands, and the extra indices cost nothing. Winding is irrelevant --
/// `Mesh2d`'s pipeline sets `cull_mode: None`.
///
/// The seam is closed by indexing modulo the vertex count rather than by
/// repeating the first sample's vertices, so there is no hairline crack and no
/// pair of coincident vertices to z-fight.
pub fn road_mesh(built: &BuiltTrack) -> Mesh {
    let kerb_width = scalar_to_world(built.map.road.kerb_width);
    let kerb_stripe = scalar_to_world(built.map.road.kerb_stripe).max(0.1);
    let with_kerbs = kerb_width > 0.0;
    let per_sample = if with_kerbs { 4 } else { 2 };

    let road = AppColors::Road.color().to_linear().to_f32_array();
    let kerb = AppColors::Kerb.color().to_linear().to_f32_array();
    let white = Color::WHITE.to_linear().to_f32_array();

    let count = built.centre.len();
    let mut positions = Vec::with_capacity(count * per_sample);
    let mut uvs = Vec::with_capacity(count * per_sample);
    let mut colors = Vec::with_capacity(count * per_sample);

    for sample in &built.centre {
        // The kerb keeps a constant width as the road body narrows and widens
        // around it, so a pinch eats into the tarmac rather than the kerb -- but
        // never past it.
        let half = sample.half_width;
        let inner = (half - kerb_width).max(half * 0.25);
        let offsets: &[f32] = if with_kerbs {
            &[half, inner, -inner, -half]
        } else {
            &[half, -half]
        };
        // Stripes alternate along the track, and the same phase drives both
        // sides so the kerbs read as one pattern.
        let stripe = ((sample.s / kerb_stripe) as u32).is_multiple_of(2);
        let edge = if stripe { kerb } else { white };
        for (i, lateral) in offsets.iter().enumerate() {
            let point = sample.position + sample.normal * *lateral;
            positions.push([point.x, point.y, 0.0]);
            uvs.push([
                sample.s / built.length.max(f32::EPSILON),
                0.5 - lateral / (2.0 * half.max(f32::EPSILON)),
            ]);
            let outermost = i == 0 || i == offsets.len() - 1;
            colors.push(if with_kerbs && outermost { edge } else { road });
        }
    }

    let mut indices = Vec::with_capacity(count * (per_sample - 1) * 6);
    for i in 0..count {
        let a = (i * per_sample) as u32;
        // Modulo, so the last ring stitches back onto the first.
        let b = (((i + 1) % count) * per_sample) as u32;
        for strip in 0..(per_sample as u32 - 1) {
            let (a0, a1) = (a + strip, a + strip + 1);
            let (b0, b1) = (b + strip, b + strip + 1);
            indices.extend_from_slice(&[a0, b0, a1, a1, b0, b1]);
        }
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        // MAIN_WORLD as well as RENDER_WORLD, so the editor can mutate this mesh
        // in place while a node is being dragged instead of allocating a new one
        // every frame.
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

/// The chequered start/finish band.
///
/// A mesh of its own rather than more vertex colours in the road, because it has
/// to be exactly at the line and exactly the road's width. Folding it into the
/// road's tessellation would put it wherever the samples happened to land.
pub fn start_line_mesh(built: &BuiltTrack) -> Mesh {
    let pose = built.start_pose;
    let forward = Vec2::new(pose.cos, pose.sin);
    let across = Vec2::new(-forward.y, forward.x);
    let half = built.centre[0].half_width;
    let depth = scalar_to_world(built.map.start.depth).max(0.5);

    const ROWS: usize = 2;
    let square = depth / ROWS as f32;
    let columns = ((2.0 * half) / square).round().max(2.0) as usize;
    let column_width = (2.0 * half) / columns as f32;

    let dark = AppColors::Dark.color().to_linear().to_f32_array();
    let white = Color::WHITE.to_linear().to_f32_array();

    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();

    for row in 0..ROWS {
        for column in 0..columns {
            let base = positions.len() as u32;
            // Centred on the line, so the band straddles progress zero.
            let back = -depth / 2.0 + row as f32 * square;
            let side = half - column as f32 * column_width;
            let corners = [
                (back, side),
                (back + square, side),
                (back, side - column_width),
                (back + square, side - column_width),
            ];
            let color = if (row + column) % 2 == 0 { white } else { dark };
            for (along, lateral) in corners {
                let point = pose.position + forward * along + across * lateral;
                positions.push([point.x, point.y, 0.0]);
                uvs.push([0.0, 0.0]);
                colors.push(color);
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
        }
    }

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::map::build::{BuildLevel, build, tests::circle};

    fn positions(mesh: &Mesh) -> Vec<[f32; 3]> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            bevy::mesh::VertexAttributeValues::Float32x3(v) => v.clone(),
            other => panic!("unexpected position attribute: {other:?}"),
        }
    }

    #[test]
    fn the_road_is_a_closed_ribbon_with_no_seam_vertices() {
        let built = build(&circle(80.0, 11.0), BuildLevel::Preview);
        let mesh = road_mesh(&built);
        let count = built.centre.len();

        assert_eq!(positions(&mesh).len(), count * 4, "four vertices per sample");
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("expected u32 indices")
        };
        // Three strips of quads per sample, two triangles each, closing the loop.
        assert_eq!(indices.len(), count * 3 * 6);
        assert!(
            indices.iter().all(|i| (*i as usize) < count * 4),
            "every index is in range, so the seam wraps rather than dangling"
        );
        // The first and last rings are distinct vertices that meet: no duplicate
        // seam ring, and nothing degenerate between them.
        let p = positions(&mesh);
        let first = Vec2::new(p[0][0], p[0][1]);
        let last = Vec2::new(p[(count - 1) * 4][0], p[(count - 1) * 4][1]);
        assert!(first.distance(last) > 0.1, "the seam is a real segment");
    }

    #[test]
    fn every_road_vertex_is_finite_and_on_the_road() {
        let built = build(&circle(80.0, 11.0), BuildLevel::Preview);
        for [x, y, z] in positions(&road_mesh(&built)) {
            assert!(x.is_finite() && y.is_finite());
            assert_eq!(z, 0.0, "the road is one flat plane; z lives on the entity");
            let r = Vec2::new(x, y).length();
            // The road spans radius 69 to 91; the slack is for the sampling.
            assert!((68.5..=91.5).contains(&r), "vertex at radius {r}");
        }
    }

    /// A kerb wider than the road it edges must not turn the road inside out.
    #[test]
    fn an_absurd_kerb_width_does_not_invert_the_road() {
        let mut map = circle(80.0, 11.0);
        map.road.kerb_width = map.road.half_width * 4;
        let built = build(&map, BuildLevel::Preview);
        for [x, y, _] in positions(&road_mesh(&built)) {
            assert!(x.is_finite() && y.is_finite());
        }
    }

    #[test]
    fn the_start_line_straddles_the_line_and_spans_the_road() {
        let built = build(&circle(80.0, 11.0), BuildLevel::Full);
        let mesh = start_line_mesh(&built);
        let p = positions(&mesh);
        assert!(!p.is_empty());
        let half = built.centre[0].half_width;
        let forward = Vec2::new(built.start_pose.cos, built.start_pose.sin);
        let across = Vec2::new(-forward.y, forward.x);
        let (mut min_across, mut max_across) = (f32::MAX, f32::MIN);
        let (mut min_along, mut max_along) = (f32::MAX, f32::MIN);
        for [x, y, _] in p {
            let d = Vec2::new(x, y) - built.start_pose.position;
            min_across = min_across.min(d.dot(across));
            max_across = max_across.max(d.dot(across));
            min_along = min_along.min(d.dot(forward));
            max_along = max_along.max(d.dot(forward));
        }
        assert!((max_across - half).abs() < 0.2 && (min_across + half).abs() < 0.2);
        assert!(min_along < 0.0 && max_along > 0.0, "it straddles the line");
    }
}
