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
            name: crate::seat_name::seat_name(core, player.slot, Some(texts)),
            colour: colours.of(view, player.slot),
            icon: manifest.hero_icon(player.hero.0),
        });
    }
    let unit = core.freshest(id)?;
    let hero = unit.hero?;
    Some(Hero {
        name: texts.hero_name(hero).to_owned(),
        colour: crate::overlay::team_color(unit.team),
        icon: manifest.hero_icon(hero.0),
    })
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
            let row = commands
                .spawn((
                    Message { banner, age: 0.0 },
                    Node {
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    UiTransform::default(),
                    Pickable::IGNORE,
                    ChildOf(column),
                ))
                .id();
            standing.push(row);
            let shake = banner.rampage();
            for piece in pieces {
                let (name, colour) = match &piece {
                    Piece::Argument(1) => {
                        if let Some(icon) = &killer.icon {
                            commands.spawn((
                                Ink(Color::WHITE),
                                ImageNode::new(server.load(icon.clone())),
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
                        continue;
                    }
                    Piece::Argument(2) => (killer.name.clone(), killer.colour),
                    Piece::Argument(3) => (victim.name.clone(), victim.colour),
                    Piece::Argument(_) => continue,
                    Piece::Words(words, lit) => (
                        words.clone(),
                        lit.unwrap_or(if shake { RAMPAGE_RED } else { Color::WHITE }),
                    ),
                };
                commands.spawn((
                    Ink(colour),
                    ui::text(name, &fonts.bold, TEXT_SIZE * ui::DOTA, colour),
                    ui::shadow(),
                    Pickable::IGNORE,
                    ChildOf(row),
                ));
            }
        }
    }
}

/// How a message stands at `age`: its scale, its tilt in degrees and its opacity (Dota's
/// `KillStreakReveal`, `FirstBloodReveal`, `KillStreakRampage` and `KillStreakExpire`).
pub fn pose_at(banner: Banner, age: f32) -> (f32, f32, f32) {
    let faded = 1.0 - ((age - SHOWN_SECONDS) / FADE_SECONDS).clamp(0.0, 1.0);
    if banner.rampage() {
        let t = (age / RAMPAGE_SECONDS).clamp(0.0, 1.0);
        let scale = FIRST_BLOOD_SWELL + (1.0 - FIRST_BLOOD_SWELL) * t;
        let step = t / RAMPAGE_STEP;
        let still_from = (RAMPAGE_SWING.len() - 1) as f32 * RAMPAGE_STEP;
        let tilt = if t >= still_from {
            let last = RAMPAGE_SWING[RAMPAGE_SWING.len() - 1];
            last * (1.0 - (t - still_from) / (1.0 - still_from))
        } else {
            let at = step.floor() as usize;
            let from = RAMPAGE_SWING[at];
            let to = RAMPAGE_SWING[at + 1];
            from + (to - from) * step.fract()
        };
        return (scale, tilt, faded);
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

/// Ages the messages, poses them and fades their parts; takes the expired away.
fn pose(
    mut commands: Commands,
    time: Res<Time>,
    mut messages: Query<(Entity, &mut Message, &mut UiTransform, &Children)>,
    mut parts: Query<(&Ink, Option<&mut TextColor>, Option<&mut ImageNode>)>,
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
        for child in children {
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
    let rows: Vec<_> = messages
        .iter(world)
        .map(|(message, children)| (message.banner, children.to_vec()))
        .collect();
    rows.into_iter()
        .map(|(banner, children)| {
            let said: String = children
                .iter()
                .filter_map(|child| texts.get(world, *child).ok())
                .map(|text| text.0.as_str())
                .collect();
            (said, banner)
        })
        .collect()
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
}
