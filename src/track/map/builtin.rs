//! The maps that ship with the game.
//!
//! Compiled in with `include_str!` rather than loaded through the `AssetServer`.
//! There is no `bevy_asset_loader` or loading state in this project and nothing
//! checks handle readiness -- sprites simply pop in when they arrive -- so an
//! asset-loaded map would mean `OnEnter(Screen::Race)` could run before the map
//! existed, and a lobby list that is empty for the first fraction of a second
//! for reasons nobody would ever diagnose. The headless tests have no
//! `AssetPlugin` at all.
//!
//! They live under `assets/` anyway, so they ship in the web bundle and a
//! developer can open one in a text editor. The editor treats them as read-only:
//! saving a built-in writes a copy into the player's own storage.

use super::data::MapData;

/// One map that ships with the game.
pub struct Builtin {
    /// Stable across renames: this is what a URL parameter or a saved
    /// preference refers to.
    pub slug: &'static str,
    json: &'static str,
}

/// In menu order. The first is the default, and the one a session falls back to.
pub const BUILTINS: &[Builtin] = &[
    Builtin {
        slug: "classic",
        json: include_str!("../../../assets/maps/classic.json"),
    },
    Builtin {
        slug: "sweeping",
        json: include_str!("../../../assets/maps/sweeping.json"),
    },
];

impl Builtin {
    /// Parse this map.
    ///
    /// Panics if it does not parse, deliberately: a built-in is compiled into
    /// the binary, so a failure here is a broken build rather than bad input,
    /// and the test below turns it into a failing test rather than a crash in
    /// front of a player.
    pub fn load(&self) -> MapData {
        serde_json::from_str(self.json)
            .unwrap_or_else(|e| panic!("built-in map `{}` does not parse: {e}", self.slug))
    }
}

/// The map used when nothing has chosen one: a fresh launch, a headless test, or
/// a session started by a script with nobody at the keyboard.
pub fn default_map() -> MapData {
    BUILTINS[0].load()
}

/// Look a built-in up by slug, for `?map=` and the lobby picker.
pub fn by_slug(slug: &str) -> Option<MapData> {
    BUILTINS
        .iter()
        .find(|builtin| builtin.slug == slug)
        .map(|builtin| builtin.load())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::map::build::{BuildLevel, MAX_GRID, build};
    use crate::track::map::data::MAP_FORMAT_VERSION;
    use bevy::prelude::Vec2;

    /// Every shipped map parses, validates, and builds into something raceable.
    /// This is what stops a broken built-in reaching a player as a crash.
    #[test]
    fn the_built_in_maps_are_raceable() {
        for builtin in BUILTINS {
            let map = builtin.load();
            assert_eq!(map.version, MAP_FORMAT_VERSION, "{}", builtin.slug);
            assert_eq!(map.validate(), Ok(()), "{}", builtin.slug);

            let built = build(&map, BuildLevel::Full);
            assert!(built.length > 100.0, "{} is tiny: {}", builtin.slug, built.length);
            assert_eq!(built.grid.len(), MAX_GRID, "{}", builtin.slug);
            assert!(built.left_wall.len() > 8, "{} has no left wall", builtin.slug);
            assert!(built.right_wall.len() > 8, "{} has no right wall", builtin.slug);
            assert!(built.progress.len() > 8, "{} has no progress line", builtin.slug);
            assert_eq!(
                built.item_boxes.len(),
                map.item_boxes.len(),
                "{} lost an item box",
                builtin.slug
            );
            for point in built.left_wall.iter().chain(built.right_wall.iter()) {
                assert!(point.is_finite(), "{} has a non-finite wall", builtin.slug);
            }
        }
    }

    /// One map bigger than the screen, because the follow camera and the minimap
    /// only do anything on one -- a game whose every map fits in the viewport
    /// would never exercise either.
    #[test]
    fn at_least_one_built_in_needs_the_camera_to_move() {
        let biggest = BUILTINS
            .iter()
            .map(|b| build(&b.load(), BuildLevel::Preview).bounds.size())
            .fold(Vec2::ZERO, |a, b| a.max(b));
        assert!(
            biggest.x > crate::RESOLUTION.x * 1.5 && biggest.y > crate::RESOLUTION.y * 1.5,
            "the largest map is only {biggest:?}"
        );
    }

    /// The classic track played on a static camera with no minimap, which it
    /// only does while the whole thing fits inside one viewport.
    #[test]
    fn the_classic_track_still_fits_on_one_screen() {
        let built = build(&by_slug("classic").unwrap(), BuildLevel::Full);
        let size = built.bounds.size();
        assert!(
            size.x <= crate::RESOLUTION.x && size.y <= crate::RESOLUTION.y,
            "the classic track grew to {size:?}"
        );
        // And it is recognisably the old one: a lap of roughly 860 units.
        assert!(
            (700.0..1000.0).contains(&built.length),
            "lap length {}",
            built.length
        );
    }

    /// The classic road was drawn as a stroked path of one constant width, so
    /// the conversion keeps it that way.
    ///
    /// The first conversion did not: it measured the corridor against two
    /// hand-traced rings, and had to narrow nine nodes -- some to little over
    /// half the road -- to keep the rings' noise from folding the corners. On
    /// screen that read as corners that suddenly went narrow, which is not a
    /// thing this track ever did.
    #[test]
    fn the_classic_road_is_one_width_the_whole_way_round() {
        let map = by_slug("classic").unwrap();
        let overrides = map.nodes.iter().filter(|n| n.half_width.is_some()).count();
        assert_eq!(overrides, 0, "{overrides} nodes narrow the classic road");

        let built = build(&map, BuildLevel::Full);
        let narrowest = built.centre.iter().map(|s| s.half_width).fold(f32::MAX, f32::min);
        let widest = built.centre.iter().map(|s| s.half_width).fold(f32::MIN, f32::max);
        assert_eq!(narrowest, widest, "the classic road changes width");
        // Twenty-four units across, which is what the sprite was drawn at.
        assert!((widest - 12.0).abs() < 0.01, "half-width {widest}");
    }

    /// The road edges still land where the old track's walls were.
    ///
    /// These two rings were hand-clicked around the original sprite and stood
    /// in `spawn_track` as the actual colliders, so they are the closest thing
    /// there is to a record of where the old track's walls *were*, independent
    /// of how the new one is derived. Both walls stay within a kart's length of
    /// them, and within a metre or so on average.
    ///
    /// The tolerance is a whole kart because the two are not built the same
    /// way: the rings cut every corner as one straight chord, and a wall vertex
    /// is dropped where a hairpin is tighter than the road is wide. Loose as it
    /// is, the conversion this replaced missed the outer ring by twelve units
    /// -- half the width of the road -- and would not pass it.
    #[test]
    fn the_classic_walls_still_stand_where_they_were_traced() {
        // Anticlockwise from the bottom straight, the outside of the circuit.
        const OUTER: &[[f32; 2]] = &[
            [-97.0, -61.5], [33.0, -57.2], [48.2, -47.4], [55.7, -38.0],
            [61.6, -26.0], [66.0, -25.6], [76.6, -45.8], [86.0, -54.0],
            [99.8, -54.2], [106.5, -51.4], [114.6, -43.4], [119.2, -27.0],
            [119.6, 4.2], [115.4, 48.0], [110.6, 57.4], [100.2, 63.2],
            [87.8, 63.6], [69.3, 53.0], [53.4, 41.6], [14.0, 0.0],
            [7.0, -1.4], [0.1, -5.5], [-54.2, -10.6], [-63.0, -6.5],
            [-59.6, -0.2], [-35.1, 0.2], [-9.6, 2.4], [9.8, 11.0],
            [23.2, 22.6], [27.0, 31.2], [27.0, 39.0], [13.8, 54.6],
            [-10.0, 60.0], [-47.2, 58.2], [-90.0, 57.8], [-106.6, 50.0],
            [-119.6, 37.2], [-124.0, 26.2], [-123.8, -34.8], [-120.6, -45.6],
            [-109.0, -57.8],
        ];
        // The infield: one self-touching loop around the track's two islands.
        const INNER: &[[f32; 2]] = &[
            [-92.0, -37.8], [-50.6, -35.2], [15.8, -33.2], [31.8, -32.0],
            [41.8, -13.8], [54.4, -1.4], [72.4, -1.6], [85.4, -11.8],
            [94.5, -29.0], [95.4, -1.4], [92.4, 20.6], [93.6, 36.4],
            [89.4, 40.0], [83.0, 33.8], [62.6, 17.8], [26.2, -21.0],
            [5.6, -29.8], [-20.2, -31.2], [-77.6, -30.8], [-84.7, -25.8],
            [-90.2, -18.8], [-90.4, 5.0], [-84.6, 14.8], [-69.8, 23.6],
            [-26.1, 25.2], [-13.0, 28.0], [-0.8, 31.4], [-0.2, 34.8],
            [-27.4, 35.6], [-87.2, 33.0], [-98.6, 25.8], [-100.6, -22.2],
            [-98.4, -35.2],
        ];

        fn to_ring(points: &[[f32; 2]]) -> Vec<Vec2> {
            points.iter().map(|p| Vec2::new(p[0], p[1])).collect()
        }

        fn distance_to(point: Vec2, ring: &[Vec2]) -> f32 {
            (0..ring.len())
                .map(|i| {
                    let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                    let along = b - a;
                    let t = if along.length_squared() > 1e-9 {
                        ((point - a).dot(along) / along.length_squared()).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    point.distance(a + along * t)
                })
                .fold(f32::MAX, f32::min)
        }

        let built = build(&by_slug("classic").unwrap(), BuildLevel::Full);
        // Left and right of the direction of travel: along the bottom straight
        // the karts run east, so the infield is on their left.
        for (wall, ring, name) in [
            (&built.left_wall, to_ring(INNER), "inner"),
            (&built.right_wall, to_ring(OUTER), "outer"),
        ] {
            let distances: Vec<f32> = wall.iter().map(|p| distance_to(*p, &ring)).collect();
            let worst = distances.iter().copied().fold(0.0, f32::max);
            let mean = distances.iter().sum::<f32>() / distances.len() as f32;
            assert!(worst < 8.0, "the {name} wall strays {worst} from its ring");
            assert!(mean < 1.5, "the {name} wall averages {mean} from its ring");
        }
    }

    #[test]
    fn slugs_are_unique_and_findable() {
        let mut seen = Vec::new();
        for builtin in BUILTINS {
            assert!(!seen.contains(&builtin.slug), "duplicate slug {}", builtin.slug);
            seen.push(builtin.slug);
            assert!(by_slug(builtin.slug).is_some());
        }
        assert!(by_slug("no-such-map").is_none());
    }
}
