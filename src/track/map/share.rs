//! Maps as text you can paste into a chat window.
//!
//! `postcard` for the bytes, `miniz_oxide` to squeeze them and `base-x` to make
//! them typeable -- the three crates `Cargo.toml` has carried since before this
//! feature existed, under the comment "For compact codes: deflate compression +
//! base62 encoding". This is that.
//!
//! It is the sharing path that works identically on both targets: no file
//! dialog, no browser plumbing, nothing to go wrong differently in a browser
//! than on a desktop. Files are the other half, and better for keeping a map;
//! this is better for handing one to somebody.

use super::data::{MapData, MapError};

/// Digits of the alphabet a code is written in.
///
/// Base62, so a code is only letters and numbers: no punctuation to be eaten by
/// a chat client, no case-folding surprise, and it survives a double-click
/// selection in one piece.
const ALPHABET: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Prefixed to the compressed bytes.
///
/// **Not optional.** Postcard is not self-describing: without a version byte, a
/// code written by an older build decodes as a differently shaped `MapData`
/// rather than failing, and the failure surfaces later as a track with a wall in
/// the wrong place.
const CODE_VERSION: u8 = 1;

/// Refuse anything absurd before spending time on it, so a pasted essay does not
/// turn into a multi-megabyte allocation.
const MAX_CODE_CHARS: usize = 64 * 1024;

pub fn to_share_code(map: &MapData) -> Result<String, String> {
    let mut bytes = vec![CODE_VERSION];
    bytes.extend(
        postcard::to_allocvec(map).map_err(|error| format!("could not encode the map: {error}"))?,
    );
    let compressed = miniz_oxide::deflate::compress_to_vec(&bytes, 9);
    Ok(base_x::encode(ALPHABET, &compressed))
}

pub fn from_share_code(code: &str) -> Result<MapData, String> {
    let trimmed: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if trimmed.is_empty() {
        return Err("that code is empty".to_string());
    }
    if trimmed.len() > MAX_CODE_CHARS {
        return Err("that is much too long to be a map code".to_string());
    }
    let compressed = base_x::decode(ALPHABET, &trimmed)
        .map_err(|_| "that does not look like a map code".to_string())?;
    let bytes = miniz_oxide::inflate::decompress_to_vec(&compressed)
        .map_err(|_| "that map code is damaged".to_string())?;
    let (version, payload) = bytes
        .split_first()
        .ok_or_else(|| "that map code is empty".to_string())?;
    if *version != CODE_VERSION {
        return Err(format!(
            "that map code was made by a different version of the game (v{version}, this is v{CODE_VERSION})"
        ));
    }
    let map: MapData = postcard::from_bytes(payload)
        .map_err(|_| "that map code is damaged".to_string())?;
    map.validate().map_err(|error: MapError| error.to_string())?;
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::map::builtin::by_slug;

    #[test]
    fn a_map_survives_a_code() {
        for slug in ["classic", "sweeping"] {
            let map = by_slug(slug).unwrap();
            let code = to_share_code(&map).unwrap();
            assert_eq!(from_share_code(&code).unwrap(), map, "{slug}");
        }
    }

    /// Codes have to survive being pasted, which means surviving a chat client
    /// that wraps them and a person who selects a trailing newline.
    #[test]
    fn a_code_is_plain_text_and_tolerates_being_pasted() {
        let map = by_slug("classic").unwrap();
        let code = to_share_code(&map).unwrap();
        assert!(
            code.chars().all(|c| c.is_ascii_alphanumeric()),
            "a code should be letters and numbers only"
        );
        let mangled = format!("  {}\n  {}  \n", &code[..20], &code[20..]);
        assert_eq!(from_share_code(&mangled).unwrap(), map);
    }

    /// The version byte is the whole reason a stale code fails loudly instead of
    /// decoding into a differently shaped map.
    #[test]
    fn a_code_from_another_version_is_refused() {
        let map = by_slug("classic").unwrap();
        let mut bytes = vec![CODE_VERSION + 1];
        bytes.extend(postcard::to_allocvec(&map).unwrap());
        let code = base_x::encode(
            ALPHABET,
            &miniz_oxide::deflate::compress_to_vec(&bytes, 9),
        );
        assert!(
            from_share_code(&code)
                .unwrap_err()
                .contains("different version")
        );
    }

    #[test]
    fn nonsense_is_refused_rather_than_believed() {
        assert!(from_share_code("").is_err());
        assert!(from_share_code("   ").is_err());
        assert!(from_share_code("!!! not base62 !!!").is_err());
        // Valid base62 that is not a map.
        assert!(from_share_code("aaaaaaaaaaaaaaaaaaaa").is_err());
        assert!(from_share_code(&"a".repeat(MAX_CODE_CHARS + 1)).is_err());
    }

    /// Short enough to paste is the entire point, so it is worth an assertion.
    #[test]
    fn a_code_is_short_enough_to_hand_to_somebody() {
        for slug in ["classic", "sweeping"] {
            let code = to_share_code(&by_slug(slug).unwrap()).unwrap();
            assert!(
                code.len() < 1400,
                "{slug} makes a {}-character code",
                code.len()
            );
        }
    }
}
