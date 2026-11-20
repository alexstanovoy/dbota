//! The first run in a browser: the player picks the Dota 2 folder in the browser's own dialog (the
//! directory picker where the browser has one, else a folder upload field, which every desktop
//! browser has), the import worker (`web/import_worker.js`, `dbota_import_web`) makes the copy in
//! the page's private storage while the frame keeps drawing, and the page is loaded again on it.
//! A browser cannot search the computer for Dota 2, so the screen starts by asking for the folder.

use std::cell::RefCell;

use bevy::prelude::*;
use js_sys::{Function, Object, Reflect};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{FileSystemDirectoryHandle, HtmlInputElement, MessageEvent, Worker};

use super::{FirstRun, Made, Press, Screen, Told, Work};

/// Where a copy is made from: the name of the folder the player picked.
pub(super) type Source = String;
/// Whether the player picked a folder (it is kept here, for the worker).
pub(super) type Picked = bool;

pub(super) const FOUND: &str = "YOUR DOTA 2 FOLDER";
pub(super) const NOT_FOUND: &str = "CHOOSE YOUR DOTA 2 FOLDER";
pub(super) const PRIVACY: &str = "Your Dota 2 files never leave your computer: dbota reads them in this \
                           tab and keeps the converted copy in this browser's private storage \
                           for this site.";

/// The line under [`NOT_FOUND`].
pub(super) fn not_found_line() -> String {
    format!(
        "A browser cannot look for it by itself: choose the folder of your Dota 2 install (free \
         on Steam). {}",
        super::WHERE
    )
}

/// A picked folder as the screen shows it: its name.
pub(super) fn shown_source(name: &Source) -> String {
    name.clone()
}

/// A page cannot quit: no QUIT.
pub(super) fn quit_or_nothing() -> Vec<(&'static str, Press)> {
    Vec::new()
}

/// The buttons while the browser's dialog is open: one closed without a word (an older browser
/// says nothing of it) must not leave the screen waiting.
pub(super) fn choosing_buttons() -> Vec<(&'static str, Press)> {
    vec![("BACK", Press::Back)]
}

/// The key of the page's session storage that makes the next start a requested re-import.
const REIMPORT: &str = "dbota-reimport";

/// Whether this start was asked to be a re-import (the settings' RE-IMPORT GAME FILES); the
/// request is used up.
pub(crate) fn requested() -> bool {
    let Some(storage) = web_sys::window().and_then(|w| w.session_storage().ok().flatten()) else {
        return false;
    };
    let asked = storage.get_item(REIMPORT).ok().flatten().is_some();
    let _ = storage.remove_item(REIMPORT);
    asked
}

/// What only the page keeps: the worker, what the player picked, and the listeners, which hold
/// browser objects and live on the page's one thread.
#[derive(Default)]
struct Page {
    worker: Option<Worker>,
    /// Whether the worker has loaded its importer and listens.
    ready: bool,
    /// The last folder picked: a folder upload's file list or a directory handle.
    picked: Option<Object>,
    /// An import asked for, waiting for the worker (or a debug hold) to post it.
    queued: bool,
    input: Option<HtmlInputElement>,
    /// The worker's and the field's listeners, alive as long as the page.
    listeners: Vec<Closure<dyn FnMut(JsValue)>>,
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::new(Page::default());
}

/// What only the browser keeps in the screen's state.
pub(super) struct Side {
    /// Debug (`--import-hold NAME`): an import waits before it starts until the page's global
    /// `NAME` is set (a test's barrier, or never: then only CANCEL ends it).
    hold: Option<String>,
}

impl Side {
    pub(super) fn new(args: &crate::Args) -> Side {
        Side {
            hold: args
                .import_hold
                .as_ref()
                .map(|name| name.to_string_lossy().into_owned()),
        }
    }
}

/// A browser call's error as text.
fn said(error: &JsValue) -> String {
    error
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| format!("{error:?}"))
}

fn field(value: &JsValue, name: &str) -> JsValue {
    Reflect::get(value, &name.into()).unwrap_or(JsValue::UNDEFINED)
}

/// Starts the import worker, so that its importer arrives while the player reads the screen, and
/// asks for the folder.
pub(super) fn begin(run: &mut FirstRun, _: &crate::Args) {
    run.screen = Screen::Ready;
    if let Err(why) = start_worker(run.tell.clone()) {
        run.trouble(why);
    }
}

/// The worker and what it says, told to the screen.
fn start_worker(tell: super::Tell) -> Result<(), String> {
    let options = web_sys::WorkerOptions::new();
    options.set_type(web_sys::WorkerType::Module);
    let worker = Worker::new_with_options("./import_worker.js", &options)
        .map_err(|e| format!("The importer could not start: {}", said(&e)))?;
    let told = tell.clone();
    let heard = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
        let data = event.unchecked_into::<MessageEvent>().data();
        heard(&data, &told);
    });
    let failed = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
        let message = field(&event, "message").as_string().unwrap_or_default();
        let why = format!(
            "The importer could not start ({message}). Check the connection and load the page \
             again."
        );
        tell.send(Told::Imported(Err(why)));
    });
    worker.set_onmessage(Some(heard.as_ref().unchecked_ref()));
    worker.set_onerror(Some(failed.as_ref().unchecked_ref()));
    PAGE.with_borrow_mut(|page| {
        page.worker = Some(worker);
        page.listeners.extend([heard, failed]);
    });
    Ok(())
}

/// What the worker said, told to the screen.
fn heard(data: &JsValue, tell: &super::Tell) {
    let number = |name: &str| field(data, name).as_f64().unwrap_or(0.0);
    let text = |name: &str| field(data, name).as_string().unwrap_or_default();
    let told = match text("type").as_str() {
        "ready" => {
            PAGE.with_borrow_mut(|page| page.ready = true);
            return;
        }
        "busy" => Told::Said(format!(
            "The server is busy: the importer arrives in {} s...",
            number("seconds")
        )),
        "found" => Told::Found(Some(text("name"))),
        "progress" => Told::Progress(Work {
            done: number("done") as usize,
            total: number("total") as usize,
            item: text("item"),
            bytes: number("bytes") as u64,
        }),
        "done" => {
            let failures = js_sys::Array::from(&field(data, "failures"));
            for line in failures.iter().take(64).filter_map(|l| l.as_string()) {
                warn!("dbota import: {line}");
            }
            Told::Imported(Ok(Made {
                files: number("files") as usize,
                failed: failures.length() as usize,
                stopped: field(data, "stopped").is_truthy(),
            }))
        }
        "error" => Told::Imported(Err(refusal(&text("kind"), &text("message")))),
        _ => return,
    };
    tell.send(told);
}

/// What the player is told when the worker could not make the copy.
fn refusal(kind: &str, message: &str) -> String {
    match kind {
        "not-dota" => format!(
            "\"{message}\" holds no Dota 2: no game/dota/pak01_dir.vpk in it. Choose the \"dota 2 \
             beta\" folder. {}",
            super::WHERE
        ),
        "room" => format!(
            "This browser lets dbota keep only {message} MB more, and the copy needs up to {} MB. \
             Free some disk space (a browser gives a site a share of what is free) and try again, \
             or use the desktop dbota.",
            dbota_import::index::MAX_TOTAL / 1_000_000
        ),
        "busy" => "Another tab of dbota is making the copy now: wait for it to finish, then load \
                   this page again."
            .to_owned(),
        _ => super::failed(message),
    }
}

/// Opens the browser's folder dialog: its directory picker where it has one, else a folder upload
/// field. Called from a press, which the browser takes as the player's gesture.
pub(super) fn ask(run: &mut FirstRun) {
    let tell = run.tell.clone();
    let global = js_sys::global();
    let picker = Reflect::get(&global, &"showDirectoryPicker".into())
        .ok()
        .and_then(|f| f.dyn_into::<Function>().ok());
    if let Some(picker) = picker {
        let options = Object::new();
        let _ = Reflect::set(&options, &"id".into(), &"dota".into());
        let _ = Reflect::set(&options, &"mode".into(), &"read".into());
        let asked = picker
            .call1(&global, &options)
            .and_then(|p| p.dyn_into::<js_sys::Promise>());
        wasm_bindgen_futures::spawn_local(async move {
            let answer = match asked {
                Ok(promise) => wasm_bindgen_futures::JsFuture::from(promise).await,
                Err(e) => Err(e),
            };
            let told = match answer {
                Ok(folder) => {
                    let folder: FileSystemDirectoryHandle = folder.unchecked_into();
                    PAGE.with_borrow_mut(|page| page.picked = Some(folder.into()));
                    Told::Picked(true)
                }
                Err(e) if field(&e, "name").as_string().as_deref() == Some("AbortError") => {
                    Told::Picked(false)
                }
                Err(e) => Told::Imported(Err(format!("The folder dialog failed: {}", said(&e)))),
            };
            tell.send(told);
        });
        return;
    }
    match upload_field(tell) {
        Ok(input) => input.click(),
        Err(why) => run.trouble(why),
    }
}

/// The folder upload field, made once: hidden, its answer told to the screen.
fn upload_field(tell: super::Tell) -> Result<HtmlInputElement, String> {
    if let Some(input) = PAGE.with_borrow(|page| page.input.clone()) {
        input.set_value("");
        return Ok(input);
    }
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("no document")?;
    let input: HtmlInputElement = document
        .create_element("input")
        .map_err(|e| said(&e))?
        .unchecked_into();
    input.set_type("file");
    input.set_multiple(true);
    input.set_webkitdirectory(true);
    input.set_hidden(true);
    let body = document.body().ok_or("no body")?;
    body.append_child(&input).map_err(|e| said(&e))?;
    let field = input.clone();
    let told = tell.clone();
    let changed = Closure::<dyn FnMut(JsValue)>::new(move |_: JsValue| {
        let files = field.files();
        let any = files.as_ref().is_some_and(|f| f.length() > 0);
        PAGE.with_borrow_mut(|page| page.picked = files.map(Into::into));
        let _ = told.send(Told::Picked(any));
    });
    let cancelled = Closure::<dyn FnMut(JsValue)>::new(move |_: JsValue| {
        tell.send(Told::Picked(false));
    });
    input.set_onchange(Some(changed.as_ref().unchecked_ref()));
    let _ = input.add_event_listener_with_callback("cancel", cancelled.as_ref().unchecked_ref());
    PAGE.with_borrow_mut(|page| {
        page.input = Some(input.clone());
        page.listeners.extend([changed, cancelled]);
    });
    Ok(input)
}

/// What the dialog answered: a folder picked is imported at once, the worker finding Dota 2 in
/// it (or saying it holds none).
pub(super) fn picked(run: &mut FirstRun, picked: Picked) {
    if !picked {
        run.screen = Screen::Ready;
        return;
    }
    run.note = None;
    // The worker says where it found Dota 2 in what was picked.
    run.source = None;
    run.screen = Screen::Working(Work::default());
    import(run, true);
}

/// Asks for the import of the folder picked last; the worker gets it once it listens (and a debug
/// hold is lifted). A browser's copy is always made anew: it cannot tell an install's files from
/// the last import's without reading them.
pub(super) fn import(run: &mut FirstRun, _force: bool) {
    PAGE.with_borrow_mut(|page| page.queued = true);
    // Kept in this browser for good where it lets a page ask (Firefox asks the player; Chromium
    // decides by itself): a browser short of space then does not evict it with the site's other
    // data. Asked inside the press, the player's gesture.
    if let Some(window) = web_sys::window()
        && let Ok(asked) = window.navigator().storage().persist()
    {
        wasm_bindgen_futures::spawn_local(async move {
            let kept = wasm_bindgen_futures::JsFuture::from(asked).await;
            info!(
                "asset copy: kept for good: {}",
                kept.map_or_else(|e| said(&e), |k| k.is_truthy().to_string())
            );
        });
    }
    post_queued(run);
}

/// Posts the import asked for once the worker listens and no hold keeps it.
fn post_queued(run: &FirstRun) {
    if let Some(hold) = &run.side.hold
        && !Reflect::get(&js_sys::global(), &hold.into()).is_ok_and(|v| v.is_truthy())
    {
        return;
    }
    PAGE.with_borrow_mut(|page| {
        let (Some(worker), true, true) = (&page.worker, page.ready, page.queued) else {
            return;
        };
        let Some(picked) = &page.picked else {
            return;
        };
        let message = Object::new();
        let _ = Reflect::set(&message, &"op".into(), &"import".into());
        let picked = JsValue::from(picked.clone());
        let (list, folder) = if picked.is_instance_of::<FileSystemDirectoryHandle>() {
            (JsValue::NULL, picked)
        } else {
            (picked, JsValue::NULL)
        };
        let _ = Reflect::set(&message, &"list".into(), &list);
        let _ = Reflect::set(&message, &"folder".into(), &folder);
        page.queued = false;
        if let Err(e) = worker.post_message(&message) {
            let why = format!("The import could not start: {}", said(&e));
            run.tell.send(Told::Imported(Err(why)));
        }
    });
}

/// Asks the worker to stop the copy under way: it removes what it made and says so. One still
/// waiting is just let go.
pub(super) fn cancel(run: &mut FirstRun) -> bool {
    let waiting = PAGE.with_borrow_mut(|page| std::mem::replace(&mut page.queued, false));
    if waiting {
        run.tell.send(Told::Imported(Ok(Made {
            stopped: true,
            ..Made::default()
        })));
        return true;
    }
    PAGE.with_borrow(|page| {
        if let Some(worker) = &page.worker {
            let stop = Object::new();
            let _ = Reflect::set(&stop, &"op".into(), &"stop".into());
            let _ = worker.post_message(&stop);
        }
    });
    true
}

/// A cancel is a request to the worker, which may have named its copy the current one already:
/// what it then says (done, or stopped) is what happened.
pub(super) const CANCEL_IS_FINAL: bool = false;

/// Where the assets that did not convert are named: the browser's console.
pub(super) fn log_place() -> String {
    "The browser's console (F12)".to_owned()
}

/// Posts an import that waited, each frame: for the worker to listen or a hold to be lifted.
pub(super) fn poll(run: Res<FirstRun>) {
    if matches!(run.screen, Screen::Working(_)) {
        post_queued(&run);
    }
}

/// No path is typed in a browser.
pub(super) fn typing() {}

/// Never called: a browser always has a folder dialog.
pub(super) fn take_typed(_: &mut FirstRun) {}

/// Loads the page again: at this screen when `reimport`, else on the copy in place.
pub(super) fn start_again(
    reimport: bool,
    _: Option<&crate::script::Script>,
    _: &mut MessageWriter<AppExit>,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Ok(Some(storage)) = window.session_storage() {
        let _ = if reimport {
            storage.set_item(REIMPORT, "1")
        } else {
            storage.remove_item(REIMPORT)
        };
    }
    info!(
        "first run: loading the page again{}",
        if reimport { " to re-import" } else { "" }
    );
    if let Err(e) = window.location().reload() {
        warn!(
            "first run: the page could not be loaded again: {}",
            said(&e)
        );
    }
}
