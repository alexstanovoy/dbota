//! The whole client end to end, headless: every plugin `main` adds, over Bevy's with no window
//! on screen and no GPU, playing a real `bota_server`'s match through the same connection the
//! client opens; the tests read what the client would draw (entities, their components, the
//! lines laid out for the GPU, colours, positions, menu entries), not pixels.

mod ambient;
mod anim;
mod bars;
mod buffs;
mod camera;
mod captions;
mod catalogue;
mod controls;
mod courier;
mod courier_spinner;
mod cursors;
mod death;
mod event_visibility;
mod fog;
mod fog_pov;
mod fx;
mod ground_items;
pub(crate) mod harness;
mod hero_fx;
mod icons;
mod item_drop_range;
mod item_states;
mod items;
mod lotus;
mod menu;
mod minimap;
mod pregame;
mod purchase_destination;
mod refusals;
mod rings;
mod runes;
mod sentry_ward;
mod shopkeepers;
mod slot_looks;
mod sound_limits;
mod soundscape;
mod start;
mod strategy;
mod tower_cadence;
mod tower_fall;
mod trees;
mod turning;
mod watcher_capture;
mod watcher_channel;
mod watcher_path;
mod watchers;
mod window;
mod wisdom_contest;
