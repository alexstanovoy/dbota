//! The window as Dota keeps it: windowed fullscreen by default (the display
//! mode of the settings, applied live), and the cursor confined to the
//! window while a match is on screen and the window has focus, so the edge
//! pan works on every edge and the mouse never leaves through the top. The
//! cursor is let go in the menus, with the settings overlay open and when
//! the window loses focus (Alt-Tab).
//!
//! A browser's canvas is the page's, never borderless by itself, and the
//! page owns its pointer: none of this runs there.

use bevy::{
    prelude::*,
    window::{MonitorSelection, VideoModeSelection, WindowMode},
};

use crate::config::DisplayMode;

pub fn plugin(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Update, (native::apply_mode, native::confine));
    #[cfg(target_arch = "wasm32")]
    let _ = app;
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

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use bevy::{
        prelude::*,
        window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowMode, WindowResized},
    };

    use crate::config::Settings;
    use crate::input::Panels;
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
        panels: Res<Panels>,
        args: Res<crate::Args>,
        mut grab: Local<Grab>,
    ) {
        let (window, mut options) = window.into_inner();
        // Captures and benchmarks run unattended, often over the owner's work.
        let unattended =
            !args.screenshot_at.is_empty() || args.screenshot.is_some() || args.bench.is_some();
        let want = window.focused && !panels.settings && !unattended && session.0.view().is_some();
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
