//! The asset cache of a browser page, as the page's script publishes it in
//! `globalThis.dbota_cache` (web/cache.js). Two kinds:
//! - `{mode: "opfs", stamp, files: Map(path -> size), small: Map(path -> bytes), read(path)}`: the
//!   cache the page's worker built from the player's own Dota install, in the Origin Private
//!   File System. `files` is its index, `small` the files read synchronously at start, `read`
//!   an async function giving a file's bytes (or null).
//! - `{mode: "http"}`: a cache a developer serves next to the page (`web/README.md`).
//!
//! Without it, the client draws its built-in stand-ins.

use js_sys::{Function, Map, Promise, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

fn published() -> Option<JsValue> {
    let cache = Reflect::get(&js_sys::global(), &"dbota_cache".into())
        .ok()
        .filter(|v| v.is_object())?;
    current(&cache).then_some(cache)
}

/// Whether a published cache may be read: a served one always (a developer's own export); one
/// in private storage only when its stamp carries this build's asset fingerprint. The page
/// publishes no other (it asks for a re-import first); were one published anyway, the client
/// draws stand-ins rather than what another dbota made.
fn current(cache: &JsValue) -> bool {
    let mode = Reflect::get(cache, &"mode".into())
        .ok()
        .and_then(|m| m.as_string());
    if mode.as_deref() != Some("opfs") {
        return true;
    }
    let stamp = Reflect::get(cache, &"stamp".into())
        .ok()
        .and_then(|s| s.as_string())
        .unwrap_or_default();
    let here = dbota_import::import::stamped_here(&stamp);
    static SAID: std::sync::Once = std::sync::Once::new();
    if !here {
        SAID.call_once(|| {
            bevy::log::error!(
                "the imported assets are stamped {stamp:?}, not this build's {}: drawn as stand-ins",
                dbota_import::import::fingerprint()
            );
        });
    }
    here
}

fn field(name: &str) -> Option<JsValue> {
    Reflect::get(&published()?, &name.into())
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
}

/// Whether the page has a cache of either kind for the client.
pub fn available() -> bool {
    published().is_some()
}

/// Whether the page's cache is the private-storage one.
pub fn is_opfs() -> bool {
    field("mode").and_then(|m| m.as_string()).as_deref() == Some("opfs")
}

/// Whether the cache has the file `rel`: by its index in private storage; every listed file
/// is taken to be there for a served cache.
pub fn present(rel: &str) -> bool {
    if !is_opfs() {
        return available();
    }
    field("files").is_some_and(|files| Map::from(files).has(&rel.into()))
}

/// A file the page loaded into memory for the client's synchronous reads.
pub fn small(rel: &str) -> Option<Vec<u8>> {
    let bytes = Map::from(field("small")?).get(&rel.into());
    bytes.dyn_into::<Uint8Array>().ok().map(|b| b.to_vec())
}

/// A file's bytes from private storage; `Ok(None)` when it is not there.
pub async fn read(rel: &str) -> Result<Option<Vec<u8>>, String> {
    let read: Function = field("read")
        .and_then(|f| f.dyn_into().ok())
        .ok_or("the page has no cache reader")?;
    let promise: Promise = read
        .call1(&JsValue::NULL, &rel.into())
        .map_err(|e| format!("{e:?}"))?
        .dyn_into()
        .map_err(|_| "the cache reader gave no promise".to_string())?;
    let value = JsFuture::from(promise)
        .await
        .map_err(|e| format!("{e:?}"))?;
    Ok(value.dyn_into::<Uint8Array>().ok().map(|b| b.to_vec()))
}
