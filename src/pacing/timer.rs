use bevy::platform::time::Instant;

#[cfg(not(target_arch = "wasm32"))]
pub use native::Timer;
#[cfg(target_arch = "wasm32")]
pub use web::Timer;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::{Arc, Condvar, Mutex, PoisonError};

    use super::Instant;

    #[derive(Default)]
    struct State {
        deadline: Option<Instant>,
        finished: bool,
    }

    pub struct Timer {
        state: Arc<(Mutex<State>, Condvar)>,
        worker: Option<std::thread::JoinHandle<()>>,
    }

    impl Timer {
        pub fn new() -> Self {
            Self {
                state: Arc::default(),
                worker: None,
            }
        }

        pub fn set(&mut self, deadline: Option<Instant>) {
            if self.worker.is_none() {
                if deadline.is_none() {
                    return;
                }
                let state = self.state.clone();
                let wake = crate::pacing::waker();
                match std::thread::Builder::new()
                    .name("dbota redraw timer".into())
                    .spawn(move || {
                        wait(&state, &wake);
                    }) {
                    Ok(worker) => self.worker = Some(worker),
                    Err(error) => {
                        bevy::log::error!("could not start redraw timer: {error}");
                        return;
                    }
                }
            }
            let (state, changed) = &*self.state;
            let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.deadline != deadline {
                state.deadline = deadline;
                changed.notify_one();
            }
        }
    }

    impl Drop for Timer {
        fn drop(&mut self) {
            if let Some(worker) = self.worker.take() {
                let (state, changed) = &*self.state;
                state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .finished = true;
                changed.notify_one();
                let _ = worker.join();
            }
        }
    }

    fn wait((state, changed): &(Mutex<State>, Condvar), wake: &std::task::Waker) {
        let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
        while !state.finished {
            let Some(deadline) = state.deadline else {
                state = changed.wait(state).unwrap_or_else(PoisonError::into_inner);
                continue;
            };
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                state.deadline = None;
                wake.wake_by_ref();
            } else {
                state = changed
                    .wait_timeout(state, left)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::{JsCast, closure::Closure};

    use super::Instant;

    pub struct Timer {
        callback: Closure<dyn FnMut()>,
        timeout: Option<(Instant, i32)>,
    }

    impl Timer {
        pub fn new() -> Self {
            let wake = crate::pacing::waker();
            Self {
                callback: Closure::new(move || wake.wake_by_ref()),
                timeout: None,
            }
        }

        pub fn set(&mut self, deadline: Option<Instant>) {
            if self.timeout.map(|(at, _)| at) == deadline {
                return;
            }
            let Some(window) = web_sys::window() else {
                return;
            };
            if let Some((_, timeout)) = self.timeout.take() {
                window.clear_timeout_with_handle(timeout);
            }
            let Some(deadline) = deadline else { return };
            let delay = deadline.saturating_duration_since(Instant::now());
            let millis = (delay.as_secs_f64() * 1000.0)
                .ceil()
                .min(f64::from(i32::MAX)) as i32;
            match window.set_timeout_with_callback_and_timeout_and_arguments_0(
                self.callback.as_ref().unchecked_ref(),
                millis,
            ) {
                Ok(timeout) => self.timeout = Some((deadline, timeout)),
                Err(error) => bevy::log::error!("could not schedule redraw: {error:?}"),
            }
        }
    }

    impl Drop for Timer {
        fn drop(&mut self) {
            self.set(None);
        }
    }
}
