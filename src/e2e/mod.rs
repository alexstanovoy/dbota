//! The whole client end to end, headless: every plugin `main` adds, over Bevy's with no window
//! on screen and no GPU, playing a real `bota_server`'s match through the same connection the
//! client opens; the tests read what the client would draw (entities, their components, the
//! lines laid out for the GPU, colours, positions, menu entries), not pixels.

mod bars;
mod buffs;
mod camera;
mod captions;
mod catalogue;
mod controls;
mod cursors;
mod death;
pub(crate) mod harness;
mod icons;
mod items;
mod menu;
mod minimap;
mod refusals;
mod rings;
mod start;
mod strategy;
mod trees;
mod window;
