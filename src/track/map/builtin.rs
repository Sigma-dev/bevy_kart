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

    /// The conversion is meant to land in the same place as the track it
    /// replaces, so the first commit changes what draws the road and nothing
    /// about how the game plays.
    #[test]
    fn the_classic_track_still_fits_on_one_screen() {
        let built = build(&by_slug("classic").unwrap(), BuildLevel::Full);
        let size = built.bounds.size();
        assert!(
            size.x < 400.0 && size.y < 260.0,
            "the classic track grew to {size:?}"
        );
        // And it is recognisably the old one: a lap of roughly 850 units.
        assert!(
            (700.0..1000.0).contains(&built.length),
            "lap length {}",
            built.length
        );
    }

    /// The width really does vary along the converted track -- that is the whole
    /// point of measuring it per node against the traced rings rather than
    /// averaging it into one number.
    #[test]
    fn the_classic_track_has_a_road_that_changes_width() {
        let map = by_slug("classic").unwrap();
        let overrides = map.nodes.iter().filter(|n| n.half_width.is_some()).count();
        assert!(overrides >= 4, "only {overrides} nodes set their own width");

        let built = build(&map, BuildLevel::Full);
        let narrowest = built.centre.iter().map(|s| s.half_width).fold(f32::MAX, f32::min);
        let widest = built.centre.iter().map(|s| s.half_width).fold(f32::MIN, f32::max);
        assert!(
            widest - narrowest > 2.0,
            "road width barely varies: {narrowest} to {widest}"
        );
        assert!(narrowest > 4.0, "narrowest {narrowest} is not drivable");
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
