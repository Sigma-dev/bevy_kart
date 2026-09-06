//! Scattering scenery over the ground a track does not cover.
//!
//! Derived from the map's seed, so everyone sees the same field of trees without
//! any of it being sent. Purely cosmetic: nothing here touches physics or the
//! network, which is what lets it use whatever arithmetic is convenient while
//! `build.rs` next door may not.
//!
//! It still has to be *portable*, though, because two players looking at
//! obviously different fields would report it as a bug. So it does not use
//! `rand`: `SmallRng` is documented as non-reproducible and picks a different
//! algorithm on 32-bit targets, which is native versus wasm; `StdRng` is
//! portable but only within a major version. A dozen lines of splitmix64 have
//! neither problem and can be pinned by a golden test.
//!
//! It is also used *statelessly* -- each element's properties come from
//! `mix(seed, index)` rather than from a running generator -- so two peers that
//! end up placing a different *number* of elements still agree about every one
//! they share.

use bevy::prelude::*;

use crate::decor::{BackgroundElement, ground_element_from};

use super::build::BuiltTrack;
use super::data::scalar_to_world;

/// splitmix64. Ordinary integer arithmetic, identical on every target.
#[inline]
pub fn mix(seed: u64, index: u64) -> u64 {
    let mut z = seed ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The top bits of a mixed value as a fraction in `0.0..1.0`.
#[inline]
pub fn unit(bits: u64) -> f32 {
    (bits >> 40) as f32 / 16_777_216.0
}

pub struct DecorPlacement {
    pub element: BackgroundElement,
    pub position: Vec2,
}

/// How many candidate positions to try per element wanted. Rejection sampling
/// against the road, so some candidates land on it.
const ATTEMPTS_PER_ELEMENT: u64 = 4;

/// Elements per thousand square world units, before the map's own multiplier.
const BASE_DENSITY: f32 = 1.0;

const MAX_ELEMENTS: usize = 1200;

pub fn scatter(built: &BuiltTrack) -> Vec<DecorPlacement> {
    let settings = built.map.decor;
    let clearance = scalar_to_world(settings.clearance);
    let bounds = built.bounds;
    let area = bounds.width() * bounds.height();
    if area <= 0.0 || settings.density <= 0.0 {
        return Vec::new();
    }
    let wanted = ((area / 1000.0) * BASE_DENSITY * settings.density) as usize;
    let wanted = wanted.min(MAX_ELEMENTS);

    let grid = RoadGrid::new(built, clearance);
    let mut out = Vec::with_capacity(wanted);
    let mut index = 0u64;
    let ceiling = wanted as u64 * ATTEMPTS_PER_ELEMENT;
    while out.len() < wanted && index < ceiling {
        let a = mix(settings.seed, index * 3);
        let b = mix(settings.seed, index * 3 + 1);
        let c = mix(settings.seed, index * 3 + 2);
        index += 1;
        let position = Vec2::new(
            bounds.min.x + unit(a) * bounds.width(),
            bounds.min.y + unit(b) * bounds.height(),
        );
        if grid.too_close_to_the_road(position) {
            continue;
        }
        out.push(DecorPlacement {
            element: ground_element_from(c),
            position,
        });
    }
    out
}

/// A coarse bucketing of the centreline, so "is this on the road" is a handful
/// of distance tests rather than one per sample.
///
/// Brute force is a few hundred thousand tests per scatter, which is fine once at
/// load and much too slow for an editor re-scattering while a seed slider moves.
struct RoadGrid {
    cell: f32,
    origin: Vec2,
    columns: usize,
    rows: usize,
    buckets: Vec<Vec<u32>>,
    samples: Vec<(Vec2, f32)>,
    clearance: f32,
}

impl RoadGrid {
    fn new(built: &BuiltTrack, clearance: f32) -> Self {
        let widest = built
            .centre
            .iter()
            .map(|s| s.half_width)
            .fold(0.0f32, f32::max);
        // One cell is the furthest a road sample can reach, so a candidate only
        // ever has to look at its own cell and the eight around it.
        let cell = (widest + clearance).max(4.0);
        let origin = built.bounds.min;
        let columns = ((built.bounds.width() / cell).ceil() as usize + 1).max(1);
        let rows = ((built.bounds.height() / cell).ceil() as usize + 1).max(1);
        let mut buckets = vec![Vec::new(); columns * rows];
        let samples: Vec<(Vec2, f32)> = built
            .centre
            .iter()
            .map(|s| (s.position, s.half_width))
            .collect();
        for (index, (position, _)) in samples.iter().enumerate() {
            let column = (((position.x - origin.x) / cell) as usize).min(columns - 1);
            let row = (((position.y - origin.y) / cell) as usize).min(rows - 1);
            buckets[row * columns + column].push(index as u32);
        }
        Self {
            cell,
            origin,
            columns,
            rows,
            buckets,
            samples,
            clearance,
        }
    }

    fn too_close_to_the_road(&self, point: Vec2) -> bool {
        let column = ((point.x - self.origin.x) / self.cell) as isize;
        let row = ((point.y - self.origin.y) / self.cell) as isize;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (c, r) = (column + dx, row + dy);
                if c < 0 || r < 0 || c >= self.columns as isize || r >= self.rows as isize {
                    continue;
                }
                for index in &self.buckets[r as usize * self.columns + c as usize] {
                    let (position, half_width) = self.samples[*index as usize];
                    if point.distance(position) < half_width + self.clearance {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::map::build::{BuildLevel, build, tests::circle};
    use crate::track::map::builtin::by_slug;

    /// Pinned, because the whole point of writing this out by hand rather than
    /// reaching for `rand` is that it cannot change under us.
    #[test]
    fn splitmix64_is_what_it_was() {
        assert_eq!(mix(0, 0), 0);
        assert_eq!(mix(1, 0), 6_238_072_747_940_578_789);
        assert_eq!(mix(0, 1), 16_294_208_416_658_607_535);
        assert_eq!(mix(42, 7), 6_029_533_247_520_485_195);
        assert_eq!(mix(20_260_906, 3), 8_352_629_837_738_901_116);
        // And the fraction it produces stays in range.
        for i in 0..1000 {
            let u = unit(mix(99, i));
            assert!((0.0..1.0).contains(&u), "unit({i}) = {u}");
        }
    }

    #[test]
    fn the_same_seed_scatters_the_same_way() {
        let built = build(&by_slug("sweeping").unwrap(), BuildLevel::Full);
        let a = scatter(&built);
        let b = scatter(&built);
        assert_eq!(a.len(), b.len());
        assert!(!a.is_empty(), "a map that size should have scenery on it");
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.position.to_array(), y.position.to_array());
            assert_eq!(x.element, y.element);
        }
    }

    #[test]
    fn a_different_seed_scatters_differently() {
        let mut map = by_slug("sweeping").unwrap();
        let a = scatter(&build(&map, BuildLevel::Full));
        map.decor.seed += 1;
        let b = scatter(&build(&map, BuildLevel::Full));
        let same = a
            .iter()
            .zip(b.iter())
            .filter(|(x, y)| x.position == y.position)
            .count();
        assert!(same < a.len() / 10, "{same} of {} placements unchanged", a.len());
    }

    /// Nothing on the tarmac: a tree in the racing line is not scenery.
    #[test]
    fn nothing_is_scattered_onto_the_road() {
        let map = by_slug("sweeping").unwrap();
        let built = build(&map, BuildLevel::Full);
        let clearance = scalar_to_world(map.decor.clearance);
        for placement in scatter(&built) {
            let nearest = built
                .centre
                .iter()
                .map(|s| (s.position.distance(placement.position) - s.half_width, s))
                .fold(f32::MAX, |best, (gap, _)| best.min(gap));
            assert!(
                nearest >= clearance - 0.01,
                "an element sits {nearest} from the road edge, inside the {clearance} clearance"
            );
        }
    }

    #[test]
    fn density_scales_the_amount_and_zero_means_none() {
        let mut map = circle(200.0, 11.0);
        map.decor.density = 0.0;
        assert!(scatter(&build(&map, BuildLevel::Full)).is_empty());

        map.decor.density = 0.5;
        let sparse = scatter(&build(&map, BuildLevel::Full)).len();
        map.decor.density = 2.0;
        let dense = scatter(&build(&map, BuildLevel::Full)).len();
        assert!(dense > sparse, "{dense} should be more than {sparse}");
    }
}
