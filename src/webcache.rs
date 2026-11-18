//! The browser's copy of the assets, as the client reads it. [`open`] finds it once, before the
//! app is built (some forty systems read the copy as they start), and the client then reads it
//! file by file. Two kinds:
//! - private: the copy the import worker made from the player's own Dota 2 install in the page's
//!   private storage, the Origin Private File System (`dbota_import::browser`). Its stamp must
//!   carry this build's asset fingerprint and its index must validate, else the client starts at
//!   the first-run screen (`firstrun`), which makes it again;
//! - served (`?cache=http`): a copy a developer serves next to the page (`web/README.md`), its
//!   validated index fetched at the start.
//!
//! Without either, the client reads no asset: it shows the first-run screen.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use bevy::log::{info, warn};
use dbota_import::browser::{CURRENT, MAX_CURRENT, ROOT};
use dbota_import::index::{CacheIndex, MAX_INDEX};
use js_sys::{Promise, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{File, FileSystemDirectoryHandle, FileSystemFileHandle};

use crate::firstrun::Need;

/// The copy the client reads.
enum Copy {
    /// In private storage, in the folder `folder` of [`ROOT`]; `small` holds the files read at once.
    Private {
        folder: String,
        files: HashMap<String, u64>,
        small: HashMap<String, Vec<u8>>,
    },
    /// Served next to the page, under `cache/`.
    Served { files: HashMap<String, u64> },
}

static COPY: OnceLock<Copy> = OnceLock::new();

thread_local! {
    /// The folders of the private copy, by path, kept once found.
    static FOLDERS: RefCell<HashMap<String, FileSystemDirectoryHandle>> = RefCell::new(HashMap::new());
}

/// The longest a step of the browser's storage may take at the start: a storage that does not
/// answer leaves the first-run screen saying so, not a page that waits for ever.
const STORAGE_MS: i32 = 20_000;
/// What a storage step that does not answer is called.
const STORAGE: &str = "The browser's private storage";

/// What the start found.
pub enum Opened {
    /// A copy of this build: the client starts on it.
    Ready,
    /// The first-run screen: why, and what went wrong with the storage when something did.
    Needs(Need, Option<String>),
    /// A served copy that cannot be read: the page says so.
    Refused(String),
}

/// Finds the copy the client starts on: the served one where the page's address asks for it
/// (`?cache=http`), else the one in private storage.
pub async fn open() -> Opened {
    if served_asked() {
        return match served().await {
            Ok(files) => {
                let _ = COPY.set(Copy::Served { files });
                Opened::Ready
            }
            Err(why) => Opened::Refused(why),
        };
    }
    match private().await {
        Ok(Found::Copy {
            folder,
            files,
            small,
        }) => {
            said_now(&format!(
                "asset copy: {folder} in this browser's private storage, {} files",
                files.len()
            ));
            let _ = COPY.set(Copy::Private {
                folder,
                files,
                small,
            });
            Opened::Ready
        }
        Ok(Found::Needs(need, why)) => {
            said_now(&format!("asset copy: {why}"));
            Opened::Needs(need, None)
        }
        Err(Trouble::Broken(why)) => {
            said_now(&format!("asset copy: {why}"));
            Opened::Needs(Need::Import, None)
        }
        Err(Trouble::Storage(why)) => {
            said_now(&format!("asset copy: the private storage failed: {why}"));
            Opened::Needs(
                Need::Import,
                Some(format!(
                    "This browser's private storage, where dbota keeps its copy of the Dota 2 \
                     files, does not open ({why}). A private window has none in most browsers: \
                     open dbota in a normal window."
                )),
            )
        }
    }
}

/// A line of the start for the console, written there at once: the app's log is not set up yet.
fn said_now(line: &str) {
    web_sys::console::info_1(&line.into());
}

/// Whether the page's address asks for the served copy.
fn served_asked() -> bool {
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    web_sys::UrlSearchParams::new_with_str(&search)
        .is_ok_and(|q| q.get("cache").as_deref() == Some("http"))
}

/// A browser call's error as text.
fn said(error: &JsValue) -> String {
    error
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| format!("{error:?}"))
}

/// Whether a storage call failed because what it asked for is not there.
fn missing(error: &JsValue) -> bool {
    let name = Reflect::get(error, &"name".into())
        .ok()
        .and_then(|n| n.as_string());
    matches!(name.as_deref(), Some("NotFoundError" | "TypeMismatchError"))
}

/// `promise`, or an error naming `what` once it has not settled within [`STORAGE_MS`].
async fn within(promise: Promise, what: &str) -> Result<JsValue, JsValue> {
    let late = Promise::new(&mut |_, reject| {
        if let Some(window) = web_sys::window() {
            let message = JsValue::from(js_sys::Error::new(&format!(
                "{what} did not answer in {} seconds",
                STORAGE_MS / 1000
            )));
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_1(
                &reject, STORAGE_MS, &message,
            );
        }
    });
    JsFuture::from(Promise::race(&js_sys::Array::of2(&promise, &late))).await
}

/// Why the private copy could not be read: the storage failed, or the copy is broken (a file
/// larger than it may be), which an import makes again.
enum Trouble {
    Storage(String),
    Broken(String),
}

impl From<JsValue> for Trouble {
    fn from(error: JsValue) -> Trouble {
        Trouble::Storage(said(&error))
    }
}

/// What private storage holds.
enum Found {
    Copy {
        folder: String,
        files: HashMap<String, u64>,
        small: HashMap<String, Vec<u8>>,
    },
    /// No copy the client may read: what it needs, and why (for the log).
    Needs(Need, String),
}

/// The folder `name` of `dir`, or nothing when it is not there.
async fn folder(
    dir: &FileSystemDirectoryHandle,
    name: &str,
) -> Result<Option<FileSystemDirectoryHandle>, Trouble> {
    match within(dir.get_directory_handle(name), STORAGE).await {
        Ok(handle) => Ok(Some(handle.unchecked_into())),
        Err(e) if missing(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// The bytes of the file `name` of `dir` when it is there and at most `limit` long (its size is
/// looked at before any of it is read); nothing when it is not there.
async fn file(
    dir: &FileSystemDirectoryHandle,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, Trouble> {
    let handle: FileSystemFileHandle = match within(dir.get_file_handle(name), STORAGE).await {
        Ok(handle) => handle.unchecked_into(),
        Err(e) if missing(&e) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let file: File = within(handle.get_file(), STORAGE).await?.unchecked_into();
    if file.size() > limit as f64 {
        let why = format!("{name} is {} bytes, at most {limit}", file.size());
        return Err(Trouble::Broken(why));
    }
    let buffer = within(file.array_buffer(), STORAGE).await?;
    Ok(Some(Uint8Array::new(&buffer).to_vec()))
}

/// The file at `path` below `dir` (see [`file`]).
async fn file_at(
    dir: &FileSystemDirectoryHandle,
    path: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, Trouble> {
    let mut parts: Vec<&str> = path.split('/').collect();
    let name = parts.pop().unwrap_or_default();
    let mut at = dir.clone();
    for part in parts {
        match folder(&at, part).await? {
            Some(next) => at = next,
            None => return Ok(None),
        }
    }
    file(&at, name, limit).await
}

/// The copy in private storage, checked: named by [`CURRENT`], stamped by this build, its index
/// valid, its small files there and of this build's layouts.
async fn private() -> Result<Found, Trouble> {
    let window = web_sys::window().ok_or_else(|| Trouble::Storage("no window".to_owned()))?;
    let opfs: FileSystemDirectoryHandle =
        within(window.navigator().storage().get_directory(), STORAGE)
            .await?
            .unchecked_into();
    let needs = |need: Need, why: &str| Ok(Found::Needs(need, why.to_owned()));
    let Some(root) = folder(&opfs, ROOT).await? else {
        return needs(Need::Import, "none in this browser's private storage");
    };
    let named = file(&root, CURRENT, MAX_CURRENT)
        .await?
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let Some(name) = named
        .as_deref()
        .and_then(dbota_import::browser::current_copy)
        .map(str::to_owned)
    else {
        return needs(
            Need::Import,
            "no finished copy in this browser's private storage",
        );
    };
    let Some(dir) = folder(&root, &name).await? else {
        return needs(Need::Import, "the copy named current is not there");
    };
    let stamp = file(
        &dir,
        dbota_import::import::STAMP,
        dbota_import::import::MAX_STAMP,
    )
    .await?;
    let stamp = stamp.map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    match &stamp {
        None => return needs(Need::Import, "the copy has no stamp"),
        Some(stamp) if !dbota_import::import::stamped_here(stamp) => {
            let why = format!(
                "the copy is stamped {:?}, this build reads assets {}",
                stamp.trim(),
                dbota_import::import::fingerprint()
            );
            return needs(Need::Reimport, &why);
        }
        Some(_) => {}
    }
    let Some(index) = file(&dir, dbota_import::import::INDEX, MAX_INDEX).await? else {
        return needs(Need::Import, "the copy has no index");
    };
    let index = match CacheIndex::parse(&index) {
        Ok(index) => index,
        Err(why) => return needs(Need::Import, &format!("the copy's index is refused: {why}")),
    };
    let mut files = HashMap::new();
    let mut small = HashMap::new();
    for listed in index.files() {
        files.insert(listed.path.clone(), listed.size);
        if listed.small {
            let limit = dbota_import::pack::size_limit(&listed.path);
            match file_at(&dir, &listed.path, limit).await? {
                Some(bytes) if bytes.len() as u64 == listed.size => {
                    small.insert(listed.path.clone(), bytes);
                }
                _ => {
                    let why = format!("the copy's {} is not as its index says", listed.path);
                    return needs(Need::Import, &why);
                }
            }
        }
    }
    let current = dbota_import::import::layouts_current_with(
        |path, limit| {
            small
                .get(path)
                .map(|bytes| bytes[..bytes.len().min(limit)].to_vec())
        },
        |path| files.contains_key(path),
    );
    if !current {
        return needs(Need::Reimport, "the copy holds files of an older layout");
    }
    // What a closed tab left of an import beside the copy goes now, unless an import (in another
    // tab) is writing its copy there.
    wasm_bindgen_futures::spawn_local(unless_importing(sweep(root, name.clone())));
    Ok(Found::Copy {
        folder: name,
        files,
        small,
    })
}

/// Runs `work` when no import holds [`dbota_import::browser::IMPORT_LOCK`], holding it meanwhile;
/// a browser without Web Locks runs it all the same.
async fn unless_importing(work: impl std::future::Future<Output = ()> + 'static) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let locks = Reflect::get(&window.navigator(), &"locks".into())
        .ok()
        .filter(JsValue::is_object);
    let request = locks.as_ref().and_then(|locks| {
        Reflect::get(locks, &"request".into())
            .ok()
            .and_then(|r| r.dyn_into::<js_sys::Function>().ok())
    });
    let (Some(locks), Some(request)) = (locks, request) else {
        return work.await;
    };
    let granted = wasm_bindgen::closure::Closure::once_into_js(move |lock: JsValue| -> JsValue {
        if lock.is_null() {
            info!("asset copy: an import runs in another tab; what it writes is left alone");
            return JsValue::UNDEFINED;
        }
        wasm_bindgen_futures::future_to_promise(async move {
            work.await;
            Ok(JsValue::UNDEFINED)
        })
        .into()
    });
    let options = js_sys::Object::new();
    let _ = Reflect::set(&options, &"ifAvailable".into(), &true.into());
    let name = JsValue::from(dbota_import::browser::IMPORT_LOCK);
    if let Ok(held) = request
        .call3(&locks, &name, &options, &granted)
        .and_then(|p| p.dyn_into::<Promise>())
    {
        let _ = JsFuture::from(held).await;
    }
}

/// Removes what [`ROOT`] holds besides the copy `kept` and the file naming it.
async fn sweep(root: FileSystemDirectoryHandle, kept: String) {
    let entries = root.keys();
    let mut names = Vec::new();
    while names.len() < 4096 {
        let Ok(next) = entries.next() else { break };
        let Ok(next) = JsFuture::from(next).await else {
            break;
        };
        if Reflect::get(&next, &"done".into()).is_ok_and(|d| d.is_truthy()) {
            break;
        }
        if let Some(name) = Reflect::get(&next, &"value".into())
            .ok()
            .and_then(|v| v.as_string())
        {
            names.push(name);
        }
    }
    let options = web_sys::FileSystemRemoveOptions::new();
    options.set_recursive(true);
    for name in names.into_iter().filter(|n| *n != kept && n != CURRENT) {
        match JsFuture::from(root.remove_entry_with_options(&name, &options)).await {
            Ok(_) => info!("asset copy: removed {name}, left beside the copy"),
            Err(e) => warn!("asset copy: {name} could not be removed: {}", said(&e)),
        }
    }
}

/// The served copy's validated index: `cache/index.txt` next to the page, read no further than an
/// index may be long.
async fn served() -> Result<HashMap<String, u64>, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str("cache/index.txt"))
        .await
        .map_err(|e| said(&e))?
        .unchecked_into();
    if !response.ok() {
        return Err(format!(
            "cache/index.txt returned HTTP {}",
            response.status()
        ));
    }
    let bytes = bounded(&response, MAX_INDEX as usize, "cache/index.txt").await?;
    let index = CacheIndex::parse(&bytes).map_err(|why| why.to_string())?;
    Ok(index
        .files()
        .iter()
        .map(|f| (f.path.clone(), f.size))
        .collect())
}

/// A response's bytes, read as they arrive and given up past `limit`: a served file is no larger
/// than its bound only if its server is honest.
async fn bounded(
    response: &web_sys::Response,
    limit: usize,
    name: &str,
) -> Result<Vec<u8>, String> {
    let length = response.headers().get("content-length").ok().flatten();
    if let Some(length) = length.and_then(|l| l.parse::<u64>().ok())
        && length > limit as u64
    {
        return Err(format!("{name} is {length} bytes, at most {limit}"));
    }
    let body = response
        .body()
        .ok_or_else(|| format!("{name} has no body"))?;
    let reader: web_sys::ReadableStreamDefaultReader = body.get_reader().unchecked_into();
    let mut bytes = Vec::new();
    let read = loop {
        let chunk = match JsFuture::from(reader.read()).await {
            Ok(chunk) => chunk,
            Err(e) => break Err(said(&e)),
        };
        if Reflect::get(&chunk, &"done".into()).is_ok_and(|d| d.is_truthy()) {
            break Ok(());
        }
        let value: Uint8Array = Reflect::get(&chunk, &"value".into())
            .map_err(|e| said(&e))?
            .unchecked_into();
        if value.length() as usize > limit - bytes.len() {
            break Err(format!("{name} exceeds {limit} bytes"));
        }
        let at = bytes.len();
        bytes.resize(at + value.length() as usize, 0);
        value.copy_to(&mut bytes[at..]);
    };
    let _ = JsFuture::from(reader.cancel()).await;
    read.map(|()| bytes)
}

/// Whether the client reads a copy in private storage.
pub fn is_private() -> bool {
    matches!(COPY.get(), Some(Copy::Private { .. }))
}

/// Whether the client reads a served copy.
pub fn is_served() -> bool {
    matches!(COPY.get(), Some(Copy::Served { .. }))
}

/// The folder of the private copy, from the storage's root (`["dbota", "copy-…"]`), for a worker
/// that reads it itself (`web/voice_worker.js`).
pub fn private_folder() -> Option<[String; 2]> {
    match COPY.get()? {
        Copy::Private { folder, .. } => Some([ROOT.to_owned(), folder.clone()]),
        Copy::Served { .. } => None,
    }
}

fn files() -> Option<&'static HashMap<String, u64>> {
    match COPY.get()? {
        Copy::Private { files, .. } | Copy::Served { files } => Some(files),
    }
}

/// Whether the copy's validated index lists `rel`.
pub fn present(rel: &str) -> bool {
    files().is_some_and(|files| files.contains_key(rel))
}

/// The copy's files whose paths start with `prefix`.
pub fn listed(prefix: &str) -> Vec<String> {
    files()
        .map(|files| {
            files
                .keys()
                .filter(|path| path.starts_with(prefix))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// A small file of the private copy, read at the start.
pub fn small(rel: &str) -> Option<Vec<u8>> {
    match COPY.get()? {
        Copy::Private { small, .. } => small.get(rel).cloned(),
        Copy::Served { .. } => None,
    }
}

/// The folder of the private copy holding `path`'s file, found once.
async fn folder_of(parts: &[&str]) -> Result<Option<FileSystemDirectoryHandle>, String> {
    let Some([root, copy]) = private_folder() else {
        return Ok(None);
    };
    let key = parts.join("/");
    if let Some(found) = FOLDERS.with_borrow(|folders| folders.get(&key).cloned()) {
        return Ok(Some(found));
    }
    let storage = web_sys::window().ok_or("no window")?.navigator().storage();
    let mut at: FileSystemDirectoryHandle = JsFuture::from(storage.get_directory())
        .await
        .map_err(|e| said(&e))?
        .unchecked_into();
    for part in [root.as_str(), copy.as_str()]
        .into_iter()
        .chain(parts.iter().copied())
    {
        match JsFuture::from(at.get_directory_handle(part)).await {
            Ok(next) => at = next.unchecked_into(),
            Err(e) if missing(&e) => return Ok(None),
            Err(e) => return Err(said(&e)),
        }
    }
    FOLDERS.with_borrow_mut(|folders| folders.insert(key, at.clone()));
    Ok(Some(at))
}

/// A file's bytes from the private copy, no more than a file of its kind may hold; `Ok(None)`
/// when it is not there.
pub async fn read(rel: &str) -> Result<Option<Vec<u8>>, String> {
    let mut parts: Vec<&str> = rel.split('/').collect();
    let name = parts.pop().unwrap_or_default();
    let Some(dir) = folder_of(&parts).await? else {
        return Ok(None);
    };
    let handle: FileSystemFileHandle = match JsFuture::from(dir.get_file_handle(name)).await {
        Ok(handle) => handle.unchecked_into(),
        Err(e) if missing(&e) => return Ok(None),
        Err(e) => return Err(said(&e)),
    };
    let file: File = JsFuture::from(handle.get_file())
        .await
        .map_err(|e| said(&e))?
        .unchecked_into();
    let limit = dbota_import::pack::size_limit(rel);
    if file.size() > limit as f64 {
        return Err(format!("{rel} is {} bytes, at most {limit}", file.size()));
    }
    let buffer = JsFuture::from(file.array_buffer())
        .await
        .map_err(|e| said(&e))?;
    Ok(Some(Uint8Array::new(&buffer).to_vec()))
}
