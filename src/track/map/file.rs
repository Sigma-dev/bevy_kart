//! Maps as files: out of the game and back into it.
//!
//! Two very different mechanisms behind one pair of functions, because "export"
//! means something different on each target. In a browser it is a download; on a
//! desktop it is a path you can find.

use bevy::prelude::*;

use super::data::MapData;
use super::store;

/// Where an import lands.
///
/// A browser file picker answers on a JavaScript callback, long after the system
/// that opened it returned, so the result cannot come back through a return
/// value. It arrives here and a system drains it.
#[derive(Resource, Default, Clone)]
pub struct PendingImport(pub std::sync::Arc<std::sync::Mutex<Option<Result<String, String>>>>);

impl PendingImport {
    pub fn put(&self, result: Result<String, String>) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(result);
        }
    }

    /// Take whatever has arrived, if anything.
    pub fn take(&self) -> Option<Result<String, String>> {
        self.0.lock().ok().and_then(|mut slot| slot.take())
    }
}

/// A suggested file name for a map.
pub fn file_name_for(map: &MapData) -> String {
    format!("{}.kartmap.json", store::slugify(&map.name))
}

// -- the browser ------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod backend {
    use super::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    /// Hand the map to the browser as a download.
    ///
    /// The anchor is never added to the page: creating it, clicking it and
    /// dropping it is enough in every current browser, and it keeps the canvas
    /// the game lives in untouched.
    pub fn export(map: &MapData, json: &str) -> Result<String, String> {
        let window = web_sys::window().ok_or("no window")?;
        let document = window.document().ok_or("no document")?;

        let parts = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(json));
        let options = web_sys::BlobPropertyBag::new();
        options.set_type("application/json");
        let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options)
            .map_err(|_| "could not make the file".to_string())?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)
            .map_err(|_| "could not make a link to the file".to_string())?;

        let anchor = document
            .create_element("a")
            .map_err(|_| "could not make a link".to_string())?
            .dyn_into::<web_sys::HtmlAnchorElement>()
            .map_err(|_| "could not make a link".to_string())?;
        anchor.set_href(&url);
        anchor.set_download(&file_name_for(map));
        anchor.click();
        // The download is already queued by the time click returns.
        let _ = web_sys::Url::revoke_object_url(&url);
        Ok(format!("Downloaded {}", file_name_for(map)))
    }

    /// Open a file picker and put whatever comes back into `pending`.
    ///
    /// Must be called from a click handler: browsers only open a picker inside
    /// the activation window a real user gesture opens.
    pub fn request_import(pending: &PendingImport) {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            pending.put(Err("no document".to_string()));
            return;
        };
        // Two different error types on the way here -- `JsValue` from
        // `create_element` and the `Element` itself back from a failed cast --
        // so they are collapsed rather than chained.
        let input = document
            .create_element("input")
            .ok()
            .and_then(|element| element.dyn_into::<web_sys::HtmlInputElement>().ok());
        let Some(input) = input else {
            pending.put(Err("could not open a file picker".to_string()));
            return;
        };
        input.set_type("file");
        input.set_accept(".json,application/json");

        let slot = pending.clone();
        let picker = input.clone();
        // `once_into_js` hands ownership to JavaScript, which keeps the closure
        // alive exactly until the picker answers and no longer -- where
        // `.forget()` would leak one per import.
        let on_change = Closure::once_into_js(move || {
            let Some(file) = picker.files().and_then(|files| files.get(0)) else {
                slot.put(Err("no file chosen".to_string()));
                return;
            };
            // `File` derefs to `Blob`, and `Blob::text()` is a promise. No
            // `FileReader`, and no second callback to keep alive.
            let text = file.text();
            let slot = slot.clone();
            wasm_bindgen_futures::spawn_local(async move {
                slot.put(
                    match wasm_bindgen_futures::JsFuture::from(text).await {
                        Ok(value) => value
                            .as_string()
                            .ok_or_else(|| "that file is not text".to_string()),
                        Err(_) => Err("could not read that file".to_string()),
                    },
                );
            });
        });
        input.set_onchange(Some(on_change.unchecked_ref()));
        input.click();
    }

    pub fn import_hint() -> String {
        "Import opens a file picker.".to_string()
    }
}

// -- native -----------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
mod backend {
    use super::*;

    /// Write the map beside the others and say where it went.
    ///
    /// No file dialog: `rfd` would pull in GTK3 or the async xdg-portal stack on
    /// Linux for a developer-facing convenience in a game whose real target is
    /// the browser. The maps directory is already where saved maps live, so
    /// "exported" and "saved" are the same place -- which is also what makes
    /// hand-editing one work.
    pub fn export(map: &MapData, json: &str) -> Result<String, String> {
        let directory = store::directory();
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("could not make {}: {error}", directory.display()))?;
        let path = directory.join(file_name_for(map));
        std::fs::write(&path, json)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        Ok(format!("Exported to {}", path.display()))
    }

    /// Nothing to open: on native a map is imported by dropping the file on the
    /// window, or by putting it in the maps directory where the list will find it.
    pub fn request_import(pending: &PendingImport) {
        pending.put(Err(format!(
            "Drop a .json map on the window, or put one in {}",
            store::storage_hint()
        )));
    }

    pub fn import_hint() -> String {
        format!("Drop a map file on the window, or put one in {}", store::storage_hint())
    }
}

pub use backend::{export, import_hint, request_import};

/// Native: accept a map file dropped onto the window.
///
/// `WindowPlugin` already emits these, so drag-and-drop costs one system and is
/// a better answer than a file dialog for the one platform that would need a
/// dependency to have one.
#[cfg(not(target_arch = "wasm32"))]
pub fn accept_dropped_files(
    pending: Res<PendingImport>,
    mut dropped: MessageReader<bevy::window::FileDragAndDrop>,
) {
    for event in dropped.read() {
        let bevy::window::FileDragAndDrop::DroppedFile { path_buf, .. } = event else {
            continue;
        };
        pending.put(
            std::fs::read_to_string(path_buf)
                .map_err(|error| format!("could not read that file: {error}")),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::map::builtin::by_slug;

    #[test]
    fn a_file_name_is_derived_from_the_map_name() {
        let mut map = by_slug("sweeping").unwrap();
        assert_eq!(file_name_for(&map), "sweeping-bends.kartmap.json");
        map.name = "My Track / v2".into();
        assert_eq!(file_name_for(&map), "my-track-v2.kartmap.json");
    }

    #[test]
    fn a_pending_import_is_taken_exactly_once() {
        let pending = PendingImport::default();
        assert!(pending.take().is_none());
        pending.put(Ok("{}".into()));
        assert_eq!(pending.take(), Some(Ok("{}".to_string())));
        assert!(pending.take().is_none(), "and is not delivered twice");
    }
}
