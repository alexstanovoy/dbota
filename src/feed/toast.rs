//! The messages of the toast column over the minimap, as Dota words and draws them: who did it (the
//! hero's icon and the player's name in the player's colour), a plate on the slant that carries what
//! kind of event it is, then what happened in words, with the figures lit and the coin beside the gold.
//! One message is one list of pairs of an icon and a text; each kind of event says its own in
//! [`toast`], and the row draws every one alike.

use bota_client_core::{App as Core, FeedKind, Killer, RuneAct};
use bota_proto::{SlotId, Team, UnitKind};

use super::{
    Laid, NUMBER_INK, SLOTS, Style, Styles, building_parts, icon_file, lay_parts, side_name,
};
use crate::dota_text::{Part, Texts};
use crate::manifest::Manifest;
use crate::player_colour::PlayerColours;
use crate::seat_name::seat_name;

/// One pair of a message: an icon (a cache path), a text and how they are drawn.
type Pair = (Option<String>, String, Style);

/// The message of a line that goes in the toast column, drawn with Dota's words where the cache
/// has them and the client's own otherwise.
pub(super) fn toast(
    core: &Core,
    texts: Option<&Texts>,
    manifest: &Manifest,
    colours: &PlayerColours,
    line: &bota_client_core::FeedLine,
) -> (Laid, Styles) {
    let kind = &line.kind;
    let pairs = texts.and_then(|texts| match *kind {
        FeedKind::StructureFell { .. } => structure(core, texts, manifest, colours, kind),
        FeedKind::Rune {
            by,
            team,
            kind: rune,
            act,
        } => rune_taken(core, texts, manifest, colours, (by, team), rune, act),
        FeedKind::ShrineActivated { by, team, xp } => {
            shrine(core, texts, manifest, colours, (by, team), xp)
        }
        _ => None,
    });
    match pairs {
        Some(pairs) => arrange(pairs),
        None => {
            // Dota's words are not in the cache (or the line is of a kind they do not word): the
            // client's own line, with the kind's plate where it leads with an icon.
            let laid = lay_parts(core, texts, manifest, line);
            let mut styles = Styles::default();
            if matches!(kind, FeedKind::Captured { .. }) && laid[0].0.is_some() {
                styles[0].plate = true;
            }
            (laid, styles)
        }
    }
}

/// The pairs of a message in a line's slots.
fn arrange(pairs: Vec<Pair>) -> (Laid, Styles) {
    let mut laid: Laid = Default::default();
    let mut styles = Styles::default();
    for (at, (icon, text, style)) in pairs.into_iter().take(SLOTS).enumerate() {
        laid[at] = (icon, text);
        styles[at] = style;
    }
    (laid, styles)
}

/// The hero a seat plays, by its icon in the cache.
fn hero_icon(core: &Core, manifest: &Manifest, slot: SlotId) -> Option<String> {
    let hero = core.view()?.player(slot)?.hero;
    manifest.hero_icon(hero.0)
}

/// Who did it: a seat by its hero's icon and its name in its colour, a side by its name in its
/// colour.
fn actor(
    core: &Core,
    texts: Option<&Texts>,
    manifest: &Manifest,
    colours: &PlayerColours,
    (by, team): (Option<SlotId>, Team),
) -> Pair {
    match by {
        Some(slot) => (
            hero_icon(core, manifest, slot),
            seat_name(core, slot, texts),
            Style {
                ink: core
                    .view()
                    .map_or(bevy::color::Color::WHITE, |view| colours.of(view, slot)),
                plate: false,
            },
        ),
        None => (
            None,
            side_name(team).to_owned(),
            Style {
                ink: crate::overlay::team_color(team),
                plate: false,
            },
        ),
    }
}

/// What kind of event it is, on its plate.
fn mark(manifest: &Manifest, icon: Option<String>) -> Pair {
    (
        icon.or_else(|| manifest.file("hud.kill")),
        String::new(),
        Style {
            plate: true,
            ..Style::default()
        },
    )
}

fn words(text: impl Into<String>) -> Pair {
    (None, text.into(), Style::default())
}

/// A figure, lit.
fn number(figure: impl ToString) -> Pair {
    (
        None,
        figure.to_string(),
        Style {
            ink: NUMBER_INK,
            plate: false,
        },
    )
}

/// The gold coin before a word that follows a sum.
fn coin(manifest: &Manifest, then: &str) -> Pair {
    (manifest.file("hud.gold"), then.to_owned(), Style::default())
}

/// A tower or a barracks that fell: its destroyer when a seat, the sword of the side that struck it,
/// its name in Dota's words and, for a tower, the gold its fall pays each seat.
fn structure(
    core: &Core,
    texts: &Texts,
    manifest: &Manifest,
    colours: &PlayerColours,
    kind: &FeedKind,
) -> Option<Vec<Pair>> {
    let FeedKind::StructureFell {
        team,
        kind: building,
        tier,
        killer,
        ..
    } = *kind
    else {
        return None;
    };
    let name: String = building_parts(texts, kind)?
        .into_iter()
        .filter_map(|part| match part {
            Part::Text(text) => Some(text),
            Part::Icon(_) => None,
        })
        .collect();
    let sword = icon_file(core, "killed_icon", kind).and_then(|logical| manifest.file(logical));
    let mut pairs = Vec::new();
    if let Some(Killer::Seat(slot)) = killer {
        pairs.push(actor(core, Some(texts), manifest, colours, (Some(slot), team)));
    }
    pairs.push(mark(manifest, sword));
    pairs.push(words(format!("{} ", name.trim())));
    let paid = tier
        .filter(|_| building == UnitKind::Tower)
        .and_then(|tier| {
            let at = usize::from(tier).checked_sub(1)?;
            bota_core::rules::TOWER_TIER_TEAM_BOUNTY.get(at).copied()
        });
    if let Some(gold) = paid {
        pairs.push(number(gold));
        pairs.push(coin(manifest, "each"));
    }
    Some(pairs)
}

/// A rune taken, bottled or destroyed: who, the rune's own icon on the plate, and its name in its
/// colour's words; a Bounty Rune adds its gold.
fn rune_taken(
    core: &Core,
    texts: &Texts,
    manifest: &Manifest,
    colours: &PlayerColours,
    who: (Option<SlotId>, Team),
    rune: bota_proto::RuneKind,
    act: RuneAct,
) -> Option<Vec<Pair>> {
    let name = crate::runes::text_name(rune);
    let label = |key: String| texts.label(&key, &[("gold", String::new())]);
    let (said, gold) = match act {
        RuneAct::Activated { gold } => (label(format!("DOTA_HUD_Rune_{name}"))?, gold),
        RuneAct::Bottled => (
            format!("bottled {}", label(format!("DOTA_HUD_Rune_{name}"))?.trim()),
            None,
        ),
        RuneAct::Denied => (label(format!("DOTA_HUD_Rune_Deny{name}"))?, None),
    };
    let icon = icon_file(
        core,
        "rune_icon",
        &FeedKind::Rune {
            by: who.0,
            team: who.1,
            kind: rune,
            act,
        },
    )
    .and_then(|logical| manifest.file(logical));
    let mut pairs = vec![
        actor(core, Some(texts), manifest, colours, who),
        mark(manifest, icon),
        words(said.trim().to_owned()),
    ];
    if let Some(gold) = gold {
        pairs.push(number(gold));
        pairs.push(coin(manifest, ""));
    }
    Some(pairs)
}

/// A Shrine of Wisdom gathered: who, the wisdom rune's icon, and the Shrine's words with its
/// experience lit.
fn shrine(
    core: &Core,
    texts: &Texts,
    manifest: &Manifest,
    colours: &PlayerColours,
    who: (Option<SlotId>, Team),
    xp: i32,
) -> Option<Vec<Pair>> {
    let said = texts.label("DOTA_HUD_Rune_XP", &[("xp", xp.to_string())])?;
    let figure = xp.to_string();
    let (before, after) = said.split_once(&figure)?;
    Some(vec![
        actor(core, Some(texts), manifest, colours, who),
        mark(manifest, manifest.file("hud.rune_timer.wisdom")),
        words(before.to_owned()),
        number(&figure),
        words(after.to_owned()),
    ])
}
