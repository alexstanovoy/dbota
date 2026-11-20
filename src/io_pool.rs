//! The process-wide pool used by the client and its asset-loading fixtures.

use std::sync::OnceLock;

use async_lock::{Semaphore, SemaphoreGuard};
use bevy::tasks::{IoTaskPool, TaskPoolBuilder};

/// A loader thread's stack.
pub const STACK_BYTES: usize = 64 << 20;
const MAX_WORKERS: usize = 4;

/// Loader threads as Bevy's default assigns them to `cores`: a quarter of them, one to four.
pub fn threads_for(cores: usize) -> usize {
    ((cores as f32 / 4.0).round() as usize).clamp(1, MAX_WORKERS)
}

/// Makes the process's IO task pool with `threads` threads of [`STACK_BYTES`], before the app's
/// plugins make it with Bevy's default stacks. The pool is the process's, made once.
pub fn make(threads: usize) {
    IoTaskPool::get_or_init(|| {
        TaskPoolBuilder::default()
            .num_threads(threads)
            .stack_size(STACK_BYTES)
            .thread_name("IO Task Pool".to_string())
            .build()
    });
}

/// [`make`] with the threads Bevy would give this machine.
pub fn make_default() {
    make(threads_for(bevy::tasks::available_parallelism()));
}

/// glTF's texture scope can run another queued model on the current worker.
/// Cap live model readers across all asset servers to bound that stack depth.
pub async fn model_slot() -> SemaphoreGuard<'static> {
    static MODELS: OnceLock<Semaphore> = OnceLock::new();
    MODELS
        .get_or_init(|| {
            make_default();
            Semaphore::new(IoTaskPool::get().thread_num().clamp(1, MAX_WORKERS))
        })
        .acquire()
        .await
}

/// Start the production pool before any app fixture adds plugins that initialize task pools.
/// The first initialization sets the configuration for the entire test process.
#[cfg(test)]
pub fn make_for_tests() {
    make_default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_of_the_cores_from_one_to_four() {
        let threads: Vec<_> = [1, 2, 4, 6, 8, 16, 32, 64].map(threads_for).into();
        assert_eq!(threads, [1, 1, 1, 2, 2, 4, 4, 4]);
    }
}
