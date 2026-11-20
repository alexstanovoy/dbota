//! Where the binary's tests keep their files: under the target folder, never beside the sources.
//! (The integration tests in `tests/` are separate crates that create their own scratch folders, so
//! none needs this; one that ever does can `#[path]`-include this file.)

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::PathBuf;

/// The folder tests keep their files in: `$CARGO_TARGET_DIR`, else this checkout's `target`.
pub(crate) fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
        PathBuf::from,
    )
}

/// A path for a file or folder a test writes, under a `scratch` folder of the target folder (never the
/// source tree), whose parent folders are created: nothing else makes them in a fresh checkout.
pub(crate) fn scratch_path(name: &str) -> PathBuf {
    let path = target_dir().join("scratch").join(name);
    std::fs::create_dir_all(path.parent().expect("a path has a parent"))
        .expect("the scratch folder");
    path
}

struct CountingAllocator;

thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn count_allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(n) = count.get() {
            count.set(Some(n + 1));
        }
    });
}

// SAFETY: each operation forwards the caller's arguments unchanged to System.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, address: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count_allocation();
        unsafe { System.realloc(address, layout, size) }
    }

    unsafe fn dealloc(&self, address: *mut u8, layout: Layout) {
        unsafe { System.dealloc(address, layout) }
    }
}

/// Counts allocations on the calling thread while the schedule runs.
pub(crate) fn count_allocations(run: impl FnOnce()) -> usize {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ALLOCATIONS.with(|count| count.set(None));
        }
    }
    assert_eq!(ALLOCATIONS.with(|count| count.replace(Some(0))), None);
    let _reset = Reset;
    run();
    ALLOCATIONS.with(|count| count.get().expect("counting enabled"))
}
