use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

use bevy::{
    prelude::*,
    winit::{EventLoopProxyWrapper, WinitUserEvent},
};

thread_local! {
    static WINDOW: RefCell<Weak<WakeWindow>> = const { RefCell::new(Weak::new()) };
}

struct WakeWindow {
    pending: AtomicBool,
    continuous: AtomicBool,
    send: Box<dyn Fn() + Send + Sync>,
}

#[derive(Resource)]
struct WindowWake(Arc<WakeWindow>);

impl Wake for WakeWindow {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if !self.continuous.load(Ordering::Acquire) && !self.pending.swap(true, Ordering::AcqRel) {
            (self.send)();
        }
    }
}

pub(super) fn install(app: &mut App) {
    if let Some(proxy) = app.world().get_resource::<EventLoopProxyWrapper>() {
        let proxy = (**proxy).clone();
        let wake = Arc::new(WakeWindow {
            pending: AtomicBool::new(false),
            continuous: AtomicBool::new(false),
            send: Box::new(move || {
                let _ = proxy.send_event(WinitUserEvent::WakeUp);
            }),
        });
        WINDOW.with_borrow_mut(|slot| *slot = Arc::downgrade(&wake));
        app.insert_resource(WindowWake(wake))
            .add_systems(First, |wake: Res<WindowWake>| {
                wake.0.pending.store(false, Ordering::Release);
            });
    }
}

pub(super) fn set_continuous(continuous: bool) {
    WINDOW.with_borrow(|window| {
        if let Some(window) = window.upgrade() {
            window.continuous.store(continuous, Ordering::Release);
        }
    });
}

/// Capture on the app's thread before starting work that the frame loop will take in.
pub fn waker() -> Waker {
    WINDOW.with_borrow(|window| {
        window
            .upgrade()
            .map_or_else(|| Waker::noop().clone(), Waker::from)
    })
}

pub(crate) fn poll<F: Future + Unpin>(future: &mut F) -> Option<F::Output> {
    let waker = waker();
    match Pin::new(future).poll(&mut Context::from_waker(&waker)) {
        Poll::Ready(value) => Some(value),
        Poll::Pending => None,
    }
}
