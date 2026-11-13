//! Buyback as Dota's HUD shows it. While the hero the panel is about lies dead, a button along
//! the foot of its portrait (`death_panel_buyback`'s `#BuybackButton`): the word, and under it
//! the coin with the cost, or a clock's red time while the cooldown runs; dimmed when the seat
//! cannot buy back, and a click buys back when it can. On the shop button, a gold frame while
//! the seat has buyback (`#ShopButtonBuybackOverlay`), and over it while Alt is held the
//! header with what the gold leaves over the cost, or the cooldown (`#BuybackHeader`).

use bevy::prelude::*;
use bevy::{
    picking::{events::PointerPress, hover::Hovered, pointer::PointerButton},
    ui_widgets::Button,
};
use bota_client_core::{App as Core, BuybackStatus};

use crate::dota_text::Texts;
use crate::hud::{Blocker, subject};
use crate::manifest::Manifest;
use crate::session::{MatchKnown, Session};
use crate::ui::{self, DOTA, Fonts, dota};

/// What the button shows, for the systems that dress it and the tests that read it.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BuybackButton {
    /// No dead hero of a seat whose buyback is told: no button.
    #[default]
    Hidden,
    /// A click buys back for this much.
    Ready { cost: i32 },
    /// The seat lacks the gold: the cost, dimmed.
    Short { cost: i32 },
    /// The cooldown runs for so many seconds more.
    Cooling { seconds: u32 },
}

/// The button's word.
#[derive(Component)]
struct Word;

/// The coin before the cost.
#[derive(Component)]
struct Coin;

/// The cost, or the cooldown's time.
#[derive(Component)]
struct Amount;

/// The gold frame round the shop button while the seat has buyback.
#[derive(Component)]
pub struct BuybackFrame;

/// The header over the shop button while Alt is held.
#[derive(Component)]
pub struct BuybackHeader;

/// The header's line.
#[derive(Component)]
struct HeaderLine;

/// The hero panel's pixels to one of Dota's (its portrait is Dota's 159 wide).
const PANEL_PER_DOTA: f32 = crate::hud::PORTRAIT.0 / 159.0;
/// The button's height (`#BuybackButton`).
const HEIGHT: f32 = 48.0;
/// The button's gradient (`#6DA18B` to `#494137`) and its top border (`#78d7bF`).
const TOP: Color = Color::srgb(0.427, 0.631, 0.545);
const FOOT: Color = Color::srgb(0.286, 0.255, 0.216);
const RIM: Color = Color::srgb(0.471, 0.843, 0.749);
/// The ground of a button that cannot be pressed: Dota takes the button's style off it.
const OFF_TOP: Color = Color::srgb(0.16, 0.17, 0.18);
const OFF_FOOT: Color = Color::srgb(0.09, 0.09, 0.1);
/// The cost's gold (`#EFDB63`) and the red of what cannot be bought (`#ff2020`).
const COST_INK: Color = Color::srgb(0.937, 0.859, 0.388);
const RED: Color = Color::srgb(1.0, 0.125, 0.125);
/// The frame's gold (`#ffd932`) and the header's ground (`#4C5059`).
const FRAME: Color = Color::srgb(1.0, 0.851, 0.196);
const HEADER_GROUND: Color = Color::srgb(0.298, 0.314, 0.349);
/// The header's inks: its word (`#FFCC33`, `#a6aBb1` without buyback) and a surplus (`#af2`).
const HEADER_WORD: Color = Color::srgb(1.0, 0.8, 0.2);
const HEADER_OFF: Color = Color::srgb(0.651, 0.671, 0.694);
const SURPLUS: Color = Color::srgb(0.667, 1.0, 0.133);

pub fn plugin(app: &mut App) {
    app.add_observer(click).add_systems(
        Update,
        (button, shop_marks).run_if(|k: Res<MatchKnown>| k.0),
    );
}

/// The button along the foot of the portrait, over everything else the portrait holds (Dota's
/// death panel lies over the portrait's column).
pub fn spawn_button(commands: &mut Commands, fonts: &Fonts, portrait: Entity) {
    let button = commands
        .spawn((
            BuybackButton::Hidden,
            Button,
            Hovered::default(),
            Blocker,
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(0),
                height: px(HEIGHT * PANEL_PER_DOTA),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::top(px(2)),
                ..default()
            },
            ground(true),
            BorderColor::all(RIM),
            ZIndex(1000),
            ChildOf(portrait),
        ))
        .id();
    commands.spawn((
        Word,
        ui::text("BUYBACK", &fonts.bold, 15.0 * PANEL_PER_DOTA, Color::WHITE),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    let row = commands
        .spawn((
            Node {
                column_gap: px(4),
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id();
    commands.spawn((
        Coin,
        Node {
            width: px(18.0 * PANEL_PER_DOTA),
            height: px(18.0 * PANEL_PER_DOTA),
            ..default()
        },
        ImageNode::default().with_color(Color::NONE),
        Pickable::IGNORE,
        ChildOf(row),
    ));
    commands.spawn((
        Amount,
        ui::text("", &fonts.bold, 18.0 * PANEL_PER_DOTA, COST_INK),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(row),
    ));
}

/// The button's ground: Dota's gradient when a click buys back, a dark one when none can.
fn ground(ready: bool) -> BackgroundGradient {
    let (top, foot) = if ready {
        (TOP, FOOT)
    } else {
        (OFF_TOP, OFF_FOOT)
    };
    BackgroundGradient::from(LinearGradient::to_bottom(vec![
        ColorStop::auto(top),
        ColorStop::auto(foot),
    ]))
}

/// The frame and the header of the shop button.
pub fn spawn_shop_marks(commands: &mut Commands, fonts: &Fonts, shop_button: Entity) {
    commands.spawn((
        BuybackFrame,
        Node {
            border: UiRect::all(dota(2.0)),
            ..ui::fill()
        },
        BorderColor::all(FRAME),
        Visibility::Hidden,
        Pickable::IGNORE,
        ChildOf(shop_button),
    ));
    let header = commands
        .spawn((
            BuybackHeader,
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(100),
                left: px(0),
                min_width: percent(100),
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(dota(4.0), dota(2.0)),
                ..default()
            },
            BackgroundColor(HEADER_GROUND),
            Visibility::Hidden,
            Pickable::IGNORE,
            ChildOf(shop_button),
        ))
        .id();
    commands.spawn((
        HeaderLine,
        ui::text("", &fonts.bold, 12.0 * DOTA, HEADER_WORD),
        TextLayout::no_wrap(),
        Pickable::IGNORE,
        ChildOf(header),
    ));
}

/// The buyback of the seat the hero panel is about, when the panel is about the seat whose
/// buyback this client is told: one's own while playing.
fn shown_status(core: &Core) -> Option<BuybackStatus> {
    let about = subject(core).0?.slot;
    let told = core.my_slot.or_else(|| core.panel_slot())?;
    (about == told).then(|| core.buyback()).flatten()
}

/// What the button shows for a status.
fn state_of(status: Option<BuybackStatus>, rate: u32) -> BuybackButton {
    match status {
        Some(status) if status.fallen => {
            if status.cooldown_left > 0 {
                BuybackButton::Cooling {
                    seconds: status.cooldown_left.div_ceil(rate),
                }
            } else if status.surplus < 0 {
                BuybackButton::Short { cost: status.cost }
            } else {
                BuybackButton::Ready { cost: status.cost }
            }
        }
        _ => BuybackButton::Hidden,
    }
}

/// Minutes and seconds, as Dota writes a cooldown.
fn clock(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[allow(clippy::type_complexity)]
fn button(
    session: NonSend<Session>,
    texts: Res<Texts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    mut button: Single<(
        &mut BuybackButton,
        &mut Node,
        &Hovered,
        &mut BackgroundGradient,
        &mut BorderColor,
    )>,
    mut word: Single<(&mut Text, &mut TextColor), (With<Word>, Without<Amount>)>,
    mut amount: Single<(&mut Text, &mut TextColor), (With<Amount>, Without<Word>)>,
    mut coin: Single<&mut ImageNode, With<Coin>>,
) {
    let core = &session.0;
    let rate = u32::from(core.tick_rate()).max(1);
    let state = state_of(shown_status(core), rate);
    button.0.set_if_neq(state);
    let display = if state == BuybackButton::Hidden {
        Display::None
    } else {
        Display::Flex
    };
    if button.1.display != display {
        button.1.display = display;
    }
    let label = texts
        .label("DOTA_Buyback", &[])
        .unwrap_or_else(|| "BUYBACK".to_owned());
    word.0.set_if_neq(Text(label));
    let (text, ink, word_ink, with_coin) = match state {
        BuybackButton::Hidden => (String::new(), COST_INK, Color::WHITE, false),
        BuybackButton::Ready { cost } => (cost.to_string(), COST_INK, Color::WHITE, true),
        BuybackButton::Short { cost } => (cost.to_string(), RED, RED, true),
        BuybackButton::Cooling { seconds } => (clock(seconds), RED, RED, false),
    };
    amount.0.set_if_neq(Text(text));
    amount.1.set_if_neq(TextColor(ink));
    word.1.set_if_neq(TextColor(word_ink));
    let ready = matches!(state, BuybackButton::Ready { .. });
    let wanted = ground(ready);
    if *button.3 != wanted {
        *button.3 = wanted;
    }
    button
        .4
        .set_if_neq(BorderColor::all(if ready { RIM } else { ui::EDGE }));
    // Brighter under the cursor when a click would buy back, as Dota's hover.
    let lit = ready && button.2.get();
    let art = manifest
        .file("hud.gold_small")
        .filter(|_| with_coin)
        .map(|path| server.load(path));
    let (image, tint) = match art {
        Some(image) if lit => (image, Color::srgb(1.6, 1.6, 1.6)),
        Some(image) => (image, Color::WHITE),
        None => (Handle::default(), Color::NONE),
    };
    if coin.image != image {
        coin.image = image;
    }
    if coin.color != tint {
        coin.color = tint;
    }
}

/// The shop button's frame while the seat shown has buyback, and its header while Alt is held.
#[allow(clippy::type_complexity)]
fn shop_marks(
    session: NonSend<Session>,
    texts: Res<Texts>,
    keys: Res<ButtonInput<KeyCode>>,
    mut frame: Single<&mut Visibility, (With<BuybackFrame>, Without<BuybackHeader>)>,
    mut header: Single<&mut Visibility, (With<BuybackHeader>, Without<BuybackFrame>)>,
    mut line: Single<(&mut Text, &mut TextColor), With<HeaderLine>>,
) {
    let core = &session.0;
    let status = core.buyback();
    let shown = |on: bool| {
        if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    frame.set_if_neq(shown(status.is_some_and(|status| status.ready())));
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    header.set_if_neq(shown(alt && status.is_some()));
    let Some(status) = status else {
        return;
    };
    let rate = u32::from(core.tick_rate()).max(1);
    let word = |key: &str, own: &str| texts.label(key, &[]).unwrap_or_else(|| own.to_owned());
    let (text, ink) = if status.cooldown_left > 0 {
        (
            format!(
                "{} ({})",
                word("DOTA_Hud_NoBuybackLabel", "No Buyback"),
                clock(status.cooldown_left.div_ceil(rate))
            ),
            HEADER_OFF,
        )
    } else if status.surplus < 0 {
        (
            format!(
                "{} {}",
                word("DOTA_Hud_NoBuybackLabel", "No Buyback"),
                status.surplus
            ),
            RED,
        )
    } else {
        (
            format!(
                "{} +{}",
                word("DOTA_Hud_BuybackLabel", "Buyback"),
                status.surplus
            ),
            SURPLUS,
        )
    };
    line.0.set_if_neq(Text(text.to_uppercase()));
    line.1.set_if_neq(TextColor(ink));
}

/// A click on the button buys back when the seat can; on a dimmed button it does nothing, as
/// Dota's.
fn click(event: On<PointerPress>, mut session: NonSendMut<Session>, button: Query<&BuybackButton>) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Ok(state) = button.get(event.entity) else {
        return;
    };
    let core = &mut session.0;
    if matches!(state, BuybackButton::Ready { .. })
        && core.my_slot.is_some()
        && core.buyback().is_some_and(|status| status.available())
    {
        core.buy_back();
    }
}

/// What the button draws: its entity, its state, whether it is laid out, and its amount's text.
#[cfg(test)]
pub(crate) fn drawn_button(world: &mut World) -> (Entity, BuybackButton, bool, String) {
    let (entity, state, node) = world
        .query::<(Entity, &BuybackButton, &Node)>()
        .single(world)
        .expect("one buyback button");
    let (state, laid_out) = (*state, node.display != Display::None);
    let amount = world
        .query_filtered::<&Text, With<Amount>>()
        .single(world)
        .expect("its amount")
        .0
        .clone();
    (entity, state, laid_out, amount)
}

/// What the shop button's marks draw: whether the frame shows, whether the header shows, and
/// the header's line.
#[cfg(test)]
pub(crate) fn drawn_shop_marks(world: &mut World) -> (bool, bool, String) {
    let shows = |visibility: &Visibility| *visibility != Visibility::Hidden;
    let frame = world
        .query_filtered::<&Visibility, With<BuybackFrame>>()
        .single(world)
        .map(shows)
        .expect("the frame");
    let header = world
        .query_filtered::<&Visibility, With<BuybackHeader>>()
        .single(world)
        .map(shows)
        .expect("the header");
    let line = world
        .query_filtered::<&Text, With<HeaderLine>>()
        .single(world)
        .expect("its line")
        .0
        .clone();
    (frame, header, line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(surplus: i32, cooldown_left: u32, fallen: bool) -> Option<BuybackStatus> {
        Some(BuybackStatus {
            cost: 246,
            surplus,
            cooldown_left,
            fallen,
        })
    }

    #[test]
    fn the_button_shows_the_cost_a_shortfall_or_the_cooldown_in_whole_seconds_and_nothing_for_the_living()
     {
        assert_eq!(state_of(None, 30), BuybackButton::Hidden);
        assert_eq!(state_of(status(500, 0, false), 30), BuybackButton::Hidden);
        assert_eq!(
            state_of(status(0, 0, true), 30),
            BuybackButton::Ready { cost: 246 }
        );
        assert_eq!(
            state_of(status(-1, 0, true), 30),
            BuybackButton::Short { cost: 246 }
        );
        assert_eq!(
            state_of(status(-1, 31, true), 30),
            BuybackButton::Cooling { seconds: 2 }
        );
        assert_eq!(clock(479), "7:59");
        assert_eq!(clock(5), "0:05");
    }
}
