//! Keeps a log that repeats from filling the disk. A failing sound device
//! makes its driver thread report the same error thousands of times a
//! second (20 GB of log in minutes); any other `warn!` or `error!` stuck in
//! a per-frame path would do the same. Each call site may log a burst, then
//! one line a minute; the rest is dropped, and the game goes on.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use bevy::{
    log::{
        BoxedFmtLayer,
        tracing_subscriber::{self, Layer, filter::DynFilterFn},
    },
    prelude::*,
};

/// Lines a call site may log before it is held to [`QUIET`].
const BURST: u32 = 5;
/// The least time between two lines of a call site past its burst.
const QUIET: Duration = Duration::from_secs(60);
/// Call sites tracked at most; past that, a new one is let through.
const SITES: usize = 4096;

/// What each call site has logged.
#[derive(Default)]
pub struct Throttle {
    sites: HashMap<usize, Site>,
}

struct Site {
    count: u32,
    last: Instant,
}

impl Throttle {
    /// Whether the call site `site` may log now.
    pub fn allow(&mut self, site: usize, now: Instant) -> bool {
        if self.sites.len() >= SITES && !self.sites.contains_key(&site) {
            return true;
        }
        let seen = self.sites.entry(site).or_insert(Site {
            count: 0,
            last: now,
        });
        if seen.count < BURST {
            seen.count += 1;
            seen.last = now;
            return true;
        }
        if now.duration_since(seen.last) >= QUIET {
            seen.last = now;
            return true;
        }
        false
    }
}

/// The filter that holds every event to a [`Throttle`].
fn limit<S>() -> impl tracing_subscriber::layer::Filter<S> {
    let throttle = Mutex::new(Throttle::default());
    // A filter of metadata alone is cached per call site; this one is not.
    DynFilterFn::new(move |meta, _| {
        if !meta.is_event() {
            return true;
        }
        // The call site's address in the binary tells events apart.
        let site = std::ptr::from_ref(meta) as usize;
        throttle
            .lock()
            .map_or(true, |mut t| t.allow(site, Instant::now()))
    })
}

/// The log's formatting layer, as Bevy's default, with every event held to
/// [`Throttle`].
pub fn throttled(_: &mut App) -> Option<BoxedFmtLayer> {
    Some(Box::new(
        tracing_subscriber::fmt::Layer::default()
            .with_writer(std::io::stderr)
            .with_filter(limit()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A call site repeating every frame reaches the log in its burst
    /// only, and another site is not held back by it.
    #[test]
    fn a_site_that_repeats_goes_quiet() {
        use std::sync::{Arc, Mutex};
        use tracing_subscriber::layer::SubscriberExt;

        #[derive(Clone, Default)]
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("sink").extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sink = Sink::default();
        let written = sink.0.clone();
        let layer = tracing_subscriber::fmt::Layer::default()
            .with_writer(move || sink.clone())
            .with_filter(limit());
        let subscriber = tracing_subscriber::registry().with(layer);
        bevy::log::tracing::subscriber::with_default(subscriber, || {
            for _ in 0..100_000 {
                error!("audio stream error");
            }
            error!("another site");
        });
        let text = String::from_utf8(written.lock().expect("sink").clone()).expect("utf8");
        assert_eq!(text.matches("audio stream error").count(), BURST as usize);
        assert_eq!(text.matches("another site").count(), 1);
    }

    /// Past its burst a site logs again once the quiet time has passed.
    #[test]
    fn a_quiet_site_speaks_again_after_a_minute() {
        let mut throttle = Throttle::default();
        let start = Instant::now();
        for _ in 0..BURST {
            assert!(throttle.allow(1, start));
        }
        assert!(!throttle.allow(1, start + QUIET / 2));
        assert!(throttle.allow(1, start + QUIET));
    }
}
