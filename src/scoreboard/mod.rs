//! Dota's scoreboard: shown while its key is held (Dota's `+showscores`,
//! the backquote by default) or pinned open by the button in the top-left
//! menu, under the top bar at the left. Each side's heading has its kills
//! and its name; each seat's row its colour, hero, player and hero names,
//! talents, level, gold, kills, deaths and assists, and ultimate, the
//! columns of Dota's `dota_hud_scoreboard` (last hits and denies are one's
//! own quick stats', at the top left, as in Dota); beside the rows, while Show Items is on, every
//! seat's inventory, backpack and neutral slot. What a row says of the other
//! side is only what the watching side may know (see [`model`]).

mod draw;
mod model;
mod row;

use bevy::prelude::*;
use bota_proto::TalentSlot;

use crate::keys::{Action, Fired};
use crate::session::{MatchKnown, Session};
use bota_proto::Team;
pub use draw::BOARD_Z;
use draw::{At, Bags, Board, Field, Inventories, Seats, ShowItems, TeamScore};
pub use model::{Line, Ult, lines};

/// A talent's pip, not learned and learned.
const PIP_SHUT: Color = Color::srgb(0.27, 0.29, 0.33);
const PIP_LIT: Color = Color::srgb(0.95, 0.75, 0.3);

/// The rows as this frame's view tells them, worked out once a frame while
/// the scoreboard shows.
#[derive(Resource, Default)]
struct Rows(Vec<Line>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Rows>()
        .add_systems(Startup, (spawn, open_at_start))
        .add_systems(Update, seat_rows.run_if(|known: Res<MatchKnown>| known.0))
        .add_systems(Update, (show, show_items).chain())
        .add_systems(
            Update,
            (gather, (heads, rows, texts, pictures, pips, rings))
                .chain()
                .after(show)
                .run_if(shown),
        );
}

fn shown(known: Res<MatchKnown>, board: Query<&Visibility, With<Board>>) -> bool {
    known.0 && board.iter().any(|v| *v != Visibility::Hidden)
}

fn gather(session: NonSend<Session>, mut rows: ResMut<Rows>) {
    rows.0 = lines(&session.0);
}

fn spawn(
    mut commands: Commands,
    fonts: Res<crate::ui::Fonts>,
    manifest: Res<crate::manifest::Manifest>,
    server: Res<AssetServer>,
    texts: Res<crate::dota_text::Texts>,
    root: Single<Entity, With<crate::hud::HudRoot>>,
) {
    let mut kit = draw::Kit {
        commands: &mut commands,
        fonts: &fonts,
        manifest: &manifest,
        server: &server,
        texts: &texts,
    };
    draw::spawn(&mut kit, *root);
}

/// A row for every seat the match has, each side's in seat order, made
/// anew when the seats change: Dota's scoreboard lists the players in the
/// game and no empty slot.
#[allow(clippy::too_many_arguments)]
fn seat_rows(
    mut commands: Commands,
    fonts: Res<crate::ui::Fonts>,
    manifest: Res<crate::manifest::Manifest>,
    server: Res<AssetServer>,
    texts: Res<crate::dota_text::Texts>,
    session: NonSend<Session>,
    seats: Query<(Entity, &Seats)>,
    bags: Query<(Entity, &Bags)>,
    mut had: Local<[usize; 2]>,
) {
    let Some(view) = session.0.view() else {
        return;
    };
    let sides = [Team::Radiant, Team::Dire];
    let count = |team| {
        let seats = view.players.iter().filter(|p| p.team == team).count();
        seats.min(draw::MAX_ROWS)
    };
    let want = sides.map(count);
    if want == *had {
        return;
    }
    *had = want;
    let mut kit = draw::Kit {
        commands: &mut commands,
        fonts: &fonts,
        manifest: &manifest,
        server: &server,
        texts: &texts,
    };
    for (team, n) in sides.into_iter().zip(want) {
        let rows = seats.iter().find(|(_, s)| s.0 == team).map(|(e, _)| e);
        let items = bags.iter().find(|(_, b)| b.0 == team).map(|(e, _)| e);
        let (Some(rows), Some(bags)) = (rows, items) else {
            continue;
        };
        kit.commands.entity(rows).despawn_children();
        kit.commands.entity(bags).despawn_children();
        for rank in 0..n as u8 {
            draw::seat(&mut kit, rows, bags, At { team, rank });
        }
    }
}

/// `--scoreboard` pins it open from the start, for screenshots.
fn open_at_start(args: Res<crate::Args>, mut panels: ResMut<crate::input::Panels>) {
    panels.scoreboard |= args.scoreboard;
}

/// Shows the scoreboard while its key is held or it is pinned, in a match
/// and with the settings screen shut; and once the match is over, as Dota's
/// end of a game shows every hero's score.
fn show(
    fired: Res<Fired>,
    panels: Res<crate::input::Panels>,
    known: Res<MatchKnown>,
    screen: Res<crate::settings::Screen>,
    session: NonSend<crate::session::Session>,
    mut board: Single<&mut Visibility, With<Board>>,
) {
    let over = session.0.over.is_some();
    let wanted =
        known.0 && !screen.open && (fired.held(Action::Scoreboard) || panels.scoreboard || over);
    board.set_if_neq(crate::ui::visible(wanted));
}

/// The Show Items button turns the items beside the rows on and off, and
/// the settings file keeps the choice.
fn show_items(
    mut settings: ResMut<crate::config::Settings>,
    store: Res<crate::main_menu::SettingsStore>,
    mut button: Query<(Ref<Interaction>, &mut ImageNode), With<ShowItems>>,
    mut panel: Single<&mut Visibility, With<Inventories>>,
) {
    for (interaction, mut image) in &mut button {
        if interaction.is_changed() && *interaction == Interaction::Pressed {
            settings.scoreboard_items = !settings.scoreboard_items;
            if let Err(err) = store.save_settings(&settings) {
                warn!("settings not saved: {err}");
            }
        }
        // Dota's buttons there are faint until hovered, bright while on.
        let alpha = match (*interaction, settings.scoreboard_items) {
            (Interaction::None, false) => 0.2,
            (Interaction::None, true) => 0.6,
            _ => 1.0,
        };
        if image.color.alpha() != alpha {
            image.color.set_alpha(alpha);
        }
    }
    panel.set_if_neq(crate::ui::visible(settings.scoreboard_items));
}

/// The row of a part: the side's `rank`-th seat, in seat order.
fn line_at<'a>(lines: &'a [Line], at: &At) -> Option<&'a Line> {
    lines
        .iter()
        .filter(|l| l.team == at.team)
        .nth(usize::from(at.rank))
}

/// Each side's kills in its heading.
fn heads(session: NonSend<Session>, mut scores: Query<(&TeamScore, &mut Text)>) {
    let Some(view) = session.0.view() else {
        return;
    };
    for (score, mut text) in &mut scores {
        let kills: u32 = view
            .players
            .iter()
            .filter(|p| p.team == score.0)
            .map(|p| u32::from(p.kills))
            .sum();
        text.set_if_neq(Text(kills.to_string()));
    }
}

/// One's own row lit as Dota lights it; the others on the plain ground.
fn rows(
    lines: Res<Rows>,
    mut rows: Query<(&At, &Field, &mut BackgroundColor, &mut BackgroundGradient)>,
) {
    let lines = &lines.0;
    for (at, field, mut ground, mut gradient) in &mut rows {
        if *field != Field::Row {
            continue;
        }
        let own = line_at(lines, at).is_some_and(|l| l.local);
        let want = if own {
            draw::own_row()
        } else {
            BackgroundGradient::default()
        };
        gradient.set_if_neq(want);
        ground.set_if_neq(BackgroundColor(draw::color(draw::ROW)));
    }
}

/// Every number and name of the rows; empty where the seat is empty or the
/// thing is not to be known.
fn texts(lines: Res<Rows>, mut texts: Query<(&At, &Field, &mut Text, &mut TextColor)>) {
    for (at, field, mut text, mut color) in &mut texts {
        let line = line_at(&lines.0, at);
        let s = line.map(|l| text_of(l, *field)).unwrap_or_default();
        text.set_if_neq(Text(s));
        if matches!(field, Field::Kills | Field::Deaths | Field::Assists) {
            let own = line.is_some_and(|l| l.local);
            let want = draw::color(if own { 0xffffffff } else { draw::SCORE });
            color.set_if_neq(TextColor(want));
        }
    }
}

fn text_of(l: &Line, field: Field) -> String {
    let number = |n: Option<i32>| n.map(|n| n.to_string()).unwrap_or_default();
    match field {
        Field::Name => l.name.clone(),
        Field::HeroName => bota_core::hero_def(l.hero)
            .map(|h| h.name.to_uppercase())
            .unwrap_or_default(),
        Field::Respawn => l.respawn.map(|s| s.to_string()).unwrap_or_default(),
        Field::RingLevel => l.level.to_string(),
        Field::Level if l.foe => l.level.to_string(),
        Field::Gold => number(l.gold),
        Field::Kills => l.kills.to_string(),
        Field::Deaths => l.deaths.to_string(),
        Field::Assists => l.assists.to_string(),
        Field::UltSeconds => match l.ult {
            Some(Ult::Cooldown(s)) => s.to_string(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// The player's colour, the hero's picture (grey while it is down), the
/// ultimate's diamond and the items (dimmed as last seen).
fn pictures(
    session: NonSend<Session>,
    lines: Res<Rows>,
    manifest: Res<crate::manifest::Manifest>,
    server: Res<AssetServer>,
    colours: Res<crate::player_colour::PlayerColours>,
    mut pictures: Query<(&At, &Field, &mut ImageNode)>,
) {
    let view = session.0.view();
    for (at, field, mut image) in &mut pictures {
        let line = line_at(&lines.0, at);
        let (path, tint) = match (*field, line) {
            (_, None) => (None, Color::NONE),
            (Field::Color, Some(l)) => {
                let color = view.map_or(Color::WHITE, |v| colours.of(v, l.slot));
                (manifest.file("hud.scoreboard_slot"), color)
            }
            (Field::Hero, Some(l)) => {
                let grey = Color::srgb(0.3, 0.3, 0.3);
                let tint = if l.respawn.is_some() {
                    grey
                } else {
                    Color::WHITE
                };
                (manifest.portrait(l.hero.0), tint)
            }
            (Field::Ult, Some(l)) => (ult_art(&manifest, l.ult), Color::WHITE),
            (Field::Item(slot), Some(l)) => {
                let item = l.items[usize::from(slot)];
                let tint = if l.stale {
                    Color::srgb(0.55, 0.55, 0.55)
                } else {
                    Color::WHITE
                };
                (item.and_then(|i| manifest.item_icon(i.0)), tint)
            }
            _ => continue,
        };
        // Where the cache has no vignette, the colour fills the edge.
        let shown = if path.is_none() && *field == Field::Color {
            Some(Handle::default())
        } else {
            path.map(|p| server.load(p))
        };
        let (handle, tint) = match shown {
            Some(handle) => (handle, tint),
            None => (Handle::default(), Color::NONE),
        };
        if image.image != handle {
            image.image = handle;
        }
        if image.color != tint {
            image.color = tint;
        }
    }
}

fn ult_art(manifest: &crate::manifest::Manifest, ult: Option<Ult>) -> Option<String> {
    manifest.file(match ult? {
        Ult::NotLearned => return None,
        Ult::Ready => "hud.ult_ready",
        Ult::NoMana => "hud.ult_no_mana",
        Ult::Cooldown(_) => "hud.ult_cooldown",
    })
}

/// The talents' pips, lit as learned.
fn pips(lines: Res<Rows>, mut pips: Query<(&At, &Field, &mut BackgroundColor)>) {
    for (at, field, mut color) in &mut pips {
        let Field::Pip(slot) = *field else {
            continue;
        };
        let line = line_at(&lines.0, at);
        let want = match line {
            None => Color::NONE,
            Some(l) if l.talents.has(TalentSlot(slot)) => PIP_LIT,
            Some(_) => PIP_SHUT,
        };
        color.set_if_neq(BackgroundColor(want));
    }
}

/// An ally's experience ring, filled towards the next level; none for a
/// foe, whose level Dota writes plainly.
fn rings(
    lines: Res<Rows>,
    mut rings: Query<(&At, &Field, &mut BackgroundGradient, &mut Visibility)>,
) {
    for (at, field, mut gradient, mut visibility) in &mut rings {
        if *field != Field::Ring {
            continue;
        }
        let line = line_at(&lines.0, at).filter(|l| !l.foe);
        visibility.set_if_neq(crate::ui::visible(line.is_some()));
        if let Some(l) = line {
            gradient.set_if_neq(crate::hud::xp_ring(crate::hud::xp_share(l.level, l.xp)));
        }
    }
}
