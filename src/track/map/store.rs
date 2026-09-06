//! Where a player's own maps live.
//!
//! Browser storage on the web, a directory on native, behind one synchronous
//! API. The `#[cfg]` split follows the shape `lobby::get_url` already uses.
//!
//! The stored form is pretty-printed JSON wrapped in a versioned envelope.
//! `serde_json` was already a dependency; JSON is what makes a native map file
//! worth opening in an editor; and browser storage holds strings anyway, so a
//! binary encoding would need wrapping in text to get in there. Postcard's job
//! is the share code next door, where being short is the whole point.

use serde::{Deserialize, Serialize};

use super::data::{MapData, MapError};

/// Bumped when the *envelope* changes. The map inside carries its own version.
const STORE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct MapFile {
    version: u32,
    map: MapData,
}

/// Enough to list a map without parsing all of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMeta {
    pub id: String,
    pub name: String,
}

/// Anything that can go wrong on the way to or from storage.
///
/// A `String` because every one of these ends up in front of the player in the
/// editor's status line, and none of them is worth a match arm anywhere.
pub type StoreResult<T> = Result<T, String>;

/// A filesystem- and URL-safe identifier derived from a map's name.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "map".to_string()
    } else {
        trimmed
    }
}

/// A slug not already taken by a *different* map.
///
/// Human-meaningful rather than a counter or a timestamp, so a native map file
/// is `maps/sweeping-bends.json` and a player can find it.
pub fn unique_id(name: &str, existing: &[MapMeta]) -> String {
    let base = slugify(name);
    if !existing.iter().any(|meta| meta.id == base) {
        return base;
    }
    for suffix in 2..1000 {
        let candidate = format!("{base}-{suffix}");
        if !existing.iter().any(|meta| meta.id == candidate) {
            return candidate;
        }
    }
    base
}

fn encode(map: &MapData) -> StoreResult<String> {
    serde_json::to_string_pretty(&MapFile {
        version: STORE_VERSION,
        map: map.clone(),
    })
    .map_err(|error| format!("could not encode the map: {error}"))
}

fn decode(text: &str) -> StoreResult<MapData> {
    let file: MapFile =
        serde_json::from_str(text).map_err(|error| format!("could not read the map: {error}"))?;
    if file.version > STORE_VERSION {
        return Err(format!(
            "that map was saved by a newer version of the game ({} > {STORE_VERSION})",
            file.version
        ));
    }
    file.map
        .validate()
        .map_err(|error: MapError| error.to_string())?;
    Ok(file.map)
}

// -- the browser ------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod backend {
    use super::*;

    const PREFIX: &str = "bevy_kart.map.";

    fn storage() -> StoreResult<web_sys::Storage> {
        web_sys::window()
            .ok_or_else(|| "no window".to_string())?
            .local_storage()
            .map_err(|_| "browser storage is blocked".to_string())?
            // `Ok(None)` rather than an error is what a private window gives, so
            // it is a normal thing to be told rather than a crash.
            .ok_or_else(|| "browser storage is unavailable in this window".to_string())
    }

    pub fn list() -> Vec<MapMeta> {
        let Ok(storage) = storage() else {
            return Vec::new();
        };
        let count = storage.length().unwrap_or(0);
        let mut out = Vec::new();
        // Scanning the keys rather than keeping an index alongside them: an index
        // is a second thing to keep in step, and a half-written one loses
        // everything it claims to list.
        for i in 0..count {
            let Ok(Some(key)) = storage.key(i) else { continue };
            let Some(id) = key.strip_prefix(PREFIX) else {
                continue;
            };
            let Ok(Some(text)) = storage.get_item(&key) else {
                continue;
            };
            if let Ok(map) = decode(&text) {
                out.push(MapMeta {
                    id: id.to_string(),
                    name: map.name,
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn load(id: &str) -> StoreResult<MapData> {
        let storage = storage()?;
        let text = storage
            .get_item(&format!("{PREFIX}{id}"))
            .map_err(|_| "could not read browser storage".to_string())?
            .ok_or_else(|| format!("no saved map called `{id}`"))?;
        decode(&text)
    }

    pub fn save(id: &str, map: &MapData) -> StoreResult<()> {
        let storage = storage()?;
        storage
            .set_item(&format!("{PREFIX}{id}"), &encode(map)?)
            // The only realistic failure is the quota, and a few hundred maps fit
            // in it, so this is a safety net rather than an expected path.
            .map_err(|_| "browser storage is full: delete a map, or export one first".to_string())
    }

    pub fn delete(id: &str) -> StoreResult<()> {
        storage()?
            .remove_item(&format!("{PREFIX}{id}"))
            .map_err(|_| "could not write to browser storage".to_string())
    }

    pub fn storage_hint() -> String {
        "browser storage".to_string()
    }
}

// -- native -----------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
mod backend {
    use super::*;
    use std::path::PathBuf;

    /// Where maps live. `KART_MAPS_DIR` if set, else `maps/` beside the working
    /// directory -- which is the repository root under both `cargo run` and
    /// `scripts/local-session.sh`, so a whole local session shares one set.
    pub fn directory() -> PathBuf {
        std::env::var("KART_MAPS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("maps"))
    }

    pub fn path_for(id: &str) -> PathBuf {
        directory().join(format!("{id}.json"))
    }

    pub fn list() -> Vec<MapMeta> {
        let Ok(entries) = std::fs::read_dir(directory()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Ok(map) = decode(&text) {
                out.push(MapMeta {
                    id: id.to_string(),
                    name: map.name,
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn load(id: &str) -> StoreResult<MapData> {
        let path = path_for(id);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        decode(&text)
    }

    pub fn save(id: &str, map: &MapData) -> StoreResult<()> {
        let directory = directory();
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("could not make {}: {error}", directory.display()))?;
        let path = path_for(id);
        std::fs::write(&path, encode(map)?)
            .map_err(|error| format!("could not write {}: {error}", path.display()))
    }

    pub fn delete(id: &str) -> StoreResult<()> {
        let path = path_for(id);
        std::fs::remove_file(&path)
            .map_err(|error| format!("could not delete {}: {error}", path.display()))
    }

    pub fn storage_hint() -> String {
        std::fs::canonicalize(directory())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| directory().display().to_string())
    }
}

pub use backend::{delete, list, load, save, storage_hint};

#[cfg(not(target_arch = "wasm32"))]
pub use backend::{directory, path_for};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::map::builtin::by_slug;

    #[test]
    fn slugs_are_readable_and_safe() {
        assert_eq!(slugify("Sweeping Bends"), "sweeping-bends");
        assert_eq!(slugify("  Hello,   World!  "), "hello-world");
        assert_eq!(slugify("../../etc/passwd"), "etc-passwd");
        assert_eq!(slugify("***"), "map");
        assert_eq!(slugify(""), "map");
        // No separator can survive into a path or a storage key.
        for name in ["a/b", "a\\b", "a.b", "a b"] {
            let slug = slugify(name);
            assert!(!slug.contains(['/', '\\', '.', ' ']), "{name} -> {slug}");
        }
    }

    #[test]
    fn a_second_map_of_the_same_name_gets_its_own_id() {
        let taken = vec![
            MapMeta { id: "loop".into(), name: "Loop".into() },
            MapMeta { id: "loop-2".into(), name: "Loop".into() },
        ];
        assert_eq!(unique_id("Fresh", &taken), "fresh");
        assert_eq!(unique_id("Loop", &taken), "loop-3");
    }

    #[test]
    fn a_map_survives_being_stored_and_read_back() {
        let map = by_slug("sweeping").unwrap();
        let text = encode(&map).unwrap();
        assert_eq!(decode(&text).unwrap(), map);
    }

    #[test]
    fn storage_refuses_what_it_cannot_trust() {
        assert!(decode("not json at all").is_err());
        // An envelope from a future build.
        let future = format!(
            r#"{{"version":{},"map":{}}}"#,
            STORE_VERSION + 1,
            serde_json::to_string(&by_slug("classic").unwrap()).unwrap()
        );
        assert!(decode(&future).unwrap_err().contains("newer version"));
        // A structurally valid file describing an unbuildable map.
        let mut broken = by_slug("classic").unwrap();
        broken.nodes.truncate(2);
        let text = encode(&broken).unwrap();
        assert!(decode(&text).is_err(), "two nodes is not a track");
    }

    /// Round-trips through the real backend. Native only: the wasm half needs a
    /// browser, and `wasm-bindgen-test` is not wired up here yet.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn saving_then_listing_then_loading_finds_the_map() {
        let directory = std::env::temp_dir().join(format!(
            "bevy_kart_store_test_{}",
            std::process::id()
        ));
        // SAFETY: single-threaded within this test, and the variable is only
        // read by this module.
        unsafe { std::env::set_var("KART_MAPS_DIR", &directory) };
        let _ = std::fs::remove_dir_all(&directory);

        assert!(list().is_empty(), "a fresh directory has no maps");
        let map = by_slug("classic").unwrap();
        let id = unique_id(&map.name, &list());
        save(&id, &map).unwrap();

        let listed = list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "classic");
        assert_eq!(listed[0].name, map.name);
        assert_eq!(load(&id).unwrap(), map);

        delete(&id).unwrap();
        assert!(list().is_empty(), "and it is gone again");
        let _ = std::fs::remove_dir_all(&directory);
        unsafe { std::env::remove_var("KART_MAPS_DIR") };
    }
}
