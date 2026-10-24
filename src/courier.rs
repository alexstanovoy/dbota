//! The courier's round buttons in the shop corner's gold and courier block,
//! as Dota's `dota_hud_courier` lays them out: select the courier (with how
//! many items it carries), its speed burst and its shield, each with its key
//! over its top left, and the larger button that delivers what waits in the
//! stash. A button that cannot be used now fades, and counts down its wait.

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{AbilityId, AbilitySlot, Order, Target};

use crate::config::Settings;
use crate::hud::{Blocker, Docks};
use crate::keys::{Action, KeyLabel};
use crate::manifest::Manifest;
use crate::session::{MatchKnown, Session};
use crate::ui::{self, DOTA, Fonts, dota};

/// A button (`.CourierButton`), the delivering one (`#DeliverItemsButton`)
/// and its ring, and the room either side of a button, in Dota's pixels.
const BUTTON: f32 = 28.0;
const DELIVER: f32 = 48.0;
const DELIVER_RING: f32 = 40.0;
const BUTTON_SPACE: f32 = 3.0;
/// The room between a button's ring and its icon.
const ICON_PADDING: f32 = 4.0;
/// The key's box (`.Hotkey`) and the badge of carried items
/// (`#ItemsOnCourierBackground`).
const KEY_BOX: f32 = 16.0;
const LOAD_BADGE: f32 = 22.0;
/// The ring's blue, top to bottom, and the glow inside it.
const RING_TOP: Color = Color::srgb(0.776, 0.855, 1.0);
const RING_FOOT: Color = Color::srgb(0.408, 0.51, 0.694);
/// A faded button's ring (`:not(.BurstReady)`).
const RING_FADED: Color = Color::srgba(1.0, 1.0, 1.0, 0.2);
/// The key's box and ink; the badge's red.
const KEY_GROUND: Color = Color::srgb(0.129, 0.153, 0.149);
const KEY_INK: Color = Color::srgb(0.733, 0.663, 0.584);
const LOAD_GROUND: Color = Color::srgb(0.749, 0.282, 0.118);

/// One of the courier's buttons.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum CourierButton {
    Select,
    Burst,
    Shield,
    Deliver,
}

impl CourierButton {
    const ALL: [CourierButton; 4] = [
        CourierButton::Select,
        CourierButton::Burst,
        CourierButton::Shield,
        CourierButton::Deliver,
    ];

    /// The art in the cache.
    fn art(self) -> &'static str {
        match self {
            CourierButton::Select => "hud.courier",
            CourierButton::Burst => "icon.ability.courier_burst",
            CourierButton::Shield => "icon.ability.courier_shield",
            CourierButton::Deliver => "icon.ability.courier_take_stash_items",
        }
    }

    /// The ring's size, its width and the icon's size, in Dota's pixels.
    fn ring(self) -> (f32, f32, f32) {
        match self {
            CourierButton::Deliver => (DELIVER_RING, 2.0, 32.0),
            _ => (BUTTON, 1.5, BUTTON),
        }
    }

    /// The courier's ability it casts, if it casts one.
    fn ability(self) -> Option<AbilityId> {
        match self {
            CourierButton::Burst => Some(AbilityId::BURST),
            CourierButton::Shield => Some(AbilityId::SHIELD),
            CourierButton::Deliver => Some(AbilityId::TAKE_STASH),
            CourierButton::Select => None,
        }
    }

    /// The binding that does the same.
    fn action(self) -> Action {
        match self {
            CourierButton::Select => Action::SelectCourier,
            CourierButton::Burst => Action::CourierBurst,
            CourierButton::Shield => Action::CourierShield,
            CourierButton::Deliver => Action::CourierDeliver,
        }
    }
}

/// A button's ring, faded while it cannot be used.
#[derive(Component)]
struct CourierRing(CourierButton);

/// A button's icon, faded while it cannot be used.
#[derive(Component)]
struct CourierArt(CourierButton);

/// The seconds a button waits, in its middle.
#[derive(Component)]
struct CourierWait(CourierButton);

/// The box of a button's key, shown while the key is bound and the button
/// can be used.
#[derive(Component)]
struct CourierKey(CourierButton);

/// The badge of how many items the courier carries, and its count.
#[derive(Component)]
struct CourierLoad;
#[derive(Component)]
struct CourierLoadCount;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(Update, (show, load, click).run_if(|k: Res<MatchKnown>| k.0));
}

fn spawn(
    mut commands: Commands,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    docks: Res<Docks>,
) {
    for which in CourierButton::ALL {
        let outer = if which == CourierButton::Deliver {
            DELIVER
        } else {
            BUTTON
        };
        let space = if which == CourierButton::Deliver {
            0.0
        } else {
            BUTTON_SPACE
        };
        let button = commands
            .spawn((
                which,
                Button,
                Blocker,
                Node {
                    width: dota(outer),
                    height: dota(outer),
                    margin: UiRect::horizontal(dota(space)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                ChildOf(docks.courier),
            ))
            .id();
        spawn_ring(&mut commands, (&fonts, &manifest, &server), button, which);
        spawn_key(&mut commands, &fonts, button, which);
        if which == CourierButton::Select {
            spawn_load(&mut commands, &fonts, docks.courier);
        }
    }
}

/// A button's round ring with its glow, its icon and its wait.
fn spawn_ring(
    commands: &mut Commands,
    (fonts, manifest, server): (&Fonts, &Manifest, &AssetServer),
    button: Entity,
    which: CourierButton,
) {
    let (size, edge, icon) = which.ring();
    let ring = commands
        .spawn((
            CourierRing(which),
            Node {
                width: dota(size),
                height: dota(size),
                border: UiRect::all(dota(edge)),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            ring_glow(),
            BorderGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::auto(RING_TOP),
                ColorStop::auto(RING_FOOT),
            ])),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.5),
                px(0),
                dota(1.0),
                px(0),
                dota(8.0),
            ),
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id();
    commands.spawn((
        CourierArt(which),
        Node {
            width: dota(icon - 2.0 * ICON_PADDING),
            height: dota(icon - 2.0 * ICON_PADDING),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        ui::art(manifest, server, which.art(), RING_FOOT),
        Pickable::IGNORE,
        ChildOf(ring),
    ));
    commands.spawn((
        CourierWait(which),
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        ui::text("", &fonts.bold, 14.0 * DOTA, Color::WHITE),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(ring),
    ));
}

/// The glow inside a ring: Dota's blue, fading out to the edge.
fn ring_glow() -> BackgroundGradient {
    BackgroundGradient::from(RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::ClosestSide,
        vec![
            ColorStop::new(RING_FOOT.with_alpha(0.447), percent(0)),
            ColorStop::new(RING_FOOT.with_alpha(0.145), percent(75)),
            ColorStop::new(RING_FOOT.with_alpha(0.0), percent(100)),
        ],
    ))
}

/// The box of a button's key over its top left, as Dota's `HotkeyContainer`
/// sits before the button, 8 pixels over the middle row's top.
fn spawn_key(commands: &mut Commands, fonts: &Fonts, button: Entity, which: CourierButton) {
    let (outer, space) = match which {
        CourierButton::Deliver => (DELIVER, 0.0),
        _ => (BUTTON, BUTTON_SPACE),
    };
    let lift = (60.0 - outer) / 2.0 - 8.0;
    let key = commands
        .spawn((
            CourierKey(which),
            Node {
                position_type: PositionType::Absolute,
                left: dota(-space),
                top: dota(-lift),
                min_width: dota(KEY_BOX),
                height: dota(KEY_BOX),
                padding: UiRect::horizontal(dota(2.0)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(dota(3.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(KEY_GROUND),
            BorderColor::all(Color::BLACK),
            GlobalZIndex(1),
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id();
    commands.spawn((
        KeyLabel(which.action()),
        ui::text("", &fonts.regular, 11.0 * DOTA, KEY_INK),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(key),
    ));
}

/// The red badge of how many items the courier carries, over the select
/// button's top right.
fn spawn_load(commands: &mut Commands, fonts: &Fonts, row: Entity) {
    let badge = commands
        .spawn((
            CourierLoad,
            Node {
                width: dota(LOAD_BADGE),
                height: dota(LOAD_BADGE),
                margin: UiRect::new(dota(-LOAD_BADGE), px(0), dota(3.0), px(0)),
                align_self: AlignSelf::FlexStart,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(LOAD_GROUND),
            BorderColor::all(Color::srgb(0.2, 0.0, 0.0)),
            Visibility::Hidden,
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    commands.spawn((
        CourierLoadCount,
        ui::text("", &fonts.regular, 16.0 * DOTA, Color::WHITE),
        Pickable::IGNORE,
        ChildOf(badge),
    ));
}

/// What a button shows now.
#[derive(Clone, Copy, PartialEq, Eq)]
struct State {
    /// Whether the button is there at all: the burst and the shield only
    /// for a courier that has them.
    shown: bool,
    /// Whether it can be used now.
    usable: bool,
    /// Seconds its ability waits.
    wait: u32,
}

/// What a button shows now.
fn state(core: &bota_client_core::App, which: CourierButton) -> State {
    let courier = core.my_courier().and_then(|id| core.view()?.unit(id));
    let casts = which.ability().is_some() && which != CourierButton::Deliver;
    let Some(courier) = courier else {
        return State {
            shown: !casts,
            usable: which == CourierButton::Select,
            wait: 0,
        };
    };
    let Some(id) = which.ability() else {
        return State {
            shown: true,
            usable: true,
            wait: 0,
        };
    };
    let Some(held) = courier.abilities.iter().find(|a| a.id == id) else {
        return State {
            shown: !casts,
            usable: false,
            wait: 0,
        };
    };
    let rate = u32::from(core.tick_rate()).max(1);
    let stash = core
        .view()
        .and_then(|v| v.player(core.my_slot?))
        .and_then(|p| p.stash.as_ref())
        .is_some_and(|s| s.iter().any(Option::is_some));
    State {
        shown: true,
        usable: held.cooldown_left == 0 && (which != CourierButton::Deliver || stash),
        wait: held.cooldown_left.div_ceil(rate),
    }
}

/// The buttons, shown to a player: faded while unusable, with their wait,
/// and their keys while bound and usable.
#[allow(clippy::type_complexity)]
fn show(
    session: NonSend<Session>,
    settings: Res<Settings>,
    mut buttons: Query<(&CourierButton, &mut Visibility, &mut Node)>,
    mut rings: Query<(&CourierRing, &mut BorderGradient, &mut BackgroundGradient)>,
    mut art: Query<(&CourierArt, &mut ImageNode)>,
    mut waits: Query<(&CourierWait, &mut Text)>,
    mut keys: Query<(&CourierKey, &mut Visibility), Without<CourierButton>>,
) {
    let core = &session.0;
    let playing = core.my_slot.is_some();
    for (which, mut visibility, mut node) in &mut buttons {
        visibility.set_if_neq(ui::visible(playing));
        let display = if state(core, *which).shown {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (CourierRing(which), mut border, mut glow) in &mut rings {
        fade_ring(state(core, *which).usable, &mut border, &mut glow);
    }
    for (CourierArt(which), mut image) in &mut art {
        let tint = Color::WHITE.with_alpha(if state(core, *which).usable { 1.0 } else { 0.3 });
        if image.color != tint && image.color != Color::NONE {
            image.color = tint;
        }
    }
    for (CourierWait(which), mut text) in &mut waits {
        let wait = state(core, *which).wait;
        let s = if wait > 0 {
            wait.to_string()
        } else {
            String::new()
        };
        text.set_if_neq(Text(s));
    }
    for (CourierKey(which), mut visibility) in &mut keys {
        let bound = settings.key(which.action()).is_some();
        visibility.set_if_neq(ui::visible(bound && state(core, *which).usable));
    }
}

/// A ring in Dota's blue with its glow, or faded and bare while its button
/// cannot be used.
fn fade_ring(usable: bool, border: &mut Mut<BorderGradient>, glow: &mut Mut<BackgroundGradient>) {
    let (top, foot, alpha) = if usable {
        (RING_TOP, RING_FOOT, 1.0)
    } else {
        (RING_FADED, RING_FADED, 0.0)
    };
    let edge = BorderGradient::from(LinearGradient::to_bottom(vec![
        ColorStop::auto(top),
        ColorStop::auto(foot),
    ]));
    border.set_if_neq(edge);
    let ground = if alpha > 0.0 {
        ring_glow()
    } else {
        BackgroundGradient::default()
    };
    glow.set_if_neq(ground);
}

/// The badge of how many items one's courier carries, while it carries any.
fn load(
    session: NonSend<Session>,
    mut badge: Single<&mut Visibility, With<CourierLoad>>,
    mut count: Single<&mut Text, With<CourierLoadCount>>,
) {
    let core = &session.0;
    let carried = core
        .my_courier()
        .and_then(|id| core.view()?.unit(id))
        .map_or(0, |c| c.items.iter().filter(|i| i.is_some()).count());
    badge.set_if_neq(ui::visible(carried > 0));
    count.set_if_neq(Text(carried.to_string()));
}

/// A click selects the courier, or sends it the button's order.
fn click(
    mut session: NonSendMut<Session>,
    buttons: Query<(&CourierButton, &Interaction), Changed<Interaction>>,
) {
    let core = &mut session.0;
    let live = matches!(core.source, bota_client_core::Source::Live(_));
    let Some((which, _)) = buttons
        .iter()
        .find(|(_, i)| live && **i == Interaction::Pressed)
    else {
        return;
    };
    match which {
        CourierButton::Select => core.act(Intent::SelectCourier),
        CourierButton::Deliver => core.act(Intent::CourierDeliver),
        CourierButton::Burst => cast(core, CourierCast::Burst),
        CourierButton::Shield => cast(core, CourierCast::Shield),
    }
}

/// The courier's own casts, by button or binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CourierCast {
    Burst,
    Shield,
}

/// Orders one's courier to cast its burst or its shield.
pub fn cast(core: &mut bota_client_core::App, which: CourierCast) {
    let ability = match which {
        CourierCast::Burst => AbilityId::BURST,
        CourierCast::Shield => AbilityId::SHIELD,
    };
    if let Some(slot) = core.courier_slot(ability.0) {
        core.send_order_to(
            core.my_courier(),
            Order::Cast {
                slot: AbilitySlot(slot),
                target: Target::None,
            },
        );
    }
}
