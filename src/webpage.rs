//! What a browser build tells the page around it (`globalThis.dbota_page`, web/main.js): a
//! panic, an allocation the browser refused and the first frame. A failure then shows on the
//! page as a message saying what to do, not as a canvas that stays black; the console has the
//! details as well.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    // `catch`: a page without the hooks (or one that throws) must not unwind through the
    // client.
    #[wasm_bindgen(js_namespace = dbota_page, js_name = panicked, catch)]
    fn page_panicked(report: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(js_namespace = dbota_page, js_name = refused, catch)]
    fn page_refused(requested: f64, heap: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(js_namespace = dbota_page, js_name = running, catch)]
    fn page_running() -> Result<(), JsValue>;
}

pub fn plugin(app: &mut App) {
    app.add_systems(Last, first_frame.run_if(run_once));
}

/// Hands every panic to the page after the hook already set (Bevy's, which writes it to the
/// console). A panic ends a browser build (it aborts), so the page shows it as the end.
pub fn report_panics() {
    let console = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        console(info);
        let _ = page_panicked(&info.to_string());
    }));
}

/// The first update runs once the renderer is made (Bevy waits for the browser's GPU before
/// it), so the page can take its "starting" notice away.
fn first_frame() {
    let _ = page_running();
}

/// The system allocator, telling the page when the browser gives the heap no more memory.
/// A refusal is not always the end (a reader may try to reserve and fall back), so the page
/// keeps it and names it only if the client then stops.
struct Reporting;

static TELLING: AtomicBool = AtomicBool::new(false);

fn refused(requested: usize) {
    // Telling the page may itself allocate (the bindings' value table): a refusal while
    // telling is not told again.
    if TELLING.swap(true, Ordering::Relaxed) {
        return;
    }
    let heap = core::arch::wasm32::memory_size(0) as f64 * 65_536.0;
    let _ = page_refused(requested as f64, heap);
    TELLING.store(false, Ordering::Relaxed);
}

// SAFETY: every call goes to `System` with the caller's own arguments; only a null result is
// looked at, after the fact.
unsafe impl GlobalAlloc for Reporting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract for `alloc` is passed on unchanged.
        let ptr = unsafe { System.alloc(layout) };
        if ptr.is_null() {
            refused(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as `alloc`.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if ptr.is_null() {
            refused(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as `alloc`; `ptr` and `layout` are the caller's, from this allocator.
        let moved = unsafe { System.realloc(ptr, layout, new_size) };
        if moved.is_null() {
            refused(new_size);
        }
        moved
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` are the caller's, from this allocator.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Reporting = Reporting;
