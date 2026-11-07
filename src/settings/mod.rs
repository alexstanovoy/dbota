//! The settings screen, as Dota's dashboard has it: tabs of hotkeys,
//! options, video and audio, with toggles, sliders, lists to pick from and
//! key fields that take the next key pressed. It opens from the main menu's
//! SETTINGS (also on the dashboard over a game, which the HUD's dashboard
//! button and Escape open); Escape closes it; whatever it changes is saved when it closes.

mod draw;
mod look;
mod model;
mod screen;

#[cfg(test)]
pub use model::Choice;
#[cfg(test)]
pub use screen::{InView, Scroll};
pub use screen::{Screen, escape, plugin};
