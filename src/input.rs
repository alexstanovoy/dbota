//! Keys and mouse into intents for the client core, Dota's way: right click
//! to move and attack, and what the bindings ([`crate::keys`]) fire: attack
//! and move for the next click, stop, hold, the ability and item keys (Ctrl
//! with an ability key, or the learn mode before it, learns it), the
//! control groups, Shift held to queue the orders given; clicks and drags
//! on the HUD's boxes.

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
use crate::config::Settings;
use crate::hud::{AbilityBox, Blocker, ItemBox, SeatBox};
use crate::keys::{Action, Fired};
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
    /// The chat line.
    Chat,
    /// The shop's search box.
    Search,
}

/// Panels and modes the bindings toggle.
#[derive(Resource, Default)]
pub struct Panels {
    /// Whether the shop is open.
    pub shop: bool,
    /// What is typed into the shop's search box.
    pub search: String,
    /// Whether the camera stays on one's own hero.
    pub camera_lock: bool,
    /// Whether the talent tree is pinned open.
    pub talents: bool,
    /// Whether the next left click moves, whatever is there (Dota's Move).
    pub move_armed: bool,
    /// Whether the next ability key learns a level (Dota's learn mode).
    pub learn_mode: bool,
    /// Whether the scoreboard is pinned open by the top-left menu's button.
    pub scoreboard: bool,
}

/// The units of Dota's control groups, one each: dbota selects one unit at
/// a time.
#[derive(Resource, Default)]
pub struct ControlGroups(pub [Option<bota_proto::EntityId>; 10]);

pub fn plugin(app: &mut App) {
    app.init_resource::<OverHud>()
        .init_resource::<Typing>()
        .init_resource::<Panels>()
        .init_resource::<ControlGroups>()
        .add_systems(PreUpdate, mute.before(crate::keys::fire))
        .add_systems(
            Update,
            (
                over_hud,
                typing,
                crate::chat::typing,
                keys,
                crate::pick::update,
                crate::pings::send,
                mouse,
                boxes,
            )
                .chain(),
        );
}

/// Keeps the bindings quiet while a line or a box has the keyboard, or the
/// settings screen or the main menu does.
fn mute(
    typing: Res<Typing>,
    screen: Res<crate::settings::Screen>,
    menu: Option<NonSend<crate::main_menu::MenuCore>>,
    mut muted: ResMut<crate::keys::Muted>,
) {
    let menu_covers = menu.is_some_and(|m| !m.shows_game());
    let now = *typing != Typing::No || screen.open || menu_covers;
    if muted.0 != now {
        muted.0 = now;
    }
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

/// Opens the console line on its key and types into it or the shop's search
/// box while one has the keyboard.
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
            // The chat line is crate::chat's.
            Typing::No | Typing::Chat => {}
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

/// What the bindings fired, done.
#[allow(clippy::too_many_arguments)]
fn keys(
    mut session: NonSendMut<Session>,
    fired: Res<Fired>,
    keys: Res<ButtonInput<KeyCode>>,
    mut typing: ResMut<Typing>,
    mut panels: ResMut<Panels>,
    mut screen: ResMut<crate::settings::Screen>,
    mut groups: ResMut<ControlGroups>,
    mut exit: MessageWriter<AppExit>,
) {
    let core = &mut session.0;
    // Orders given while Shift is held wait their turn behind what the unit
    // does, as Dota's Shift-queue: by key, by click, and by an aimed click.
    core.queueing = *typing == Typing::No
        && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight));
    if *typing != Typing::No || typing.is_changed() {
        return;
    }
    if fired.pressed(Action::Escape) {
        escape(core, &mut panels, &mut screen, &mut exit);
    }
    if fired.pressed(Action::Console) && matches!(core.source, Source::Live(_)) {
        *typing = Typing::Console;
        core.console = Some(String::new());
        return;
    }
    if fired.pressed(Action::Shop) && core.my_slot.is_some() {
        panels.shop = !panels.shop;
    }
    if matches!(core.source, Source::Replay(_)) {
        watch(core, &fired);
        return;
    }
    if fired.pressed(Action::NextEyes) && core.my_slot.is_none() {
        core.act(Intent::NextEyes);
    }
    // Heroes are picked and readied with the mouse, on the selection screen.
    if core.phase == bota_client_core::Phase::Lobby {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    seat(core, &fired, &mut panels, &mut groups, ctrl);
    if core.my_hero().is_none() || !core.controls_selection() || core.over.is_some() {
        return;
    }
    command(core, &fired, &mut panels, ctrl);
}

/// A replay's own bindings.
fn watch(core: &mut bota_client_core::App, fired: &Fired) {
    for (action, intent) in [
        (Action::Pause, Intent::TogglePause),
        (Action::Faster, Intent::Faster),
        (Action::Slower, Intent::Slower),
        (Action::Step, Intent::Step),
        (Action::NextEyes, Intent::NextEyes),
    ] {
        if fired.pressed(action) {
            core.act(intent);
        }
    }
}

/// What a seated player's bindings do whatever is selected: picking units
/// and groups, the courier, the Glyph, the shop's buys, the camera lock and
/// the talent tree.
fn seat(
    core: &mut bota_client_core::App,
    fired: &Fired,
    panels: &mut Panels,
    groups: &mut ControlGroups,
    ctrl: bool,
) {
    for (action, intent) in [
        (Action::SelectHero, Intent::SelectHero),
        (Action::SelectCourier, Intent::SelectCourier),
        (Action::CourierDeliver, Intent::CourierDeliver),
        (Action::QuickBuy, Intent::QuickBuy),
        (Action::StickyBuy, Intent::Buy(crate::shop::STICKY.0)),
    ] {
        if fired.pressed(action) {
            core.act(intent);
        }
    }
    for (action, which) in [
        (Action::CourierBurst, crate::courier::CourierCast::Burst),
        (Action::CourierShield, crate::courier::CourierCast::Shield),
    ] {
        if fired.pressed(action) {
            crate::courier::cast(core, which);
        }
    }
    if fired.pressed(Action::Glyph) && core.my_slot.is_some() {
        core.activate_glyph();
    }
    if fired.pressed(Action::CameraLock) {
        panels.camera_lock = !panels.camera_lock;
    }
    if fired.pressed(Action::LearnStats) {
        panels.talents = !panels.talents;
    }
    for group in 0..10u8 {
        if !fired.pressed(Action::ControlGroup(group)) {
            continue;
        }
        let kept = &mut groups.0[usize::from(group)];
        if ctrl {
            *kept = core.selected.filter(|id| core.drives(*id));
        } else if let Some(unit) = *kept {
            core.act(Intent::SelectUnit(Some(unit)));
        }
    }
}

/// The orders to the commanded unit: attack, move, stop, hold, its
/// abilities (learned instead in learn mode, or with Ctrl) and its items.
fn command(core: &mut bota_client_core::App, fired: &Fired, panels: &mut Panels, ctrl: bool) {
    if fired.pressed(Action::Attack) {
        core.attack_move_armed = true;
        panels.move_armed = false;
        core.aiming = None;
    }
    if fired.pressed(Action::Move) {
        panels.move_armed = true;
        core.attack_move_armed = false;
        core.aiming = None;
    }
    if fired.pressed(Action::LearnAbilities) {
        panels.learn_mode = !panels.learn_mode;
    }
    if fired.pressed(Action::Stop) {
        core.attack_move_armed = false;
        panels.move_armed = false;
        core.aiming = None;
        core.act(Intent::Stop);
    }
    if fired.pressed(Action::Hold) {
        core.attack_move_armed = false;
        panels.move_armed = false;
        core.act(Intent::Hold);
    }
    let count = core.slot_unit().map_or(0, |u| u.abilities.len());
    for slot in 0..count.min(6) as u8 {
        if fired.pressed(Settings::ability_action(slot, count)) {
            let learn = ctrl || std::mem::take(&mut panels.learn_mode);
            press(core, Slot::Ability(slot), learn);
        }
    }
    for slot in 0..bota_client_core::INVENTORY_SLOTS {
        if fired.pressed(Action::Item(slot)) {
            press(core, Slot::Item(slot), false);
        }
    }
    if fired.pressed(Action::TownPortal) {
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

/// Escape lets go of what is half done and closes what is open, as Dota's;
/// with nothing of either it opens the settings, and it leaves a match that
/// is over.
fn escape(
    core: &mut bota_client_core::App,
    panels: &mut Panels,
    screen: &mut crate::settings::Screen,
    exit: &mut MessageWriter<AppExit>,
) {
    let half_done = core.attack_move_armed
        || core.aiming.is_some()
        || core.held_item.is_some()
        || panels.move_armed
        || panels.learn_mode;
    if half_done {
        core.attack_move_armed = false;
        core.aiming = None;
        core.held_item = None;
        panels.move_armed = false;
        panels.learn_mode = false;
    } else if panels.shop {
        panels.shop = false;
    } else if panels.talents {
        panels.talents = false;
    } else if panels.scoreboard {
        panels.scoreboard = false;
    } else if core.over.is_some() || core.phase == bota_client_core::Phase::Lobby {
        exit.write(AppExit::Success);
    } else {
        screen.open = true;
    }
}

/// Clicks on the world.
fn mouse(
    mut session: NonSendMut<Session>,
    buttons: Res<ButtonInput<MouseButton>>,
    cursor: Res<CursorGround>,
    hover: Res<crate::pick::Hover>,
    over: Res<OverHud>,
    mut panels: ResMut<Panels>,
    screen: Res<crate::settings::Screen>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    if over.0 || screen.open || window.cursor_position().is_none() {
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
        if commands && live && panels.move_armed {
            panels.move_armed = false;
            core.act(Intent::MoveTo(ground));
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
        panels.move_armed = false;
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
