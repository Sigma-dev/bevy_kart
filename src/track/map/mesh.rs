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
/// to jump between the tarmac and the two wall bands, and the extra indices cost
/// nothing. Winding is irrelevant -- `Mesh2d`'s pipeline sets `cull_mode: None`.
///
/// The wall bands are what the player sees of the track's walls. The barriers
/// `spawn_barriers` builds are colliders and nothing else: a second
/// red-and-white polyline drawn over this one, from a coarser set of points, is
/// what made the track look like it had two rows of barriers with only one of
/// them stopping anything.
pub fn road_mesh(built: &BuiltTrack) -> Mesh {
    let stripe_length = scalar_to_world(built.map.road.kerb_stripe).max(0.1);
    let kerb_width = scalar_to_world(built.map.road.kerb_width);

    let road = AppColors::Road.color().to_linear().to_f32_array();
    let kerb = AppColors::Kerb.color().to_linear().to_f32_array();
    let white = Color::WHITE.to_linear().to_f32_array();

    let count = built.centre.len();
    let mut positions = Vec::with_capacity(count * 10);
    let mut uvs = Vec::with_capacity(count * 10);
    let mut colors = Vec::with_capacity(count * 10);
    let mut indices = Vec::with_capacity(count * 18);

    // How far the band reaches either side of the road edge. Half of it hangs
    // over the grass, exactly as the barriers standing on that edge used to --
    // and it is clamped, so a map with an absurd kerb width cannot eat the road
    // it is supposed to be edging.
    let reach = |half: f32| (kerb_width / 2.0).min(half * 0.5);

    // The tarmac: one ribbon of shared rings, closed by indexing modulo the
    // vertex count rather than by repeating the first ring, so there is no
    // hairline crack and no pair of coincident vertices to z-fight.
    for sample in &built.centre {
        let inner = sample.half_width - reach(sample.half_width);
        for lateral in [inner, -inner] {
            let point = sample.position + sample.normal * lateral;
            positions.push([point.x, point.y, 0.0]);
            uvs.push([
                sample.s / built.length.max(f32::EPSILON),
                0.5 - lateral / (2.0 * sample.half_width.max(f32::EPSILON)),
            ]);
            colors.push(road);
        }
    }
    for i in 0..count {
        let a = (i * 2) as u32;
        let b = (((i + 1) % count) * 2) as u32;
        indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
    }

    // The wall band down each edge, as quads that own their four vertices.
    //
    // Its own quads rather than two more columns of the ribbon, because a stripe
    // has to *start*. Sharing a ring between the span before a colour change and
    // the span after it is an instruction to the shader to interpolate across
    // it, and a two-unit fade at each end of a nine-unit stripe is the whole
    // band reading as a blur rather than as paint.
    for i in 0..count {
        let (a, b) = (&built.centre[i], &built.centre[(i + 1) % count]);
        // The same phase drives both sides, so the two edges read as one
        // pattern, and it is taken from `a` alone so the quad is one colour.
        let colour = if ((a.s / stripe_length) as u32).is_multiple_of(2) {
            kerb
        } else {
            white
        };
        for side in [1.0f32, -1.0] {
            let base = positions.len() as u32;
            for sample in [a, b] {
                let edge = sample.half_width * side;
                let out = reach(sample.half_width) * side;
                for lateral in [edge - out, edge + out] {
                    let point = sample.position + sample.normal * lateral;
                    positions.push([point.x, point.y, 0.0]);
                    uvs.push([sample.s / built.length.max(f32::EPSILON), 0.0]);
                    colors.push(colour);
                }
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
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

        // Two shared vertices per sample for the tarmac, and four of its own per
        // wall-band quad, twice.
        assert_eq!(positions(&mesh).len(), count * 10);
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("expected u32 indices")
        };
        // One quad of tarmac and two of band per sample, two triangles each.
        assert_eq!(indices.len(), count * 3 * 6);
        assert!(
            indices.iter().all(|i| (*i as usize) < count * 10),
            "every index is in range, so the seam wraps rather than dangling"
        );
        // The tarmac's first and last rings are distinct vertices that meet: no
        // duplicate seam ring, and nothing degenerate between them.
        let p = positions(&mesh);
        let first = Vec2::new(p[0][0], p[0][1]);
        let last = Vec2::new(p[(count - 1) * 2][0], p[(count - 1) * 2][1]);
        assert!(first.distance(last) > 0.1, "the seam is a real segment");
    }

    /// Every wall-band quad is one flat colour.
    ///
    /// The band used to be two more columns of the shared ribbon, which meant
    /// each stripe faded into the tarmac across its width *and* into the next
    /// stripe along its length. Vertex colours interpolate; paint does not.
    #[test]
    fn the_wall_band_is_painted_rather_than_blended() {
        let built = build(&circle(80.0, 11.0), BuildLevel::Preview);
        let mesh = road_mesh(&built);
        let count = built.centre.len();
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap() {
            bevy::mesh::VertexAttributeValues::Float32x4(v) => v.clone(),
            other => panic!("unexpected colour attribute: {other:?}"),
        };
        let kerb = AppColors::Kerb.color().to_linear().to_f32_array();
        let white = Color::WHITE.to_linear().to_f32_array();

        let mut seen_red = false;
        let mut seen_white = false;
        for quad in colors[count * 2..].chunks(4) {
            assert!(
                quad.iter().all(|c| *c == quad[0]),
                "a band quad blends across itself: {quad:?}"
            );
            seen_red |= quad[0] == kerb;
            seen_white |= quad[0] == white;
            assert!(quad[0] == kerb || quad[0] == white, "off-palette band colour");
        }
        assert!(seen_red && seen_white, "the band is meant to be striped");
    }

    #[test]
    fn every_road_vertex_is_finite_and_on_the_road() {
        let built = build(&circle(80.0, 11.0), BuildLevel::Preview);
        for [x, y, z] in positions(&road_mesh(&built)) {
            assert!(x.is_finite() && y.is_finite());
            assert_eq!(z, 0.0, "the road is one flat plane; z lives on the entity");
            let r = Vec2::new(x, y).length();
            // Tarmac from radius 69.75 to 90.25, and a band straddling each edge
            // out to 68.25 and 91.75. The slack is for the sampling.
            assert!((68.0..=92.0).contains(&r), "vertex at radius {r}");
        }
    }

    /// A kerb wider than the road it edges must not turn the road inside out.
    #[test]
    fn an_absurd_kerb_width_does_not_invert_the_road() {
        let mut map = circle(80.0, 11.0);
        map.road.kerb_width = map.road.half_width * 4;
        let built = build(&map, BuildLevel::Preview);
        let mesh = road_mesh(&built);
        for [x, y, _] in positions(&mesh) {
            assert!(x.is_finite() && y.is_finite());
        }
        // The tarmac is still tarmac: the band ate half the road's width and
        // stopped, rather than crossing the centreline and folding it over.
        let p = positions(&mesh);
        for i in 0..built.centre.len() {
            let left = Vec2::new(p[i * 2][0], p[i * 2][1]);
            let right = Vec2::new(p[i * 2 + 1][0], p[i * 2 + 1][1]);
            let across = built.centre[i].normal;
            assert!(
                (left - right).dot(across) > 0.0,
                "the tarmac ribbon inverted at sample {i}"
            );
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
