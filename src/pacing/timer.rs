use std::time::Duration;

use bevy::platform::time::Instant;

#[cfg(not(target_arch = "wasm32"))]
pub use native::Timer;
#[cfg(target_arch = "wasm32")]
pub use web::Timer;

const MIN_INTERVAL: Duration = Duration::from_millis(16);

fn bounded_deadline(deadline: Instant, now: Instant) -> Instant {
    // A frame can cross a deadline between selecting it and arming the timer.
    debug_assert!(
        now.saturating_duration_since(deadline) <= MIN_INTERVAL,
        "redraw deadline was left in the past"
    );
    deadline.max(now + MIN_INTERVAL)
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::{Arc, Condvar, Mutex, PoisonError};

    use super::{Instant, bounded_deadline};

    #[derive(Default)]
    struct State {
        requested: Option<Instant>,
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
            if state.requested != deadline {
                state.requested = deadline;
                state.deadline = deadline.map(|at| bounded_deadline(at, Instant::now()));
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
                state.requested = None;
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
    use std::{cell::Cell, rc::Rc};

    use wasm_bindgen::{JsCast, closure::Closure};

    use super::{Instant, bounded_deadline};

    type Pending = Rc<Cell<Option<(Instant, i32)>>>;

    pub struct Timer {
        callback: Closure<dyn FnMut()>,
        timeout: Pending,
    }

    impl Timer {
        pub fn new() -> Self {
            let wake = crate::pacing::waker();
            let timeout = Pending::default();
            let armed = timeout.clone();
            Self {
                callback: Closure::new(move || {
                    armed.set(None);
                    wake.wake_by_ref();
                }),
                timeout,
            }
        }

        pub fn set(&mut self, deadline: Option<Instant>) {
            if self.timeout.get().map(|(at, _)| at) == deadline {
                return;
            }
            let Some(window) = web_sys::window() else {
                return;
            };
            if let Some((_, timeout)) = self.timeout.take() {
                window.clear_timeout_with_handle(timeout);
            }
            let Some(deadline) = deadline else { return };
            let now = Instant::now();
            let delay = bounded_deadline(deadline, now).duration_since(now);
            let millis = (delay.as_secs_f64() * 1000.0)
                .ceil()
                .min(f64::from(i32::MAX)) as i32;
            match window.set_timeout_with_callback_and_timeout_and_arguments_0(
                self.callback.as_ref().unchecked_ref(),
                millis,
            ) {
                Ok(timeout) => self.timeout.set(Some((deadline, timeout))),
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

#[cfg(test)]
#[test]
fn imminent_redraw_deadlines_have_a_minimum_interval() {
    let now = Instant::now();
    assert_eq!(bounded_deadline(now, now), now + MIN_INTERVAL);
    assert_eq!(
        bounded_deadline(now - Duration::from_millis(1), now),
        now + MIN_INTERVAL,
    );
    let later = now + Duration::from_secs(1);
    assert_eq!(bounded_deadline(later, now), later);
}

#[cfg(all(test, debug_assertions))]
#[test]
#[should_panic(expected = "redraw deadline was left in the past")]
fn a_stale_redraw_deadline_is_rejected_in_debug_builds() {
    let now = Instant::now();
    bounded_deadline(now - Duration::from_secs(1), now);
}
