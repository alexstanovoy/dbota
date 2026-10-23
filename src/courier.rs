//! The courier's round buttons right of the gold, as Dota's corner has them:
//! select the courier, its speed burst and its shield, and deliver what
//! waits in the stash. Each shows its key where it has one, dims while it
//! cannot be used, and counts down its wait.

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{AbilityId, AbilitySlot, Order, Target};

use crate::config::{Action, Settings, key_name};
use crate::hud::{Blocker, Docks};
use crate::manifest::Manifest;
use crate::session::{MatchKnown, Session};
use crate::ui::{self, Fonts};

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

    /// The art in the cache and the button's size: delivering, the one
    /// Dota draws largest.
    fn art(self) -> (&'static str, f32) {
        match self {
            CourierButton::Select => ("hud.courier", 38.0),
            CourierButton::Burst => ("icon.ability.courier_burst", 34.0),
            CourierButton::Shield => ("icon.ability.courier_shield", 34.0),
            CourierButton::Deliver => ("icon.ability.courier_take_stash_items", 44.0),
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

    /// The key that does the same, if any.
    fn action(self) -> Option<Action> {
        match self {
            CourierButton::Select => Some(Action::SelectCourier),
            CourierButton::Deliver => Some(Action::CourierDeliver),
            _ => None,
        }
    }
}

/// The art of a button, dimmed while it cannot be used.
#[derive(Component)]
struct CourierArt(CourierButton);

/// The seconds a button waits, in its middle.
#[derive(Component)]
struct CourierWait(CourierButton);

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(Update, (show, click).run_if(|k: Res<MatchKnown>| k.0));
}

fn spawn(
    mut commands: Commands,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    settings: Res<Settings>,
    docks: Res<Docks>,
) {
    for which in CourierButton::ALL {
        let (art, size) = which.art();
        let button = commands
            .spawn((
                which,
                Button,
                Blocker,
                Node {
                    width: px(size),
                    height: px(size),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::MAX,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BorderColor::all(Color::srgb(0.42, 0.45, 0.5)),
                BackgroundColor(ui::PANEL_DEEP),
                ChildOf(docks.courier),
            ))
            .id();
        commands.spawn((
            CourierArt(which),
            Node {
                border_radius: BorderRadius::MAX,
                ..ui::fill()
            },
            ui::art(&manifest, &server, art, Color::srgb(0.35, 0.4, 0.5)),
            Pickable::IGNORE,
            ChildOf(button),
        ));
        commands.spawn((
            CourierWait(which),
            ui::text("", &fonts.bold, 14.0, ui::INK),
            ui::shadow(),
            Pickable::IGNORE,
            ChildOf(button),
        ));
        if let Some(action) = which.action() {
            commands.spawn((
                ui::text(key_name(settings.key(action)), &fonts.bold, 10.0, ui::INK),
                ui::shadow(),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(-4),
                    top: px(-6),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(button),
            ));
        }
    }
}

/// Seconds a button's ability waits, and whether it can be used now.
fn state(core: &bota_client_core::App, which: CourierButton) -> (u32, bool) {
    let courier = core.my_courier().and_then(|id| core.view()?.unit(id));
    let Some(courier) = courier else {
        return (0, false);
    };
    let Some(id) = which.ability() else {
        return (0, true);
    };
    let Some(held) = courier.abilities.iter().find(|a| a.id == id) else {
        return (0, false);
    };
    let rate = u32::from(core.tick_rate()).max(1);
    let stash = core
        .view()
        .and_then(|v| v.player(core.my_slot?))
        .and_then(|p| p.stash.as_ref())
        .is_some_and(|s| s.iter().any(Option::is_some));
    let usable = held.cooldown_left == 0 && (which != CourierButton::Deliver || stash);
    (held.cooldown_left.div_ceil(rate), usable)
}

/// The buttons, shown to a player: dimmed while unusable, with their wait.
fn show(
    session: NonSend<Session>,
    mut buttons: Query<(&CourierButton, &mut Visibility)>,
    mut art: Query<(&CourierArt, &mut ImageNode)>,
    mut waits: Query<(&CourierWait, &mut Text)>,
) {
    let core = &session.0;
    let playing = core.my_slot.is_some();
    for (_, mut visibility) in &mut buttons {
        visibility.set_if_neq(ui::visible(playing));
    }
    for (CourierArt(which), mut image) in &mut art {
        let tint = if state(core, *which).1 {
            Color::WHITE
        } else {
            Color::srgb(0.4, 0.4, 0.4)
        };
        if image.color != tint && image.color != Color::NONE {
            image.color = tint;
        }
    }
    for (CourierWait(which), mut text) in &mut waits {
        let wait = state(core, *which).0;
        let s = if wait > 0 {
            wait.to_string()
        } else {
            String::new()
        };
        text.set_if_neq(Text(s));
    }
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
        CourierButton::Burst | CourierButton::Shield => {
            let ability = which.ability().map(|a| a.0);
            if let Some(slot) = ability.and_then(|a| core.courier_slot(a)) {
                core.send_order_to(
                    core.my_courier(),
                    Order::Cast {
                        slot: AbilitySlot(slot),
                        target: Target::None,
                    },
                );
            }
        }
    }
}
