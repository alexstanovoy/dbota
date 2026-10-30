//! The asset loaders' threads, with stacks of their own size.
//!
//! Bevy's glTF loader waits for a model's textures by running the pool's other tasks on the
//! loading thread, so the models being loaded nest on one stack, deeper the fewer loader
//! threads there are. A thread's default 2 MiB held it with four loader threads and overflowed
//! with fewer (a machine, or a job, with few cores gets one loader thread per four cores), the
//! client aborting while it loaded its models. Stacks are address space until used, so these
//! are roomy.

use bevy::tasks::{IoTaskPool, TaskPoolBuilder};

/// A loader thread's stack.
pub const STACK_BYTES: usize = 64 << 20;

/// Loader threads as Bevy's default assigns them to `cores`: a quarter of them, one to four.
pub fn threads_for(cores: usize) -> usize {
    ((cores as f32 / 4.0).round() as usize).clamp(1, 4)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_of_the_cores_from_one_to_four() {
        let threads: Vec<_> = [1, 2, 4, 6, 8, 16, 32, 64].map(threads_for).into();
        assert_eq!(threads, [1, 1, 1, 2, 2, 4, 4, 4]);
    }
}
