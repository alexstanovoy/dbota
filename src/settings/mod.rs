//! The settings screen, as Dota's dashboard has it: tabs of hotkeys,
//! options, video and audio, with toggles, sliders, lists to pick from and
//! key fields that take the next key pressed. It opens from the main menu's
//! SETTINGS (also on the dashboard over a game) and the HUD's gear; Escape
//! closes it; whatever it changes is saved when it closes.

mod draw;
mod model;
mod screen;

pub use screen::{Screen, escape, plugin};
