//! Keys and mouse into intents for the client core, Dota's way: right click
//! to move and attack, A to attack-move, S to stop, H to hold, the ability
//! and item keys, Ctrl with an ability key to learn it, Shift held to queue
//! the orders given; clicks and drags on the HUD's boxes.

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    window::PrimaryWindow,
};
use bota_client_core::{BAG_SLOTS, Intent, Slot, Source, TOWN_PORTAL_SLOT};

use crate::camera::CursorGround;
use crate::config::{Action, Settings};
use crate::hud::{AbilityBox, Blocker, ItemBox, SeatBox};
use crate::session::Session;

/// Whether the cursor is over the HUD this frame, so clicks stay out of the
/// world.
#[derive(Resource, Default)]
pub struct OverHud(pub bool);

/// What the keyboard is typing into, when anything.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum Typing {
    /// Nothing: keys are hotkeys.
    #[default]
    No,
    /// The console line.
    Console,
    /// The shop's search box.
    Search,
}

/// Panels toggled by keys.
#[derive(Resource, Default)]
pub struct Panels {
    /// Whether the shop is open.
    pub shop: bool,
    /// What is typed into the shop's search box.
    pub search: String,
    /// Whether the settings overlay is open.
    pub settings: bool,
    /// Whether the camera stays on one's own hero.
    pub camera_lock: bool,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<OverHud>()
        .init_resource::<Typing>()
        .init_resource::<Panels>()
        .add_systems(
            Update,
            (over_hud, typing, keys, crate::pick::update, mouse, boxes).chain(),
        );
}

/// Notes whether the cursor is over any part of the HUD that swallows clicks.
fn over_hud(
    blockers: Query<&Interaction, Or<(With<Blocker>, With<Button>)>>,
    mut over: ResMut<OverHud>,
) {
    let now = blockers.iter().any(|i| *i != Interaction::None);
    if over.0 != now {
        over.0 = now;
    }
}

/// Types into the console line or the shop's search box while one has the
/// keyboard.
fn typing(
    mut session: NonSendMut<Session>,
    mut events: MessageReader<KeyboardInput>,
    mut typing: ResMut<Typing>,
    mut panels: ResMut<Panels>,
    texts: Res<crate::dota_text::Texts>,
    manifest: Res<crate::manifest::Manifest>,
    catalog: Res<crate::shop::Catalog>,
) {
    let core = &mut session.0;
    let look = crate::tip_content::Lookup {
        texts: &texts,
        manifest: &manifest,
    };
    for event in events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        match *typing {
            Typing::No => {
                let live_player = matches!(core.source, Source::Live(_)) && core.my_slot.is_some();
                if event.key_code == KeyCode::Enter && live_player && !panels.shop {
                    *typing = Typing::Console;
                    core.console = Some(String::new());
                }
            }
            Typing::Console => {
                let line = core.console.get_or_insert_with(String::new);
                match &event.logical_key {
                    Key::Enter => {
                        let line = core.console.take().unwrap_or_default();
                        *typing = Typing::No;
                        core.act(Intent::Console(line));
                    }
                    Key::Escape => {
                        core.console = None;
                        *typing = Typing::No;
                    }
                    Key::Backspace => {
                        line.pop();
                    }
                    Key::Character(s) if line.len() < 64 => line.push_str(s),
                    Key::Space => line.push(' '),
                    _ => {}
                }
            }
            Typing::Search => match &event.logical_key {
                Key::Enter => {
                    if let Some(&id) = catalog.search(&look, &panels.search).first() {
                        core.act(Intent::Buy(id));
                    }
                    *typing = Typing::No;
                }
                Key::Escape => {
                    if panels.search.is_empty() {
                        *typing = Typing::No;
                    } else {
                        panels.search.clear();
                    }
                }
                Key::Backspace => {
                    panels.search.pop();
                }
                Key::Character(s) if panels.search.len() < 24 => panels.search.push_str(s),
                Key::Space => panels.search.push(' '),
                _ => {}
            },
        }
    }
}

/// The hotkeys.
fn keys(
    mut session: NonSendMut<Session>,
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<Settings>,
    typing: Res<Typing>,
    mut panels: ResMut<Panels>,
    mut exit: MessageWriter<AppExit>,
) {
    let core = &mut session.0;
    // Orders given while Shift is held wait their turn behind what the unit
    // does, as Dota's Shift-queue: by key, by click, and by an aimed click.
    core.queueing = *typing == Typing::No
        && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight));
    if *typing != Typing::No {
        return;
    }
    let pressed = |action: Action| keys.just_pressed(settings.key(action));
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if keys.just_pressed(KeyCode::Escape) {
        escape(core, &mut panels, &mut exit);
    }
    if pressed(Action::Settings) {
        panels.settings = !panels.settings;
    }
    if pressed(Action::Shop) && core.my_slot.is_some() {
        panels.shop = !panels.shop;
    }
    if keys.just_pressed(KeyCode::Tab) {
        core.act(Intent::NextEyes);
    }
    if matches!(core.source, Source::Replay(_)) {
        for (key, intent) in [
            (KeyCode::Space, Intent::TogglePause),
            (KeyCode::Equal, Intent::Faster),
            (KeyCode::NumpadAdd, Intent::Faster),
            (KeyCode::Minus, Intent::Slower),
            (KeyCode::NumpadSubtract, Intent::Slower),
            (KeyCode::Period, Intent::Step),
        ] {
            if keys.just_pressed(key) {
                core.act(intent);
            }
        }
        return;
    }
    // Heroes are picked and readied with the mouse, on the selection screen.
    if core.phase == bota_client_core::Phase::Lobby {
        return;
    }
    if pressed(Action::SelectHero) {
        core.act(Intent::SelectHero);
    }
    if pressed(Action::SelectCourier) {
        core.act(Intent::SelectCourier);
    }
    if pressed(Action::CourierDeliver) {
        core.act(Intent::CourierDeliver);
    }
    if pressed(Action::Glyph) && core.my_slot.is_some() {
        core.activate_glyph();
    }
    if pressed(Action::QuickBuy) {
        core.act(Intent::QuickBuy);
    }
    if pressed(Action::CameraLock) {
        panels.camera_lock = !panels.camera_lock;
    }
    if core.my_hero().is_none() || !core.controls_selection() || core.over.is_some() {
        return;
    }
    if pressed(Action::AttackMove) {
        core.attack_move_armed = true;
        core.aiming = None;
    }
    if pressed(Action::Stop) {
        core.attack_move_armed = false;
        core.aiming = None;
        core.act(Intent::Stop);
    }
    if pressed(Action::Hold) {
        core.attack_move_armed = false;
        core.act(Intent::Hold);
    }
    let count = core.slot_unit().map_or(0, |u| u.abilities.len());
    for slot in 0..count.min(6) as u8 {
        if keys.just_pressed(settings.ability_key(slot, count)) {
            press(core, Slot::Ability(slot), ctrl);
        }
    }
    for slot in 0..bota_client_core::INVENTORY_SLOTS {
        if pressed(Action::Item(slot)) {
            press(core, Slot::Item(slot), false);
        }
    }
    if pressed(Action::TownPortal) {
        press(core, Slot::Item(TOWN_PORTAL_SLOT), false);
    }
}

/// Presses a slot; one taken up for aiming disarms a pending attack-move.
pub fn press(core: &mut bota_client_core::App, slot: Slot, ctrl: bool) {
    core.act(Intent::Press { slot, ctrl });
    if core.aiming.is_some() {
        core.attack_move_armed = false;
    }
}

/// Escape lets go of what is half done, closes what is open, and leaves when
/// the match is over.
fn escape(
    core: &mut bota_client_core::App,
    panels: &mut Panels,
    exit: &mut MessageWriter<AppExit>,
) {
    if core.attack_move_armed || core.aiming.is_some() || core.held_item.is_some() {
        core.attack_move_armed = false;
        core.aiming = None;
        core.held_item = None;
    } else if panels.settings {
        panels.settings = false;
    } else if panels.shop {
        panels.shop = false;
    } else if core.over.is_some() || core.phase == bota_client_core::Phase::Lobby {
        exit.write(AppExit::Success);
    }
}

/// Clicks on the world.
fn mouse(
    mut session: NonSendMut<Session>,
    buttons: Res<ButtonInput<MouseButton>>,
    cursor: Res<CursorGround>,
    hover: Res<crate::pick::Hover>,
    over: Res<OverHud>,
    panels: Res<Panels>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    if over.0 || panels.settings || window.cursor_position().is_none() {
        return;
    }
    let core = &mut session.0;
    let Some(ground) = cursor.0.map(|p| (p.x, -p.z)) else {
        return;
    };
    // The picker alone says which unit a click is on; with none, the click is
    // for the ground at the cursor.
    let unit_of = |hit: crate::pick::Hit| hit.map(|(id, _)| id);
    let commands = core.my_hero().is_some() && core.controls_selection() && core.over.is_none();
    let live = matches!(core.source, Source::Live(_));
    if buttons.just_pressed(MouseButton::Left) {
        if core.held_item.is_some() {
            return;
        }
        if commands && live && core.aiming.is_some() {
            core.act(Intent::Aimed {
                at: ground,
                unit: unit_of(hover.aim),
            });
            return;
        }
        if commands && live && core.attack_move_armed {
            core.attack_move_armed = false;
            core.act(Intent::AttackAt {
                at: ground,
                unit: unit_of(hover.attack),
            });
            return;
        }
        core.act(Intent::SelectUnit(unit_of(hover.select)));
    }
    if buttons.just_pressed(MouseButton::Right) && commands && live {
        core.attack_move_armed = false;
        core.aiming = None;
        core.act(Intent::Smart {
            at: ground,
            unit: unit_of(hover.attack),
        });
    }
    if buttons.just_released(MouseButton::Left)
        && let Some(slot) = core.held_item.take()
        && slot < BAG_SLOTS
    {
        core.act(Intent::PutAt {
            slot,
            at: ground,
            unit: unit_of(hover.select),
        });
    }
}

/// Clicks and drags on the HUD's boxes: an ability box presses its slot, an
/// item box is picked up and dropped on another box, a seat, or the world.
fn boxes(
    mut session: NonSendMut<Session>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    abilities: Query<(&Interaction, &AbilityBox), Changed<Interaction>>,
    items: Query<(&Interaction, &ItemBox)>,
    seats: Query<(&Interaction, &SeatBox)>,
    over: Res<OverHud>,
) {
    let over_hud = over.0;
    let core = &mut session.0;
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let controls = core.controls_selection() && matches!(core.source, Source::Live(_));
    for (interaction, AbilityBox(slot)) in &abilities {
        if *interaction == Interaction::Pressed && controls {
            press(core, Slot::Ability(*slot), ctrl);
        }
    }
    let hovered_item = items
        .iter()
        .find(|(i, _)| **i != Interaction::None)
        .map(|(_, b)| b.0);
    let hovered_seat = seats
        .iter()
        .find(|(i, _)| **i != Interaction::None)
        .map(|(_, b)| b.0);
    if buttons.just_pressed(MouseButton::Left) {
        if let Some(slot) = hovered_item
            && controls
            && core.item_in(slot).is_some()
        {
            core.held_item = Some(slot);
        } else if let Some(seat) = hovered_seat {
            core.act(Intent::SelectSeat(bota_proto::SlotId(seat)));
        }
    }
    if buttons.just_released(MouseButton::Left)
        && let Some(from) = core.held_item
    {
        if let Some(to) = hovered_item {
            core.held_item = None;
            if to == from {
                press(core, Slot::Item(from), false);
            } else if from != TOWN_PORTAL_SLOT {
                core.act(Intent::Swap { from, to });
            }
        } else if let Some(seat) = hovered_seat
            && from != TOWN_PORTAL_SLOT
        {
            core.held_item = None;
            core.act(Intent::HandTo {
                slot: from,
                seat: bota_proto::SlotId(seat),
            });
        } else if over_hud {
            core.held_item = None;
        }
    }
}
