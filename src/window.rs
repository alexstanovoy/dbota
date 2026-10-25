//! The window as Dota keeps it: windowed fullscreen by default (the display
//! mode of the settings, applied live), and the cursor confined to the
//! window while a match is on screen and the window has focus, so the edge
//! pan works on every edge and the mouse never leaves through the top. The
//! cursor is let go in the menus, with the settings overlay open and when
//! the window loses focus (Alt-Tab).
//!
//! A browser's canvas is the page's, never borderless by itself, and the
//! page owns its pointer. There "windowed fullscreen" is the Fullscreen API
//! on the canvas (`web`), and there is no exclusive mode.

use bevy::{
    prelude::*,
    window::{MonitorSelection, VideoModeSelection, WindowMode},
};

use crate::config::DisplayMode;

pub fn plugin(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Update, (native::apply_mode, native::confine));
    #[cfg(target_arch = "wasm32")]
    {
        app.init_non_send::<web::State>();
        app.add_systems(Update, web::sync_fullscreen);
    }
}

/// The window mode that shows a display mode.
///
/// A window does not know its monitor before it exists, so at creation
/// (`on_start`) the primary one is asked for.
pub fn window_mode(display: DisplayMode, on_start: bool) -> WindowMode {
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
        DisplayMode::Exclusive => WindowMode::Fullscreen(monitor, VideoModeSelection::Current),
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
/// takes it back on Esc, so the page's state is the truth and the setting
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
    /// it refuses anyway, or the player leaves with Esc, the setting goes
    /// back to windowed. The canvas follows the screen by itself (the window
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
        window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowMode, WindowResized},
    };

    use crate::config::Settings;
    use crate::session::Session;

    /// What `apply_mode` remembers between frames.
    #[derive(Default)]
    pub struct Sizes {
        /// The size the last window had.
        windowed: Option<Vec2>,
        /// The size to ask for once the compositor has taken the window out
        /// of fullscreen.
        restore: Option<Vec2>,
    }

    /// Switches the window to the settings' display mode while it runs.
    ///
    /// Back in a window it takes the size the last window had, else the
    /// size asked for at the start: a window made fullscreen never had one
    /// for the compositor to restore. The size is asked for when the
    /// compositor has answered the change of mode, as it ignores one asked
    /// for with it.
    pub fn apply_mode(
        settings: Res<Settings>,
        args: Res<crate::Args>,
        mut window: Single<&mut Window, With<PrimaryWindow>>,
        mut resized: MessageReader<WindowResized>,
        mut sizes: Local<Sizes>,
    ) {
        let answered = resized.read().count() > 0;
        if answered
            && window.mode == WindowMode::Windowed
            && let Some(size) = sizes.restore.take()
        {
            window.resolution.set(size.x, size.y);
        }
        let mode = super::window_mode(settings.display, false);
        // The monitor the window opened on stays the one it is on.
        if std::mem::discriminant(&window.mode) == std::mem::discriminant(&mode) {
            return;
        }
        if window.mode == WindowMode::Windowed {
            sizes.windowed = Some(window.resolution.size());
        }
        window.mode = mode;
        sizes.restore = (mode == WindowMode::Windowed).then(|| {
            sizes
                .windowed
                .unwrap_or(Vec2::new(args.window.0 as f32, args.window.1 as f32))
        });
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

    /// Confines the cursor to the window while a match shows and the window
    /// has focus; lets it go otherwise.
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
        // Captures and benchmarks run unattended, often over the owner's work.
        let want =
            window.focused && !screen.open && !args.unattended() && session.0.view().is_some();
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
    use DisplayMode::{Borderless, Windowed};

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
}
