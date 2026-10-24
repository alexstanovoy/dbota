//! Dota's error line (`DOTAErrorMsg`): what an order turned down tells the
//! player, in Dota's words out of the install and with Dota's sound, on a dark
//! red plate below the middle of the screen. Only what Dota itself tells, or
//! what the player has to act on, is told; the rest (an order of a dead hero,
//! a target gone the moment it was clicked) goes by without a word, as in
//! Dota.

use bevy::prelude::*;
use bota_client_core::Refused;
use bota_proto::RejectReason;

use crate::dota_text::Texts;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// How a refusal is told: the key of Dota's line in the localization (none:
/// the client's own words, for what Dota has no line of) and its sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Told {
    pub key: Option<&'static str>,
    pub sound: Deny,
}

/// The sounds of Dota's refusals (`General.NoGold`, `General.CastFail_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deny {
    /// `ui_general_deny`, which most of them play.
    General,
    /// `deny_cooldown`: an ability or item still recharging.
    Cooldown,
    /// `deny_mana`: not enough mana.
    Mana,
}

impl Deny {
    /// The cache's sound of it.
    pub fn logical(self) -> &'static [&'static str] {
        match self {
            Deny::General => &["sound.no_gold"],
            Deny::Cooldown => &["sound.deny.cooldown"],
            Deny::Mana => &["sound.deny.mana"],
        }
    }
}

const DENY: Deny = Deny::General;

/// How Dota tells a refusal, or nothing for one it lets go by.
pub fn told(reason: RejectReason) -> Option<Told> {
    use RejectReason as Why;
    let dota = |key, sound| {
        Some(Told {
            key: Some(key),
            sound,
        })
    };
    let own = Some(Told {
        key: None,
        sound: DENY,
    });
    match reason {
        Why::WrongTargetKind => dota("dota_hud_error_cant_cast_on_other", DENY),
        Why::OnCooldown => dota("dota_hud_error_ability_in_cooldown", Deny::Cooldown),
        Why::NotReady => dota("dota_hud_error_item_in_cooldown", Deny::Cooldown),
        Why::NotEnoughMana => dota("dota_hud_error_not_enough_mana", Deny::Mana),
        Why::NotEnoughGold => dota("dota_hud_error_not_enough_gold", DENY),
        Why::NotLearned => dota("dota_hud_error_ability_not_learned", DENY),
        Why::NoCharges => dota("dota_hud_error_no_charges", DENY),
        Why::NotAtShop => dota("dota_hud_error_tutorial_shop_not_in_range", DENY),
        Why::InventoryFull => dota("dota_hud_error_cant_pick_up_item", DENY),
        Why::Disabled => dota("dota_hud_error_unit_command_restricted", DENY),
        Why::NotYourItem | Why::Unsellable => dota("dota_hud_error_cant_sell_item", DENY),
        Why::ClosedGround => dota("dota_hud_error_cant_cast_here", DENY),
        Why::OutOfStock => dota("dota_hud_error_item_out_of_stock", DENY),
        Why::QueueFull => dota("dota_hud_error_queue_full", DENY),
        // What Dota has no line of, and the player must still hear to act:
        // a cheat this match does not honour, a seat the server will not give.
        Why::NoCheats | Why::BadCheat | Why::NotAdmitted | Why::SeatHeld => own,
        // What the player needs no word for: the hero is dead (its panel
        // says so), the target was gone when the order came, or what the
        // interface never offers (an empty slot, a passive, another's unit,
        // an item that is not there, a talent shown as closed).
        Why::HeroDead
        | Why::UnknownTarget
        | Why::EmptySlot
        | Why::NotCastable
        | Why::NotYourUnit
        | Why::UnknownItem
        | Why::NotInBag
        | Why::CannotLearn => None,
    }
}

/// The line a refusal shows, if any: Dota's from the cache, else the
/// simulator's plain words.
pub fn line(refused: &Refused, texts: &Texts) -> Option<String> {
    match refused {
        Refused::Rule(reason) => {
            let told = told(*reason)?;
            Some(
                told.key
                    .and_then(|key| texts.label(key, &[]))
                    .unwrap_or_else(|| capitalised(refused.text())),
            )
        }
        Refused::Said(line) => Some(line.clone()),
    }
}

/// The plain words with a capital first letter, as a line of the HUD.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// The plate.
#[derive(Component)]
struct Plate;

/// The line on it.
#[derive(Component)]
struct Line;

/// Dota's plate: a gradient from #780000aa to #590000aa, as one colour.
const PLATE: Color = Color::srgba(0.41, 0.0, 0.0, 0.67);
/// Dota's text shadow of the line.
const SHADOW: Color = Color::srgb(0.35, 0.0, 0.0);

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(Update, show);
}

fn spawn(mut commands: Commands, fonts: Res<Fonts>) {
    let row = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                // Dota's margin-top: 70%, of the screen's height here.
                top: percent(66),
                justify_content: JustifyContent::Center,
                ..default()
            },
            // Over the strategy screen, under the hero pick's.
            GlobalZIndex(35),
            Pickable::IGNORE,
        ))
        .id();
    commands.spawn((
        Plate,
        Node {
            padding: UiRect::axes(px(15), px(4)),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(PLATE),
        Pickable::IGNORE,
        Visibility::Hidden,
        ChildOf(row),
        children![(
            Line,
            ui::text("", &fonts.regular, 28.0, Color::WHITE),
            TextShadow {
                offset: Vec2::ZERO,
                color: SHADOW,
            },
        )],
    ));
}

/// Shows the latest refusal's line while the client keeps it.
fn show(
    session: NonSend<Session>,
    texts: Res<Texts>,
    mut plate: Single<&mut Visibility, With<Plate>>,
    mut text: Single<&mut Text, With<Line>>,
) {
    let line = session
        .0
        .reject
        .as_ref()
        .and_then(|(refused, _)| line(refused, &texts));
    plate.set_if_neq(ui::visible(line.is_some()));
    if let Some(line) = line
        && text.0 != line
    {
        text.0 = line;
    }
}
