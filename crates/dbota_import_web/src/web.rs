//! The importer in a browser, as a Web Worker (`web/import_worker.js` loads this module and calls
//! [`run`]). The client posts the folder the player picked; the worker reads the install's files
//! from it by slices (`File.slice`, never whole), converts them item by item and writes each
//! item's files into a new copy in the page's private storage (the Origin Private File System,
//! through synchronous access handles, which only a worker has), then names that copy the current
//! one (`dbota_import::browser`). Nothing leaves the page: there is no network code in here.
//!
//! Messages in:  `{op: "import", list: FileList | null, folder: FileSystemDirectoryHandle | null,
//!               keep: bool}` (`keep`: the current copy stays until the new one is whole; else it
//!               goes first), and `{op: "stop"}`.
//! Messages out: `{type: "ready"}` once it listens; `{type: "found", name, files}`;
//!               `{type: "progress", done, total, item, bytes}` before each item;
//!               `{type: "done", stopped, files, failures: [line]}`;
//!               `{type: "error", kind, message}` (`kind`: "not-dota", "room", "busy" (another
//!               tab's import runs) or "failed").

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::rc::Rc;

use heatel::{Slice, SliceSource, Source};
use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    DedicatedWorkerGlobalScope, File, FileList, FileReaderSync, FileSystemDirectoryHandle,
    FileSystemFileHandle, FileSystemGetDirectoryOptions, FileSystemGetFileOptions,
    FileSystemReadWriteOptions, FileSystemRemoveOptions, FileSystemSyncAccessHandle, MessageEvent,
    FileSystemWritableFileStream, StorageEstimate,
};

use dbota_import::browser::{self, CURRENT, IMPORT_LOCK, LEVELS, MAX_PICKED, PACKAGE, ROOT};
use dbota_import::import::{self, IDS, Importer, MANIFEST, Plan, Sink};
use dbota_import::{source, voice};

/// The storage an import asks to have free before it starts: the most an import writes, as the
/// desktop asks of its drive (a whole install's copy is about 1.1 GB). A write the browser refuses
/// all the same ends the import unfinished.
const ROOM: f64 = dbota_import::index::MAX_TOTAL as f64;

/// The longest the worker works before it lets a message in (a stop), in milliseconds.
const SLICE_MS: f64 = 50.0;

thread_local! {
    /// Set by a stop message; the import under way ends before its next item.
    static STOPPING: Cell<bool> = const { Cell::new(false) };
    /// Whether an import is under way: one at a time.
    static WORKING: Cell<bool> = const { Cell::new(false) };
}

fn scope() -> DedicatedWorkerGlobalScope {
    js_sys::global().unchecked_into()
}

/// Sends the page a message of `kind` with `fields`.
fn post(kind: &str, fields: &[(&str, JsValue)]) {
    let message = Object::new();
    let _ = Reflect::set(&message, &"type".into(), &kind.into());
    for (name, value) in fields {
        let _ = Reflect::set(&message, &(*name).into(), value);
    }
    let _ = scope().post_message(&message);
}

/// Starts listening to the client: its import and stop messages.
#[wasm_bindgen]
pub fn run() {
    let heard = Closure::<dyn FnMut(MessageEvent)>::new(|event: MessageEvent| {
        let data = event.data();
        let op = Reflect::get(&data, &"op".into())
            .ok()
            .and_then(|o| o.as_string());
        match op.as_deref() {
            Some("stop") => STOPPING.set(true),
            Some("import") if !WORKING.replace(true) => {
                STOPPING.set(false);
                wasm_bindgen_futures::spawn_local(async move {
                    let ended = locked(import_from(data)).await.unwrap_or_else(|| {
                        Err(Failure {
                            kind: "busy",
                            message: String::new(),
                        })
                    });
                    if let Err(Failure { kind, message }) = ended {
                        post(
                            "error",
                            &[("kind", kind.into()), ("message", message.into())],
                        );
                    }
                    WORKING.set(false);
                });
            }
            _ => {}
        }
    });
    scope().set_onmessage(Some(heard.as_ref().unchecked_ref()));
    // The worker lives as long as the page keeps it: so does its listener.
    heard.forget();
    post("ready", &[]);
}

/// Runs `work` holding [`IMPORT_LOCK`]; no copy may be written without this lock.
async fn locked<T: 'static>(work: impl Future<Output = T> + 'static) -> Option<T> {
    let locks = Reflect::get(&scope().navigator(), &"locks".into())
        .ok()
        .filter(|locks| locks.is_object());
    let request = locks.as_ref().and_then(|locks| {
        Reflect::get(locks, &"request".into())
            .ok()
            .and_then(|r| r.dyn_into::<Function>().ok())
    });
    let (Some(locks), Some(request)) = (locks, request) else {
        return None;
    };
    let result = Rc::new(RefCell::new(None));
    let kept = result.clone();
    let granted = Closure::once_into_js(move |lock: JsValue| -> JsValue {
        if lock.is_null() {
            return JsValue::UNDEFINED;
        }
        wasm_bindgen_futures::future_to_promise(async move {
            *kept.borrow_mut() = Some(work.await);
            Ok(JsValue::UNDEFINED)
        })
        .into()
    });
    let options = Object::new();
    let _ = Reflect::set(&options, &"ifAvailable".into(), &true.into());
    let held = request.call3(&locks, &IMPORT_LOCK.into(), &options, &granted);
    if let Ok(held) = held.and_then(|p| p.dyn_into::<Promise>()) {
        let _ = JsFuture::from(held).await;
    }
    result.borrow_mut().take()
}

/// Why an import did not finish: what kind of trouble (the client words it), and the details.
struct Failure {
    kind: &'static str,
    message: String,
}

impl Failure {
    fn failed(message: impl Into<String>) -> Failure {
        Failure {
            kind: "failed",
            message: message.into(),
        }
    }
}

/// A browser call's error as text.
fn said(error: JsValue) -> String {
    error
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| format!("{error:?}"))
}

async fn wait(promise: Promise) -> Result<JsValue, String> {
    JsFuture::from(promise).await.map_err(said)
}

/// A picked file, read by slices through the synchronous reader of a worker (one reader serves
/// every file).
struct Picked {
    file: File,
    reader: Rc<FileReaderSync>,
}

impl Slice for Picked {
    fn size(&self) -> u64 {
        self.file.size() as u64
    }

    fn read_at(&self, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
        let to_io = |e: JsValue| std::io::Error::other(said(e));
        let slice = self
            .file
            .slice_with_f64_and_f64(offset as f64, (offset + len as u64) as f64)
            .map_err(to_io)?;
        let buffer = self.reader.read_as_array_buffer(&slice).map_err(to_io)?;
        let bytes = Uint8Array::new(&buffer).to_vec();
        if bytes.len() == len {
            Ok(bytes)
        } else {
            Err(std::io::ErrorKind::UnexpectedEof.into())
        }
    }
}

/// The install's files the importer reads, by path relative to its `game/dota` folder, and the
/// name of the folder the player picked.
struct Install {
    name: String,
    files: Vec<(String, File)>,
}

/// The files of a folder picked whole (`<input webkitdirectory>`).
fn from_list(list: &FileList) -> Install {
    let all: Vec<File> = (0..list.length()).filter_map(|at| list.item(at)).collect();
    let paths: Vec<String> = all
        .iter()
        .map(|file| {
            let path = Reflect::get(file, &"webkitRelativePath".into())
                .ok()
                .and_then(|p| p.as_string())
                .unwrap_or_default();
            if path.is_empty() { file.name() } else { path }
        })
        .collect();
    let name = paths
        .first()
        .and_then(|path| path.split('/').next())
        .unwrap_or_default()
        .to_owned();
    let files = browser::picked_files(paths.iter().map(String::as_str))
        .into_iter()
        .map(|(at, relative)| (relative, all[at].clone()))
        .collect();
    Install { name, files }
}

/// The handle of `name` in `dir`, if it is a folder there.
async fn folder_in(
    dir: &FileSystemDirectoryHandle,
    name: &str,
) -> Option<FileSystemDirectoryHandle> {
    wait(dir.get_directory_handle(name))
        .await
        .ok()
        .map(JsCast::unchecked_into)
}

/// The entries of a folder: `(name, handle)`, at most `limit`.
async fn entries(
    dir: &FileSystemDirectoryHandle,
    limit: usize,
) -> Result<Vec<(String, JsValue)>, String> {
    let iterator = dir.entries();
    let mut found = Vec::new();
    while found.len() < limit {
        let next = wait(iterator.next().map_err(said)?).await?;
        if Reflect::get(&next, &"done".into()).is_ok_and(|done| done.is_truthy()) {
            break;
        }
        let pair: Array = Reflect::get(&next, &"value".into())
            .map_err(said)?
            .unchecked_into();
        if let Some(name) = pair.get(0).as_string() {
            found.push((name, pair.get(1)));
        }
    }
    Ok(found)
}

/// The files the importer reads below `dir` (its `game/dota`), by path from it: the packages at
/// the top and in `maps/`, the typefaces of `panorama/fonts/`.
async fn files_below(dir: &FileSystemDirectoryHandle) -> Result<Vec<(String, File)>, String> {
    let mut files = Vec::new();
    let mut folders = vec![(String::new(), dir.clone())];
    for sub in ["maps", "panorama"] {
        if let Some(found) = folder_in(dir, sub).await {
            folders.push((format!("{sub}/"), found));
        }
    }
    if let Some(panorama) = folders
        .iter()
        .find(|(path, _)| path == "panorama/")
        .map(|(_, d)| d.clone())
        && let Some(fonts) = folder_in(&panorama, "fonts").await
    {
        folders.push(("panorama/fonts/".to_owned(), fonts));
    }
    for (prefix, folder) in folders {
        for (name, handle) in entries(&folder, MAX_PICKED * 4).await? {
            let path = format!("{prefix}{name}");
            if files.len() < MAX_PICKED
                && source::picked(&path)
                && handle.is_instance_of::<FileSystemFileHandle>()
            {
                let handle: FileSystemFileHandle = handle.unchecked_into();
                let file: File = wait(handle.get_file()).await?.unchecked_into();
                files.push((path, file));
            }
        }
    }
    Ok(files)
}

/// The files of a folder picked with the browser's directory picker: its `game/dota` at one of
/// the levels a player picks ([`LEVELS`]).
async fn from_folder(picked: &FileSystemDirectoryHandle) -> Result<Install, String> {
    let name = picked.name();
    for level in LEVELS {
        let mut dir = Some(picked.clone());
        for step in level {
            dir = match dir {
                Some(d) => folder_in(&d, step).await,
                None => None,
            };
        }
        let Some(dir) = dir else { continue };
        if wait(dir.get_file_handle(PACKAGE)).await.is_ok() {
            let files = files_below(&dir).await?;
            return Ok(Install { name, files });
        }
    }
    Ok(Install {
        name,
        files: Vec::new(),
    })
}

/// The bytes of storage this page may still use, when the browser says.
async fn room() -> Option<f64> {
    let storage = scope().navigator().storage();
    let estimate: StorageEstimate = wait(storage.estimate().ok()?).await.ok()?.unchecked_into();
    Some(estimate.get_quota()? - estimate.get_usage().unwrap_or(0.0))
}

/// Files an item made, waiting to be written: the storage's files are made asynchronously, so
/// they are written after the item, not as the importer hands them over.
#[derive(Default)]
struct Collected(Vec<(String, Vec<u8>)>);

impl Sink for Collected {
    fn write(&mut self, path: &str, data: &[u8]) -> heatel::Result<()> {
        self.0.push((path.to_owned(), data.to_vec()));
        Ok(())
    }

    /// Kept as it is, not copied: at most one item's files are in memory at once.
    fn write_owned(&mut self, path: &str, data: Vec<u8>) -> heatel::Result<()> {
        self.0.push((path.to_owned(), data));
        Ok(())
    }
}

/// A copy's folder, with the handles of its sub-folders kept once made.
struct Copy {
    root: FileSystemDirectoryHandle,
    folders: HashMap<String, FileSystemDirectoryHandle>,
}

impl Copy {
    /// The handle of the folder of `path`, made as needed.
    async fn folder_of(&mut self, path: &str) -> Result<FileSystemDirectoryHandle, String> {
        let Some((parent, _)) = path.rsplit_once('/') else {
            return Ok(self.root.clone());
        };
        if let Some(found) = self.folders.get(parent) {
            return Ok(found.clone());
        }
        let mut dir = self.root.clone();
        let options = FileSystemGetDirectoryOptions::new();
        options.set_create(true);
        for part in parent.split('/') {
            dir = wait(dir.get_directory_handle_with_options(part, &options))
                .await?
                .unchecked_into();
        }
        self.folders.insert(parent.to_owned(), dir.clone());
        Ok(dir)
    }

    /// Writes `bytes` as the copy's file at `path`; any failure is the import's end.
    async fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        // The importer writes only such paths; this checks again before the storage is touched.
        if !import::safe_path(path) {
            return Err(format!("{path:?} is not a path a copy may hold"));
        }
        let dir = self.folder_of(path).await?;
        let name = path.rsplit('/').next().unwrap_or(path);
        write_file(&dir, name, bytes)
            .await
            .map_err(|why| format!("{path}: {why}"))
    }
}

/// Writes `bytes` as the file `name` of `dir`, whole, through a synchronous access handle.
async fn write_file(
    dir: &FileSystemDirectoryHandle,
    name: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let options = FileSystemGetFileOptions::new();
    options.set_create(true);
    let handle: FileSystemFileHandle = wait(dir.get_file_handle_with_options(name, &options))
        .await?
        .unchecked_into();
    let sync: FileSystemSyncAccessHandle = wait(handle.create_sync_access_handle())
        .await?
        .unchecked_into();
    let written = (|| {
        sync.truncate_with_f64(0.0).map_err(said)?;
        let at = FileSystemReadWriteOptions::new();
        at.set_at(0.0);
        let written = sync
            .write_with_u8_array_and_options(bytes, &at)
            .map_err(said)?;
        if written as usize != bytes.len() {
            return Err(format!("{written} of {} bytes written", bytes.len()));
        }
        sync.flush().map_err(said)
    })();
    sync.close();
    written
}

/// The browser commits a writable stream on close. A refused write leaves the old pointer whole.
async fn publish_current(root: &FileSystemDirectoryHandle, name: &str) -> Result<bool, String> {
    let options = FileSystemGetFileOptions::new();
    options.set_create(true);
    let handle: FileSystemFileHandle = wait(root.get_file_handle_with_options(CURRENT, &options))
        .await?.unchecked_into();
    let stream: FileSystemWritableFileStream = wait(handle.create_writable()).await?.unchecked_into();
    if let Err(why) = wait(stream.write_with_str(&format!("{name}\n")).map_err(said)?).await {
        let _ = wait(stream.abort()).await;
        return Err(why);
    }
    if STOPPING.get() {
        wait(stream.abort()).await?;
        return Ok(false);
    }
    wait(stream.close()).await?;
    Ok(true)
}

fn missing(error: &JsValue) -> bool {
    Reflect::get(error, &"name".into()).ok().and_then(|name| name.as_string()).as_deref()
        == Some("NotFoundError")
}

/// Removes `name` from `dir`, with all it holds; a name not there is no error.
async fn remove(dir: &FileSystemDirectoryHandle, name: &str) -> Result<(), String> {
    let options = FileSystemRemoveOptions::new();
    options.set_recursive(true);
    match JsFuture::from(dir.remove_entry_with_options(name, &options)).await {
        Ok(_) => Ok(()),
        Err(e) if missing(&e) => Ok(()),
        Err(e) => Err(said(e)),
    }
}

/// Cleans up superseded or incomplete copies while holding the import lock.
async fn sweep(root: &FileSystemDirectoryHandle, keep: Option<&str>) -> Result<(), String> {
    for (name, entry) in entries(root, 4096).await? {
        if Some(name.as_str()) != keep
            && browser::current_copy(&name).is_some()
            && entry.is_instance_of::<FileSystemDirectoryHandle>()
        {
            remove(root, &name).await?;
        }
    }
    Ok(())
}

/// The copy [`CURRENT`] names. An unreadable pointer must not authorize cleanup.
async fn current_copy(root: &FileSystemDirectoryHandle) -> Result<Option<String>, String> {
    let handle: FileSystemFileHandle = match JsFuture::from(root.get_file_handle(CURRENT)).await {
        Ok(handle) => handle.unchecked_into(),
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(said(error)),
    };
    let file: File = wait(handle.get_file()).await?.unchecked_into();
    if file.size() > browser::MAX_CURRENT as f64 {
        return Ok(None);
    }
    let text = wait(file.text()).await?.as_string().ok_or("the cache pointer could not be read")?;
    Ok(browser::current_copy(&text).map(str::to_owned))
}

/// Lets a message in (a stop) once [`SLICE_MS`] of work has passed since the last time.
async fn breathe(since: &mut f64) {
    let now = js_sys::Date::now();
    if now - *since < SLICE_MS {
        return;
    }
    let tick = Promise::new(&mut |resolve, _| {
        let _ = scope().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0);
    });
    let _ = JsFuture::from(tick).await;
    *since = js_sys::Date::now();
}

/// The import the client asked for: the install found in what it posted, read and written as a
/// new copy, which becomes the current one only once whole.
async fn import_from(data: JsValue) -> Result<(), Failure> {
    let list = Reflect::get(&data, &"list".into())
        .ok()
        .and_then(|l| l.dyn_into::<FileList>().ok());
    let folder = Reflect::get(&data, &"folder".into())
        .ok()
        .and_then(|f| f.dyn_into::<FileSystemDirectoryHandle>().ok());
    let install = match (list, folder) {
        (Some(list), _) => from_list(&list),
        (None, Some(folder)) => from_folder(&folder).await.map_err(Failure::failed)?,
        (None, None) => return Err(Failure::failed("no folder was given")),
    };
    let Some(package) = install.files.iter().find(|(path, _)| path == PACKAGE) else {
        return Err(Failure {
            kind: "not-dota",
            message: install.name,
        });
    };
    let stamp = format!(
        "{}/{}/{}",
        import::fingerprint(),
        package.1.size() as u64,
        package.1.last_modified()
    );
    post(
        "found",
        &[
            ("name", install.name.clone().into()),
            ("files", install.files.len().into()),
        ],
    );
    let storage = scope().navigator().storage();
    let opfs: FileSystemDirectoryHandle = wait(storage.get_directory())
        .await
        .map_err(Failure::failed)?
        .unchecked_into();
    let options = FileSystemGetDirectoryOptions::new();
    options.set_create(true);
    let root: FileSystemDirectoryHandle =
        wait(opfs.get_directory_handle_with_options(ROOT, &options))
            .await
            .map_err(Failure::failed)?
            .unchecked_into();
    // Keep even a stale published copy until its successor has committed.
    let kept = current_copy(&root).await.map_err(Failure::failed)?;
    sweep(&root, kept.as_deref()).await.map_err(Failure::failed)?;
    if let Some(free) = room().await
        && free < ROOM
    {
        return Err(Failure {
            kind: "room",
            message: format!("{:.0}", free / 1e6),
        });
    }
    let millis = js_sys::Date::now() as u64;
    let mut name = browser::copy_name(millis);
    if kept.as_deref() == Some(name.as_str()) {
        name = browser::copy_name(millis.saturating_add(1));
    }
    let copy_root: FileSystemDirectoryHandle =
        wait(root.get_directory_handle_with_options(&name, &options))
            .await
            .map_err(Failure::failed)?
            .unchecked_into();
    let mut copy = Copy {
        root: copy_root,
        folders: HashMap::new(),
    };
    let made = convert(install.files, &mut copy, &stamp).await;
    let ended = match made {
        Ok(Some(report)) if !STOPPING.get() => match publish_current(&root, &name).await {
            Ok(true) => {
                if let Err(why) = sweep(&root, Some(&name)).await {
                    web_sys::console::warn_1(&format!("The cache was published but old copies could not be removed: {why}").into());
                }
                Ok(Some(report))
            }
            Ok(false) => Ok(None),
            Err(why) => Err(why),
        },
        Ok(Some(_)) => Ok(None),
        other => other,
    };
    match ended {
        Ok(Some(report)) => {
            for (item, why) in &report.warnings {
                web_sys::console::warn_1(&format!("Asset import: {item}: {why}").into());
            }
            let failures: Array = report
                .failed
                .iter()
                .map(|(item, why)| JsValue::from(format!("{item}: {why}")))
                .collect();
            post(
                "done",
                &[
                    ("stopped", false.into()),
                    ("files", report.files.len().into()),
                    ("failures", failures.into()),
                ],
            );
            Ok(())
        }
        Ok(None) => {
            remove(&root, &name).await.map_err(Failure::failed)?;
            post(
                "done",
                &[
                    ("stopped", true.into()),
                    ("files", 0.into()),
                    ("failures", Array::new().into()),
                ],
            );
            Ok(())
        }
        Err(why) => {
            // Nothing of it is a copy (no index, no stamp, not named): its space goes.
            let _ = remove(&root, &name).await;
            Err(Failure::failed(why))
        }
    }
}

/// Converts the install's `files` into `copy`, its index and stamp last; the report, or nothing
/// when the client stopped it.
async fn convert(
    files: Vec<(String, File)>,
    copy: &mut Copy,
    stamp: &str,
) -> Result<Option<import::Report>, String> {
    let reader = Rc::new(FileReaderSync::new().map_err(said)?);
    let mut source = SliceSource::default();
    for (path, file) in files {
        source.insert(
            path,
            Picked {
                file,
                reader: reader.clone(),
            },
        );
    }
    if Source::size(&source, PACKAGE).is_err() {
        return Err(format!("{PACKAGE} is not in the chosen folder"));
    }
    let plan = Plan::parse(MANIFEST, IDS).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let mut importer = Importer::new(source, plan).map_err(|e| e.to_string())?;
    let mut since = js_sys::Date::now();
    loop {
        post(
            "progress",
            &[
                ("done", importer.done().into()),
                ("total", importer.total().into()),
                ("item", importer.upcoming().unwrap_or_default().into()),
                ("bytes", (importer.bytes_written() as f64).into()),
            ],
        );
        breathe(&mut since).await;
        if STOPPING.get() {
            return Ok(None);
        }
        let mut made = Collected::default();
        if !importer.step(&mut made) {
            break;
        }
        // A file that cannot be written ends the import: the copy stays unfinished (no index, no
        // stamp, not named), never one whose index lists files that are not there.
        for (path, bytes) in made.0 {
            copy.write(&path, &bytes).await?;
        }
    }
    let report = importer.finish();
    // An install that asked for more than an import reads finishes no copy.
    if let Some(cut) = &report.cut {
        return Err(cut.clone());
    }
    copy.write(import::INDEX, report.index().as_bytes()).await?;
    copy.write(import::STAMP, stamp.as_bytes()).await?;
    Ok(Some(report))
}

/// The most bytes of a hero's voice pack: what the voice worker reads at most.
#[wasm_bindgen]
pub fn voice_pack_limit() -> usize {
    voice::pack::MAX_PACK_BYTES
}

/// The records of `hero`'s voice pack, checked whole with the reader the native client uses
/// ([`voice::open`]): `[name, start, end]` each, the record's place in `bytes`. The
/// voice worker hands each record to the page by itself.
#[wasm_bindgen]
pub fn voice_pack_records(hero: &str, bytes: &[u8]) -> Result<Array, JsError> {
    voice::open(hero, bytes).map_err(|why| JsError::new(&why))?;
    let records = voice::pack::read(bytes).map_err(|why| JsError::new(&why.to_string()))?;
    Ok(records
        .into_iter()
        .map(|record| {
            Array::of3(
                &record.name.into(),
                &(record.data.start as u32).into(),
                &(record.data.end as u32).into(),
            )
        })
        .collect())
}
