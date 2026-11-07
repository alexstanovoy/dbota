//! The window as Dota keeps it: windowed fullscreen by default (the display
//! mode of the settings, applied live), and the cursor confined to the
//! window while a match is on screen and the window has focus, so the edge
//! pan works on every edge and the mouse never leaves through the top. The
//! cursor is let go in the menus, with the settings overlay open and when
//! the window loses focus (Alt-Tab).
//!
//! A browser's canvas is the page's, never borderless by itself. There
//! "windowed fullscreen" is the Fullscreen API on the canvas (`web`), there
//! is no exclusive mode, and the cursor is kept in the game by locking the
//! pointer (`web_cursor`) when and where the desktop confines it.

use bevy::{
    prelude::*,
    window::{MonitorSelection, VideoModeSelection, WindowMode},
};

use crate::config::DisplayMode;

pub fn plugin(app: &mut App) {
    app.init_resource::<Displays>();
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(
        Update,
        (native::watch_monitor, native::apply_mode, native::confine).chain(),
    );
    #[cfg(target_arch = "wasm32")]
    {
        app.init_non_send::<web::State>()
            .add_plugins(crate::web_cursor::plugin);
        app.add_systems(Update, web::sync_fullscreen);
    }
}

/// The sizes the monitor offers, which the Resolution setting picks from.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct Displays {
    /// The monitor's own size: what windowed fullscreen has, and what no choice means.
    pub native: (u32, u32),
    /// Ascending, once each, none larger than the monitor, the monitor's own among them.
    sizes: Vec<(u32, u32)>,
}

impl Displays {
    /// The sizes of `modes` (one of each, whatever their refresh rates) up to `native`.
    pub fn new(native: (u32, u32), modes: impl IntoIterator<Item = (u32, u32)>) -> Displays {
        let mut sizes: Vec<(u32, u32)> = modes
            .into_iter()
            .chain([native])
            .filter(|(w, h)| *w <= native.0 && *h <= native.1)
            .collect();
        sizes.sort_unstable();
        sizes.dedup();
        Displays { native, sizes }
    }

    #[cfg(test)]
    pub fn sizes(&self) -> &[(u32, u32)] {
        &self.sizes
    }

    /// `size` cut down to the monitor's own: no window is made larger than the screen it opens on
    /// (a garbled file must not ask the renderer for a 16K surface).
    pub fn fit(&self, (width, height): (u32, u32)) -> (u32, u32) {
        (width.min(self.native.0), height.min(self.native.1))
    }

    /// The size the game runs at under `settings`: the monitor's own in windowed fullscreen; in a
    /// window the saved size as it is, any size being a valid window; in exclusive fullscreen the
    /// saved size when the monitor lists it, else the largest listed size at or below it (the
    /// monitor's own when none is).
    pub fn in_use(&self, settings: &crate::config::Settings) -> (u32, u32) {
        let Some(saved) = settings.resolution else {
            return self.native;
        };
        match settings.display {
            DisplayMode::Borderless => self.native,
            DisplayMode::Windowed => self.fit(saved),
            DisplayMode::Exclusive => self
                .sizes
                .iter()
                .rev()
                .find(|(w, h)| *w <= saved.0 && *h <= saved.1)
                .copied()
                .unwrap_or(self.native),
        }
    }

    /// What the open list of resolutions holds, in order: the monitor's sizes, and the saved size
    /// in its sorted place when the monitor does not list it (a window's, cut to the monitor's).
    pub fn entries(&self, settings: &crate::config::Settings) -> Vec<SizeEntry> {
        let mut entries: Vec<SizeEntry> = self
            .sizes
            .iter()
            .map(|size| SizeEntry {
                size: *size,
                note: SizeNote::Listed,
            })
            .collect();
        if let Some(saved) = settings.resolution.map(|saved| match settings.display {
            DisplayMode::Windowed => self.fit(saved),
            _ => saved,
        }) && !self.sizes.contains(&saved)
        {
            let note = match settings.display {
                DisplayMode::Exclusive => SizeNote::Unsupported,
                _ => SizeNote::Saved,
            };
            let at = entries.partition_point(|entry| entry.size < saved);
            entries.insert(at, SizeEntry { size: saved, note });
        }
        entries
    }
}

/// A size of the list of resolutions and what is to be said of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeEntry {
    pub size: (u32, u32),
    pub note: SizeNote,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeNote {
    /// A mode of the monitor.
    Listed,
    /// The player's saved size, which the monitor does not list: fine for a window.
    Saved,
    /// The saved size in exclusive fullscreen, which the monitor cannot do.
    Unsupported,
}

impl Default for Displays {
    /// What stands until a monitor reports its modes (and for a browser, which has none).
    fn default() -> Displays {
        Displays::new((1920, 1080), [(1280, 720), (1600, 900), (1920, 1080)])
    }
}

/// Whether the game keeps the cursor (confined to the window, or a page's
/// pointer locked): while a match shows, the window has focus and the
/// settings overlay is closed. Captures and benchmarks run unattended, often
/// over the owner's work, and never keep it.
pub fn keeps_cursor(
    window: &Window,
    screen: &crate::settings::Screen,
    args: &crate::Args,
    session: &crate::session::Session,
) -> bool {
    window.focused && !screen.open && !args.unattended() && session.0.view().is_some()
}

/// The window mode that shows a display mode.
///
/// A window does not know its monitor before it exists, so at creation
/// (`on_start`) the primary one is asked for.
pub fn window_mode(display: DisplayMode, video: VideoModeSelection, on_start: bool) -> WindowMode {
    let monitor = if on_start {
        MonitorSelection::Primary
    } else {
        MonitorSelection::Current
    };
    match display {
        DisplayMode::Windowed => WindowMode::Windowed,
        DisplayMode::Borderless => WindowMode::BorderlessFullscreen(monitor),
        DisplayMode::Exclusive if exclusive_unavailable() => {
            WindowMode::BorderlessFullscreen(monitor)
        }
        DisplayMode::Exclusive => WindowMode::Fullscreen(monitor, video),
    }
}

/// Whether exclusive fullscreen is not on offer, so the mode shows as
/// windowed fullscreen: a Wayland compositor decides the video mode itself
/// and winit ignores the request. (winit takes Wayland when the session has
/// it, unless `WINIT_UNIX_BACKEND` says X11.)
pub fn exclusive_unavailable() -> bool {
    let set = |name| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    cfg!(target_os = "linux")
        && set("WAYLAND_DISPLAY")
        && std::env::var("WINIT_UNIX_BACKEND").map_or(true, |b| b != "x11")
}

/// The display modes a player may pick: a browser cannot take the monitor's
/// video mode, so it has no exclusive one.
pub fn pickable_modes() -> usize {
    if cfg!(target_arch = "wasm32") { 2 } else { 3 }
}

/// What the display mode of the settings must become, given whether the
/// canvas is fullscreen. A browser grants fullscreen only on a gesture and
/// takes it back on Esc (with the page's Esc lock, `web/keys.js`, only on a
/// held Esc, in Chromium), so the page's state is the truth and the setting
/// follows it, except in the frame the setting itself changed.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Debug, PartialEq, Eq)]
pub enum Fullscreen {
    Enter,
    Leave,
    /// The setting is set to what the page shows.
    Follow(DisplayMode),
    Keep,
}

/// `changed`: the setting was just picked; `asked`: an `Enter` is still
/// awaiting the browser's answer.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn fullscreen_step(
    display: DisplayMode,
    fullscreen: bool,
    changed: bool,
    asked: bool,
) -> Fullscreen {
    let want = display != DisplayMode::Windowed;
    match (want, fullscreen) {
        (true, false) if changed => Fullscreen::Enter,
        (true, false) if asked => Fullscreen::Keep,
        (true, false) => Fullscreen::Follow(DisplayMode::Windowed),
        (false, true) if changed => Fullscreen::Leave,
        (false, true) => Fullscreen::Follow(DisplayMode::Borderless),
        _ => Fullscreen::Keep,
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::{cell::Cell, rc::Rc};

    use bevy::prelude::*;
    use wasm_bindgen::{JsCast, closure::Closure};

    use super::{Fullscreen, fullscreen_step};
    use crate::config::Settings;

    #[derive(Default)]
    pub struct State {
        shown: Option<crate::config::DisplayMode>,
        asked: bool,
        /// Set by the document's `fullscreenerror`.
        refused: Option<Rc<Cell<bool>>>,
    }

    /// Puts the canvas fullscreen when the settings ask for windowed
    /// fullscreen and out of it when they ask for a window. The request is
    /// made in the frame after the click that picked it, which the browser
    /// still counts as that click's gesture (transient user activation); if
    /// it refuses anyway, or the player leaves with Esc (a held one under the
    /// page's Esc lock), the setting goes back to windowed. The canvas follows the screen by itself (the window
    /// observes it), and so does the HUD scale (from the window's height).
    pub fn sync_fullscreen(mut settings: ResMut<Settings>, mut state: NonSendMut<State>) {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let refused = state
            .refused
            .get_or_insert_with(|| {
                let flag = Rc::new(Cell::new(false));
                let seen = flag.clone();
                let on_error = Closure::<dyn FnMut()>::new(move || seen.set(true));
                let _ = document.add_event_listener_with_callback(
                    "fullscreenerror",
                    on_error.as_ref().unchecked_ref(),
                );
                on_error.forget();
                flag
            })
            .clone();
        let fullscreen = document.fullscreen_element().is_some();
        if refused.replace(false) {
            state.asked = false;
            settings.display = crate::config::DisplayMode::Windowed;
            state.shown = Some(settings.display);
            return;
        }
        if fullscreen {
            state.asked = false;
        }
        let changed = state.shown.is_some_and(|shown| shown != settings.display);
        let display = settings.display;
        let step = fullscreen_step(display, fullscreen, changed, state.asked);
        match step {
            Fullscreen::Enter => {
                state.asked = true;
                let canvas = document.get_element_by_id("dbota");
                match canvas.map(|c| c.request_fullscreen()) {
                    Some(Ok(())) => {}
                    _ => {
                        state.asked = false;
                        settings.display = crate::config::DisplayMode::Windowed;
                    }
                }
            }
            Fullscreen::Leave => document.exit_fullscreen(),
            Fullscreen::Follow(mode) => settings.display = mode,
            Fullscreen::Keep => {}
        }
        state.shown = Some(settings.display);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use bevy::{
        prelude::*,
        window::{
            CursorGrabMode, CursorOptions, Monitor, OnMonitor, PrimaryMonitor, PrimaryWindow,
            VideoMode, VideoModeSelection, WindowMode, WindowResized,
        },
    };

    use crate::config::Settings;
    use crate::session::Session;

    /// What `apply_mode` remembers between frames.
    #[derive(Default)]
    pub struct Sizes {
        /// The size the last window had.
        windowed: Option<Vec2>,
        /// The size a window had (or was asked for), to give back once the compositor has taken
        /// it out of fullscreen, unless a resolution is chosen by then.
        restore: Option<Vec2>,
        /// The resolution a window was last given.
        resolution: Option<Option<(u32, u32)>>,
    }

    /// A size to give the window: the monitor's physical pixels for a resolution of the
    /// settings, the window's logical ones for the size it had.
    #[derive(Clone, Copy)]
    enum Restore {
        Logical(Vec2),
        Physical(UVec2),
    }

    impl Restore {
        fn of((width, height): (u32, u32)) -> Restore {
            Restore::Physical(UVec2::new(width, height))
        }

        fn give(self, window: &mut Window) {
            match self {
                Restore::Logical(size) => window.resolution.set(size.x, size.y),
                Restore::Physical(size) => {
                    window.resolution.set_physical_resolution(size.x, size.y)
                }
            }
        }
    }

    /// The monitor the window is on, else (before it is on one) the primary, else any.
    fn current_monitor<'a>(
        on: Option<&OnMonitor>,
        monitors: &'a Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
    ) -> Option<&'a Monitor> {
        on.and_then(|on| monitors.get(on.0).ok())
            .or_else(|| monitors.iter().max_by_key(|(_, _, primary)| *primary))
            .map(|(_, monitor, _)| monitor)
    }

    /// The size and modes of the monitor the window is on, for the Resolution setting.
    pub fn watch_monitor(
        monitors: Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
        window: Single<Option<&OnMonitor>, With<PrimaryWindow>>,
        mut displays: ResMut<super::Displays>,
    ) {
        let Some(monitor) = current_monitor(*window, &monitors) else {
            return;
        };
        let native = (monitor.physical_width, monitor.physical_height);
        let modes = monitor
            .video_modes
            .iter()
            .map(|mode| (mode.physical_size.x, mode.physical_size.y));
        let seen = super::Displays::new(native, modes);
        if *displays != seen {
            *displays = seen;
        }
    }

    /// The monitor's mode of `size` with the most colours and the highest refresh rate.
    fn video_mode(monitor: &Monitor, size: (u32, u32)) -> Option<VideoMode> {
        monitor
            .video_modes
            .iter()
            .filter(|mode| (mode.physical_size.x, mode.physical_size.y) == size)
            .max_by_key(|mode| (mode.bit_depth, mode.refresh_rate_millihertz))
            .copied()
    }

    /// Whether `window` already shows `mode` (the monitor it opened on stays the one it is on).
    fn shows(window: WindowMode, mode: WindowMode) -> bool {
        match (window, mode) {
            (WindowMode::Fullscreen(_, was), WindowMode::Fullscreen(_, now)) => was == now,
            _ => std::mem::discriminant(&window) == std::mem::discriminant(&mode),
        }
    }

    /// Switches the window to the settings' display mode and resolution while it runs.
    ///
    /// Back in a window it takes the resolution chosen, else the size the
    /// last window had, else the size asked for at the start: a window made
    /// fullscreen never had one for the compositor to restore. The size is
    /// asked for when the compositor has answered the change of mode, as it
    /// ignores one asked for with it. Windowed fullscreen keeps the desktop's
    /// mode, so the resolution waits for the other two.
    pub fn apply_mode(
        settings: Res<Settings>,
        args: Res<crate::Args>,
        displays: Res<super::Displays>,
        monitors: Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
        window: Single<(&mut Window, Option<&OnMonitor>), With<PrimaryWindow>>,
        mut resized: MessageReader<WindowResized>,
        mut sizes: Local<Sizes>,
    ) {
        let (mut window, on_monitor) = window.into_inner();
        let answered = resized.read().count() > 0;
        if answered
            && window.mode == WindowMode::Windowed
            && let Some(previous) = sizes.restore.take()
        {
            match settings.resolution {
                Some(_) => Restore::of(displays.in_use(&settings)),
                None => Restore::Logical(previous),
            }
            .give(&mut window);
            sizes.resolution = Some(settings.resolution);
        }
        let monitor = current_monitor(on_monitor, &monitors);
        let wanted = settings.resolution.map(|_| displays.in_use(&settings));
        let video = match (wanted, monitor) {
            (Some(size), Some(monitor)) => video_mode(monitor, size)
                .map_or(VideoModeSelection::Current, VideoModeSelection::Specific),
            _ => VideoModeSelection::Current,
        };
        let mode = super::window_mode(settings.display, video, false);
        if !shows(window.mode, mode) {
            if window.mode == WindowMode::Windowed {
                sizes.windowed = Some(window.resolution.size());
            }
            window.mode = mode;
            sizes.restore = (mode == WindowMode::Windowed).then(|| {
                sizes
                    .windowed
                    .unwrap_or(Vec2::new(args.window.0 as f32, args.window.1 as f32))
            });
            return;
        }
        // Already a window: a resolution newly chosen resizes it, and choosing the monitor's own
        // (reset) gives the window the monitor's size. A window that opened with no choice keeps
        // the size it opened with.
        let wanted = Some(settings.resolution);
        if window.mode == WindowMode::Windowed
            && sizes.restore.is_none()
            && sizes.resolution != wanted
        {
            let first = sizes.resolution.is_none();
            sizes.resolution = wanted;
            if settings.resolution.is_some() || !first {
                Restore::of(displays.in_use(&settings)).give(&mut window);
            }
        }
    }

    /// What the confinement did with the last request.
    #[derive(Default)]
    pub struct Grab {
        /// Whether a confinement was asked for and not yet seen to hold.
        asked: bool,
        /// Whether the platform refused it; no more asking until the cursor
        /// is let go.
        refused: bool,
    }

    /// Confines the cursor to the window while the game keeps it
    /// ([`super::keeps_cursor`]); lets it go otherwise.
    ///
    /// Where the platform cannot confine (a Wayland compositor without the
    /// pointer-constraints protocol), the cursor stays free, a warning says
    /// so once, and the edge pan keeps panning the way a cursor that left
    /// the window went. The cursor is never locked instead: a locked cursor
    /// has no position to pan by.
    pub fn confine(
        window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
        session: NonSend<Session>,
        screen: Res<crate::settings::Screen>,
        args: Res<crate::Args>,
        mut grab: Local<Grab>,
    ) {
        let (window, mut options) = window.into_inner();
        let want = super::keeps_cursor(window, &screen, &args, &session);
        if !want {
            if options.grab_mode != CursorGrabMode::None {
                options.grab_mode = CursorGrabMode::None;
                info!("cursor released");
            }
            *grab = Grab::default();
        } else if options.grab_mode == CursorGrabMode::Confined {
            if grab.asked {
                info!("cursor confined to the window");
            }
            grab.asked = false;
        } else if grab.asked {
            // Bevy puts the old mode back when the platform says no.
            warn!(
                "the cursor cannot be confined to the window here; edge pan follows the cursor past the window's edge"
            );
            *grab = Grab {
                asked: false,
                refused: true,
            };
        } else if !grab.refused {
            options.grab_mode = CursorGrabMode::Confined;
            grab.asked = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use DisplayMode::{Borderless, Exclusive, Windowed};

    /// The browser's state is the truth: Esc turns the setting back, a
    /// refused request does too, and a setting picked this frame is asked
    /// for, not undone.
    #[test]
    fn fullscreen_setting_follows_the_page() {
        assert_eq!(
            fullscreen_step(Borderless, false, true, false),
            Fullscreen::Enter
        );
        assert_eq!(
            fullscreen_step(Borderless, false, false, true),
            Fullscreen::Keep
        );
        assert_eq!(
            fullscreen_step(Borderless, false, false, false),
            Fullscreen::Follow(Windowed)
        );
        assert_eq!(
            fullscreen_step(Windowed, true, true, false),
            Fullscreen::Leave
        );
        assert_eq!(
            fullscreen_step(Windowed, true, false, false),
            Fullscreen::Follow(Borderless)
        );
        assert_eq!(
            fullscreen_step(Borderless, true, false, false),
            Fullscreen::Keep
        );
    }

    /// A monitor's modes are listed once each, none over its own size, its own always in.
    #[test]
    fn displays_list_each_size_once_up_to_the_monitors_own() {
        let displays = Displays::new(
            (2560, 1440),
            [
                (1920, 1080),
                (1920, 1080),
                (3840, 2160),
                (1280, 720),
                (800, 600),
            ],
        );
        assert_eq!(
            displays.sizes(),
            [(800, 600), (1280, 720), (1920, 1080), (2560, 1440)]
        );
    }

    /// The size in use: the monitor's own in windowed fullscreen, the saved size as it is in a
    /// window, the nearest listed size at or below it in exclusive fullscreen (native if none),
    /// and the saved size kept for the other modes.
    #[test]
    fn the_size_in_use_follows_the_display_mode_and_a_saved_size_the_monitor_lacks() {
        use crate::config::Settings;
        let displays = Displays::new((2560, 1440), [(1280, 720), (1920, 1080), (2560, 1440)]);
        let with = |display, resolution| Settings {
            display,
            resolution,
            ..Settings::default()
        };
        assert_eq!(
            displays.in_use(&with(Borderless, Some((1600, 900)))),
            (2560, 1440)
        );
        assert_eq!(
            displays.in_use(&with(Windowed, Some((1600, 900)))),
            (1600, 900)
        );
        assert_eq!(
            displays.in_use(&with(Exclusive, Some((1600, 900)))),
            (1280, 720)
        );
        assert_eq!(
            displays.in_use(&with(Exclusive, Some((1920, 1080)))),
            (1920, 1080)
        );
        assert_eq!(
            displays.in_use(&with(Exclusive, Some((640, 480)))),
            (2560, 1440)
        );
        assert_eq!(displays.in_use(&with(Windowed, None)), (2560, 1440));
    }

    /// The list holds a saved size the monitor lacks in its sorted place: usable in a window,
    /// not in exclusive fullscreen; a listed saved size is not doubled.
    #[test]
    fn the_list_of_sizes_holds_a_saved_size_the_monitor_lacks_in_its_sorted_place() {
        use crate::config::Settings;
        let displays = Displays::new((2560, 1440), [(1280, 720), (1920, 1080)]);
        let with = |display, resolution| Settings {
            display,
            resolution,
            ..Settings::default()
        };
        let sizes = |s: &Settings| -> Vec<((u32, u32), SizeNote)> {
            displays
                .entries(s)
                .iter()
                .map(|e| (e.size, e.note))
                .collect()
        };
        assert_eq!(
            sizes(&with(Windowed, Some((1600, 900)))),
            [
                ((1280, 720), SizeNote::Listed),
                ((1600, 900), SizeNote::Saved),
                ((1920, 1080), SizeNote::Listed),
                ((2560, 1440), SizeNote::Listed)
            ]
        );
        assert_eq!(
            sizes(&with(Exclusive, Some((1600, 900))))[1],
            ((1600, 900), SizeNote::Unsupported)
        );
        assert_eq!(sizes(&with(Windowed, Some((1920, 1080)))).len(), 3);
        assert_eq!(sizes(&with(Windowed, None)).len(), 3);
    }
}
