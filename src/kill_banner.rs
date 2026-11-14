//! The large text under the top bar that tells of a hero's kill, as Dota's `DOTAKillStreak` panel
//! does: First Blood, a kill streak, a multi-kill. A message is the killer's hero icon and name
//! in the player's colour with the install's words, drops in with a short flourish, stays six
//! seconds and fades; a Rampage shakes in red. What to say is [`crate::kill_calls`]'s.

use bevy::color::Alpha;
use bevy::math::Rot2;
use bevy::prelude::*;
use bota_proto::{EntityId, SlotId};

use crate::dota_text::{Piece, Texts};
use crate::hud::HudRoot;
use crate::kill_calls::{Banner, Calls, banners};
use crate::manifest::Manifest;
use crate::player_colour::PlayerColours;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// Dota's `DOTAKillStreak`: the panel's width and its distance from the top, and the size of a
/// message's text and of the hero icon before it (`dota_hud_killstreak.vcss`).
const WIDTH: f32 = 800.0;
const TOP: f32 = 75.0;
const TEXT_SIZE: f32 = 26.0;
const ICON: f32 = 32.0;
const ICON_GAP: f32 = 4.0;
/// Seconds before a message starts to fade, and the fade (`KillStreakExpire`).
pub const SHOWN_SECONDS: f32 = 6.0;
pub const FADE_SECONDS: f32 = 0.5;
/// The reveal of a message (`KillStreakReveal`, `FirstBloodReveal`): its length and how large it
/// swells.
const REVEAL_SECONDS: f32 = 0.2;
const STREAK_SWELL: f32 = 1.6;
const FIRST_BLOOD_SWELL: f32 = 3.0;
/// The Rampage's shake (`KillStreakRampage`): its length, and its swing in degrees at 0, 5, 10,
/// ... 45 % of it, which is still at the end.
const RAMPAGE_SECONDS: f32 = 1.0;
const RAMPAGE_SWING: [f32; 10] = [5.0, -5.0, 4.0, -4.0, 3.0, -3.0, 2.0, -2.0, 1.0, -1.0];
const RAMPAGE_STEP: f32 = 0.05;
const RAMPAGE_RED: Color = Color::srgb(1.0, 0.0, 0.0);
/// Messages shown at once at most; the oldest gives way.
const MESSAGES: usize = 8;

/// The column the messages stand in.
#[derive(Component)]
struct Column;

/// A message and how long it has shown.
#[derive(Component)]
struct Message {
    banner: Banner,
    age: f32,
}

/// The colour a part of a message is drawn in before it fades.
#[derive(Component, Clone, Copy)]
struct Ink(Color);

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(
        Update,
        (say, pose).chain().after(crate::kill_calls::Observed),
    );
}

fn spawn(mut commands: Commands, root: Single<Entity, With<HudRoot>>) {
    commands.spawn((
        Column,
        Name::new("hud kill banner"),
        Node {
            position_type: PositionType::Absolute,
            width: ui::dota(WIDTH),
            top: ui::dota(TOP),
            left: Val::Percent(50.0),
            margin: UiRect::left(ui::dota(-WIDTH / 2.0)),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(*root),
    ));
}

/// A hero as a message shows it.
struct Hero {
    name: String,
    /// The bot version of the seat, drawn dimmer beside the name by the shared name widget.
    version: Option<String>,
    colour: Color,
    icon: Option<String>,
}

/// The hero a body is: a seat's, by its player's name and colour, else a hero no seat drives, by
/// its own name and its side's colour.
fn hero(
    core: &bota_client_core::App,
    texts: &Texts,
    manifest: &Manifest,
    colours: &PlayerColours,
    id: EntityId,
) -> Option<Hero> {
    let view = core.view()?;
    let seat: Option<SlotId> = view
        .players
        .iter()
        .find(|player| player.unit == Some(id))
        .map(|player| player.slot)
        .or_else(|| core.freshest(id)?.owner);
    if let Some(player) = seat.and_then(|slot| view.player(slot)) {
        return Some(Hero {
            name: crate::seat_name::base_seat_name(core, player.slot, Some(texts)),
            version: core.player_version(player.slot).map(str::to_owned),
            colour: colours.of(view, player.slot),
            icon: manifest.hero_icon(player.hero.0),
        });
    }
    let unit = core.freshest(id)?;
    let hero = unit.hero?;
    Some(Hero {
        name: texts.hero_name(hero).to_owned(),
        version: None,
        colour: crate::overlay::team_color(unit.team),
        icon: manifest.hero_icon(hero.0),
    })
}

/// What the messages are drawn with.
struct Stage<'a> {
    fonts: &'a Fonts,
    server: &'a AssetServer,
}

/// Puts up the messages of the frame's calls.
#[allow(clippy::too_many_arguments)]
fn say(
    mut commands: Commands,
    session: NonSend<Session>,
    calls: Res<Calls>,
    texts: Res<Texts>,
    manifest: Res<Manifest>,
    colours: Res<PlayerColours>,
    fonts: Res<Fonts>,
    server: Res<AssetServer>,
    column: Single<(Entity, Option<&Children>), With<Column>>,
) {
    let core = &session.0;
    let (column, children) = *column;
    let stage = Stage {
        fonts: &fonts,
        server: &server,
    };
    let mut standing: Vec<Entity> = children
        .into_iter()
        .flat_map(|children| children.iter())
        .collect();
    for call in &calls.0 {
        let (Some(killer), Some(victim)) = (
            hero(core, &texts, &manifest, &colours, call.killer),
            hero(core, &texts, &manifest, &colours, call.victim),
        ) else {
            continue;
        };
        for banner in banners(call) {
            let Some(pieces) = texts.chat_line(&banner.key()) else {
                continue;
            };
            while standing.len() >= MESSAGES {
                commands.entity(standing.remove(0)).despawn();
            }
            let row = spawn_row(&mut commands, column, banner);
            standing.push(row);
            for piece in &pieces {
                spawn_piece(
                    &mut commands,
                    &stage,
                    row,
                    piece,
                    (&killer, &victim),
                    banner,
                );
            }
        }
    }
}

/// The row of one message, which wraps rather than outgrow the column.
fn spawn_row(commands: &mut Commands, column: Entity, banner: Banner) -> Entity {
    commands
        .spawn((
            Message { banner, age: 0.0 },
            Node {
                align_items: AlignItems::Center,
                align_content: AlignContent::Center,
                justify_content: JustifyContent::Center,
                flex_wrap: FlexWrap::Wrap,
                max_width: Val::Percent(100.0),
                ..default()
            },
            UiTransform::default(),
            Pickable::IGNORE,
            ChildOf(column),
        ))
        .id()
}

/// One piece of a message's words in its row: the killer's icon, a hero's name or lit words.
fn spawn_piece(
    commands: &mut Commands,
    stage: &Stage,
    row: Entity,
    piece: &Piece,
    (killer, victim): (&Hero, &Hero),
    banner: Banner,
) {
    let (name, colour) = match piece {
        Piece::Argument(1) => {
            if let Some(icon) = &killer.icon {
                commands.spawn((
                    Ink(Color::WHITE),
                    ImageNode::new(stage.server.load(icon.clone())),
                    Node {
                        width: ui::dota(ICON),
                        height: ui::dota(ICON),
                        margin: UiRect::right(ui::dota(ICON_GAP)),
                        ..default()
                    },
                    Pickable::IGNORE,
                    ChildOf(row),
                ));
            }
            return;
        }
        Piece::Argument(argument @ (2 | 3)) => {
            let hero = if *argument == 2 { killer } else { victim };
            spawn_name(commands, stage, row, hero);
            return;
        }
        Piece::Argument(_) => return,
        Piece::Words(words, lit) => (
            words.clone(),
            lit.unwrap_or(if banner.rampage() {
                RAMPAGE_RED
            } else {
                Color::WHITE
            }),
        ),
    };
    commands.spawn((
        Ink(colour),
        ui::text(name, &stage.fonts.bold, TEXT_SIZE * ui::DOTA, colour),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(row),
    ));
}

/// A hero's name in a message through the shared name widget, its colour on the name and a dimmer
/// one on the version; both fade with the message ([`Ink`]).
fn spawn_name(commands: &mut Commands, stage: &Stage, row: Entity, hero: &Hero) {
    let label = crate::player_name::inline_within(
        commands,
        stage.fonts,
        row,
        (&shown_name(&hero.name), hero.version.as_deref()),
        crate::theme::Type {
            size: TEXT_SIZE * ui::DOTA,
            face: crate::theme::Face::Bold,
            ..crate::theme::BODY
        },
        hero.colour,
        NAME_WIDTH * ui::DOTA,
    );
    commands
        .entity(label.name)
        .insert((Ink(hero.colour), ui::shadow()));
    commands
        .entity(label.version)
        .insert((Ink(crate::theme::version_color(hero.colour)), ui::shadow()));
}

/// The widest a name and its version draw in a message.
const NAME_WIDTH: f32 = 330.0;

/// The most columns of a hero's name a message shows: a letter takes one and a wide glyph two. It
/// holds the longest name the game makes of a hero no player named ("Shadow Fiend (Radiant)"), and
/// cuts what players type, which a server admits to 32 bytes. Two names and the words between
/// them can still outgrow [`WIDTH`]: the row then wraps.
const NAME_COLUMNS: usize = 24;

/// A name as a message shows it: control and hidden characters dropped, white space made one
/// space, cut to [`NAME_COLUMNS`] columns with an ellipsis. Every character takes a column at
/// least (marks that stack above a letter too), so that no name is taller or wider than its cut.
pub fn shown_name(name: &str) -> String {
    let name = crate::main_menu::text::line(name, NAME_COLUMNS);
    let mut shown = String::with_capacity(name.len());
    let mut columns = 0;
    for c in name.chars() {
        // Hangul, CJK, full-width forms and the emoji all lie above U+1100.
        let width = if c >= '\u{1100}' { 2 } else { 1 };
        if columns + width > NAME_COLUMNS {
            shown.push('…');
            break;
        }
        columns += width;
        shown.push(c);
    }
    shown
}

const _: () = assert!(
    RAMPAGE_SWING.len() >= 2,
    "the shake has a segment to stand in"
);

/// How a message stands at `age`: its scale, its tilt in degrees and its opacity (Dota's
/// `KillStreakReveal`, `FirstBloodReveal`, `KillStreakRampage` and `KillStreakExpire`). An age
/// outside the message's life, or no time at all, reads as the nearest edge of it.
pub fn pose_at(banner: Banner, age: f32) -> (f32, f32, f32) {
    let age = if age.is_nan() {
        0.0
    } else {
        age.clamp(0.0, SHOWN_SECONDS + FADE_SECONDS)
    };
    let faded = 1.0 - ((age - SHOWN_SECONDS) / FADE_SECONDS).clamp(0.0, 1.0);
    if banner.rampage() {
        let t = (age / RAMPAGE_SECONDS).clamp(0.0, 1.0);
        let scale = FIRST_BLOOD_SWELL + (1.0 - FIRST_BLOOD_SWELL) * t;
        return (scale, rampage_tilt(t), faded);
    }
    let swell = if banner == Banner::FirstBlood {
        FIRST_BLOOD_SWELL
    } else {
        STREAK_SWELL
    };
    let t = (age / REVEAL_SECONDS).clamp(0.0, 1.0);
    let scale = if t < 0.5 {
        1.0 + (swell - 1.0) * (t / 0.5)
    } else {
        swell + (1.0 - swell) * ((t - 0.5) / 0.5)
    };
    (scale, 0.0, t.min(faded))
}

/// The Rampage's tilt in degrees `t` (0 to 1) through its shake: the swing's points joined, then
/// the last one eased to rest.
fn rampage_tilt(t: f32) -> f32 {
    let last = RAMPAGE_SWING.len() - 1;
    let still_from = last as f32 * RAMPAGE_STEP;
    if t >= still_from {
        return RAMPAGE_SWING[last] * (1.0 - (t - still_from) / (1.0 - still_from));
    }
    // `t` just below `still_from` can divide out to exactly `last` in f32: the segment is
    // clamped, never trusted, and the point in it kept within the segment.
    let step = t / RAMPAGE_STEP;
    let at = (step as usize).min(last - 1);
    let (from, to) = (RAMPAGE_SWING[at], RAMPAGE_SWING[at + 1]);
    from + (to - from) * (step - at as f32).clamp(0.0, 1.0)
}

/// Ages the messages, poses them and fades their parts; takes the expired away.
fn pose(
    mut commands: Commands,
    time: Res<Time>,
    mut messages: Query<(Entity, &mut Message, &mut UiTransform, &Children)>,
    mut parts: Query<(&Ink, Option<&mut TextColor>, Option<&mut ImageNode>)>,
    nested: Query<&Children>,
) {
    for (entity, mut message, mut transform, children) in &mut messages {
        message.age += time.delta_secs();
        if message.age >= SHOWN_SECONDS + FADE_SECONDS {
            commands.entity(entity).despawn();
            continue;
        }
        let (scale, tilt, opacity) = pose_at(message.banner, message.age);
        transform.scale = Vec2::splat(scale);
        transform.rotation = Rot2::degrees(tilt);
        // The parts of a message are its children, and the name widgets' texts one level below.
        let mut all: Vec<Entity> = children.to_vec();
        for child in children {
            if let Ok(inner) = nested.get(*child) {
                all.extend(inner.iter());
            }
        }
        for child in &all {
            let Ok((Ink(ink), text, image)) = parts.get_mut(*child) else {
                continue;
            };
            let shown = ink.with_alpha(ink.alpha() * opacity);
            if let Some(mut text) = text {
                text.set_if_neq(TextColor(shown));
            }
            if let Some(mut image) = image {
                image.color = shown;
            }
        }
    }
}

/// What the messages say now, in order, with whether each shakes: for the tests.
#[cfg(test)]
pub fn shown(world: &mut World) -> Vec<(String, Banner)> {
    let mut messages = world.query::<(&Message, &Children)>();
    let mut texts = world.query::<&Text>();
    let mut inner = world.query::<&Children>();
    let rows: Vec<_> = messages
        .iter(world)
        .map(|(message, children)| (message.banner, children.to_vec()))
        .collect();
    rows.into_iter()
        .map(|(banner, children)| {
            // A part is a text, or a name widget whose name and version are two texts.
            let mut said = String::new();
            for child in children {
                if let Ok(text) = texts.get(world, child) {
                    said.push_str(&text.0);
                } else if let Ok(parts) = inner.get(world, child) {
                    for part in parts.iter() {
                        if let Ok(text) = texts.get(world, part) {
                            said.push_str(&text.0);
                        }
                    }
                }
            }
            (said, banner)
        })
        .collect()
}

/// Where the column stands and where each message does, on the window: for the tests.
#[cfg(test)]
pub fn placed(world: &mut World) -> (Rect, Vec<Rect>) {
    let rect = |world: &World, entity: Entity| {
        let at = world.get::<UiGlobalTransform>(entity).expect("placed");
        let size = world.get::<ComputedNode>(entity).expect("laid out").size();
        Rect::from_center_size(at.translation, size)
    };
    let column = world
        .query_filtered::<Entity, With<Column>>()
        .single(world)
        .expect("the column");
    let rows: Vec<Entity> = world
        .query_filtered::<Entity, With<Message>>()
        .iter(world)
        .collect();
    let rows = rows.into_iter().map(|row| rect(world, row)).collect();
    (rect(world, column), rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MULTI: Banner = Banner::Multi {
        count: 3,
        named: true,
    };
    const RAMPAGE: Banner = Banner::Multi {
        count: 5,
        named: true,
    };

    #[test]
    fn a_message_swells_and_settles_while_it_appears() {
        assert_eq!(pose_at(MULTI, 0.0), (1.0, 0.0, 0.0));
        let (peak, _, _) = pose_at(MULTI, REVEAL_SECONDS / 2.0);
        assert!((peak - STREAK_SWELL).abs() < 1e-5);
        assert_eq!(pose_at(MULTI, REVEAL_SECONDS), (1.0, 0.0, 1.0));
    }

    #[test]
    fn first_blood_swells_further_than_a_streak() {
        let (blood, _, _) = pose_at(Banner::FirstBlood, REVEAL_SECONDS / 2.0);
        assert!((blood - FIRST_BLOOD_SWELL).abs() < 1e-5);
    }

    #[test]
    fn a_message_holds_six_seconds_then_fades_out_in_half_a_second() {
        assert_eq!(pose_at(MULTI, SHOWN_SECONDS).2, 1.0);
        let (_, _, half) = pose_at(MULTI, SHOWN_SECONDS + FADE_SECONDS / 2.0);
        assert!((half - 0.5).abs() < 1e-5);
        assert_eq!(pose_at(MULTI, SHOWN_SECONDS + FADE_SECONDS).2, 0.0);
    }

    #[test]
    fn a_rampage_drops_in_from_three_times_its_size_shaking_and_comes_to_rest() {
        let (scale, tilt, opacity) = pose_at(RAMPAGE, 0.0);
        assert_eq!((scale, tilt, opacity), (3.0, 5.0, 1.0));
        let (_, tilt, _) = pose_at(RAMPAGE, 0.05);
        assert!((tilt + 5.0).abs() < 1e-4, "{tilt}");
        assert_eq!(pose_at(RAMPAGE, RAMPAGE_SECONDS), (1.0, 0.0, 1.0));
    }

    /// Every age at which one of the animation tables changes its segment, and the floats beside
    /// it, then ages no clock should give.
    fn boundary_ages() -> Vec<f32> {
        let edges = (0..=RAMPAGE_SWING.len() as u32 + 2)
            .map(|n| n as f32 * RAMPAGE_STEP * RAMPAGE_SECONDS)
            .chain([
                REVEAL_SECONDS / 2.0,
                REVEAL_SECONDS,
                RAMPAGE_SECONDS,
                SHOWN_SECONDS,
                SHOWN_SECONDS + FADE_SECONDS,
            ]);
        let mut ages = Vec::new();
        for edge in edges {
            let mut below = edge;
            let mut above = edge;
            for _ in 0..4 {
                ages.extend([below, above]);
                below = below.next_down();
                above = above.next_up();
            }
        }
        ages.extend([
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
            f32::MIN,
            f32::MIN_POSITIVE,
            -0.0,
            -1.0,
            1.0e9,
        ]);
        ages
    }

    #[test]
    fn no_age_poses_a_message_outside_its_tables_or_panics() {
        let banners = [Banner::FirstBlood, Banner::Streak(3), MULTI, RAMPAGE];
        for banner in banners {
            for age in boundary_ages() {
                let (scale, tilt, opacity) = pose_at(banner, age);
                assert!(
                    (1.0..=FIRST_BLOOD_SWELL).contains(&scale),
                    "{banner:?} at {age}: scale {scale}"
                );
                assert!(tilt.abs() <= 5.0, "{banner:?} at {age}: tilt {tilt}");
                assert!(
                    (0.0..=1.0).contains(&opacity),
                    "{banner:?} at {age}: opacity {opacity}"
                );
            }
        }
    }

    #[test]
    fn a_rampage_at_the_start_of_its_stillness_has_no_gap_in_its_shake() {
        // 0.45 s divides out to the shake's last point in f32, one float below where it rests.
        let (_, before, _) = pose_at(RAMPAGE, 0.45);
        let (_, after, _) = pose_at(RAMPAGE, 0.45_f32.next_up().next_up());
        assert!((before - after).abs() < 1e-3, "{before} then {after}");
    }

    #[test]
    fn a_name_is_cut_to_its_columns_with_an_ellipsis_and_loses_its_hidden_characters() {
        let long = "W".repeat(32);
        let wide = "漢".repeat(16);
        let hostile = "\u{202E}a\u{200B}b\u{0}c";
        assert_eq!(shown_name(&long), format!("{}…", "W".repeat(NAME_COLUMNS)));
        assert_eq!(
            shown_name(&wide),
            format!("{}…", "漢".repeat(NAME_COLUMNS / 2))
        );
        assert_eq!(shown_name(hostile), "abc");
        assert_eq!(shown_name("Short"), "Short");
    }
}
