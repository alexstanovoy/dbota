//! The Glyph of Fortification sits in the lower well beside the minimap.
//! A player sees a charge badge and cooldown. Spectators of both sides see each
//! side’s cooldown sweep in its own half without a charge badge.
//! A click or the Glyph key activates it.

use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy::{
    picking::{events::PointerPress, hover::Hovered, pointer::PointerButton},
    ui_widgets::Button,
};
use bota_core::rules;
use bota_proto::{GlyphView, PlayerId, Team};

use crate::hud::Docks;
use crate::manifest::Manifest;
use crate::session::{MatchKnown, Session};
use crate::ui::{self, Fonts, dota, team_buttons};

#[derive(Resource)]
struct SpectatorGlyph(team_buttons::TeamFaces);

/// The button.
#[derive(Component)]
pub struct GlyphButton;

/// The button's icon: Dota's lit and dimmed Glyph, where the cache has them.
#[derive(Component)]
struct GlyphIcon {
    ready: Option<Handle<Image>>,
    waiting: Option<Handle<Image>>,
}

/// The dark sweep over the icon while the Glyph waits.
#[derive(Component)]
pub(crate) struct GlyphSweep;

#[derive(Component)]
struct GlyphBadge {
    label: Entity,
    drawn: Option<u8>,
}

/// The seconds left before the Glyph can be used.
#[derive(Component)]
pub(crate) struct GlyphSeconds;

/// Dota's pixels across the button, how far the plate's wells stand from its left edge, and how far
/// the Glyph's and the Scan's wells stand from its foot (`#glyph`, `#RadarButton`).
pub(crate) const BUTTON: f32 = 44.0;
pub(crate) const PLATE_LEFT: f32 = 6.0;
const GLYPH_BOTTOM: f32 = 6.0;
pub(crate) const SCAN_BOTTOM: f32 = 50.0;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_observer(click)
        .add_systems(Update, updates());
}

pub(crate) fn updates() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    (show, show_charges, show_teams).run_if(|k: Res<MatchKnown>| k.0)
}

fn spawn(
    mut commands: Commands,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    docks: Res<Docks>,
) {
    // On the plate, in the lower of its two wells (`#glyph`: 44 of Dota's pixels across, 6 from the
    // plate's left and foot).
    let frame = commands
        .spawn((
            Name::new("hud glyph well"),
            Node {
                position_type: PositionType::Absolute,
                left: dota(PLATE_LEFT),
                bottom: dota(GLYPH_BOTTOM),
                ..default()
            },
            ChildOf(docks.minimap_buttons),
        ))
        .id();
    let button = commands
        .spawn((
            GlyphButton,
            Name::new("hud glyph"),
            Button,
            Hovered::default(),
            bevy::picking::hover::DirectlyHovered::default(),
            Node {
                width: dota(BUTTON),
                height: dota(BUTTON),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(ui::PANEL_DEEP),
            Visibility::Hidden,
            ChildOf(frame),
        ))
        .id();
    let load = |name: &str| manifest.file(name).map(|path| server.load(path));
    let round = Node {
        border_radius: BorderRadius::MAX,
        ..ui::fill()
    };
    commands.spawn((
        GlyphIcon {
            ready: load("hud.glyph_on"),
            waiting: load("hud.glyph_off"),
        },
        round.clone(),
        ImageNode::default().with_color(Color::NONE),
        BackgroundColor(Color::NONE),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    commands.spawn((
        GlyphSweep,
        round,
        ui::split_disc(TAU, Color::NONE, Color::NONE),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    commands.spawn((
        GlyphSeconds,
        Name::new("hud glyph seconds"),
        ui::text("", &fonts.bold, 16.0, ui::INK),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    let (badge, label) = ui::spawn_charge_badge(&mut commands, &fonts, button);
    commands.entity(badge).insert((
        Name::new("hud glyph charges"),
        GlyphBadge { label, drawn: None },
        Visibility::Hidden,
    ));
    let teams = team_buttons::TeamFaces::spawn(
        &mut commands,
        frame,
        team_buttons::Ability::Glyph,
        [load("hud.glyph_radiant"), load("hud.glyph_dire")],
        load("hud.glyph_off"),
    );
    commands.insert_resource(SpectatorGlyph(teams));
}

fn show_charges(
    session: NonSend<Session>,
    mut badge: Single<(&mut GlyphBadge, &mut Visibility)>,
    mut labels: Query<&mut Text>,
) {
    let core = &session.0;
    let charges = displayed_glyph(core)
        .and_then(|glyph| glyph.charges)
        .filter(|charges| *charges > 0);
    let (badge, visibility) = &mut *badge;
    visibility.set_if_neq(ui::visible(charges.is_some()));
    if let Some(charges) = charges
        && badge.drawn != Some(charges)
        && let Ok(mut label) = labels.get_mut(badge.label)
    {
        badge.drawn = Some(charges);
        label.0 = charges.to_string();
    }
}

fn show_teams(
    session: NonSend<Session>,
    mut faces: ResMut<SpectatorGlyph>,
    mut paint: team_buttons::Paint,
) {
    let core = &session.0;
    let states = core
        .view()
        .filter(|_| team_buttons::all_teams(core))
        .and_then(|view| {
            let schedule = rules::Schedule::of(core.pregame_ticks());
            let state = |team| {
                let glyph = view.glyphs.iter().find(|glyph| glyph.team == team)?;
                let wait = glyph.ready_after?;
                let left = sweep_left(wait, schedule.glyph_wait(view.game_tick(), glyph.charges?));
                Some(team_buttons::FaceState::new(wait > 0, left))
            };
            Some([state(Team::Radiant)?, state(Team::Dire)?])
        });
    faces.0.paint(states, &mut paint);
}

fn displayed_glyph(core: &bota_client_core::App) -> Option<GlyphView> {
    if team_buttons::all_teams(core) {
        return None;
    }
    let view = core.view()?;
    if core.my_slot.is_none()
        && let Some(team) = view.viewer
    {
        return view.glyphs.iter().find(|glyph| glyph.team == team).copied();
    }
    core.glyph()
}

/// The share of a wait of `wait` ticks still to go with `ready_in` left: what the dark sweep
/// covers.
fn sweep_left(ready_in: u32, wait: u32) -> f32 {
    (ready_in as f32 / wait.max(1) as f32).clamp(0.0, 1.0)
}

/// What the button last showed, so that nothing is built for a frame that changes nothing.
#[derive(Default)]
pub(crate) struct Shown {
    /// The icon was the ready one; the seconds under it; the sweep's angle in half degrees and its
    /// darkness in hundredths.
    waiting: Option<bool>,
    seconds: Option<u32>,
    sweep: Option<(u16, u8)>,
    recharge: Recharge,
}

#[derive(Default)]
struct Recharge {
    last: Option<GlyphObservation>,
    origin: Option<u32>,
}

#[derive(Clone, Copy)]
struct GlyphObservation {
    game: u32,
    connection: Option<PlayerId>,
    team: Team,
    tick: u32,
    charges: u8,
}

impl Recharge {
    /// Start a recharge when this client first sees the charge arrive or a full Glyph spent.
    /// A snapshot may skip over the actual transition, so the sweep starts at the observed tick.
    fn observe(&mut self, now: GlyphObservation) -> Option<u32> {
        let previous = self.last.replace(now).filter(|last| {
            last.game == now.game
                && last.connection == now.connection
                && last.team == now.team
                && last.tick <= now.tick
        });
        self.origin = match previous {
            Some(last)
                if now.charges == 1 && matches!(last.charges, 0 | rules::GLYPH_MAX_CHARGES) =>
            {
                Some(now.tick)
            }
            Some(_) if now.charges < rules::GLYPH_MAX_CHARGES => self.origin,
            _ => None,
        };
        self.origin
    }
}

/// The icon and its timer follow the side the HUD is about.
#[allow(clippy::type_complexity)]
fn show(
    session: NonSend<Session>,
    game: Res<crate::main_menu::GameEpoch>,
    mut shown: Local<Shown>,
    mut frame: Single<&mut Visibility, With<GlyphButton>>,
    mut icon: Single<(&GlyphIcon, &mut ImageNode), Without<GlyphSweep>>,
    mut sweep: Single<&mut BackgroundGradient, With<GlyphSweep>>,
    mut seconds: Single<(&mut Text, &mut Node), With<GlyphSeconds>>,
) {
    let core = &session.0;
    let rate = u32::from(core.tick_rate()).max(1);
    // The Glyph of the side the HUD is about, whose wait and charges the view tells.
    let (
        Some(view),
        Some(GlyphView {
            team,
            ready_after: Some(ready_in),
            charges: Some(charges),
            ..
        }),
    ) = (core.view(), displayed_glyph(core))
    else {
        shown.recharge = Recharge::default();
        frame.set_if_neq(Visibility::Hidden);
        return;
    };
    frame.set_if_neq(Visibility::Inherited);
    let clock = view.game_tick();
    let origin = if core.lost || core.source.is_redialing() {
        shown.recharge = Recharge::default();
        None
    } else {
        shown.recharge.observe(GlyphObservation {
            game: game.0,
            connection: core.lobby.me,
            team,
            tick: view.tick,
            charges,
        })
    };
    let waiting = ready_in > 0;
    if shown.waiting != Some(waiting) {
        shown.waiting = Some(waiting);
        let (art, art_color) = match (&icon.0.ready, &icon.0.waiting, waiting) {
            (Some(ready), _, false) => (ready.clone(), Color::WHITE),
            (_, Some(dim), true) => (dim.clone(), Color::WHITE),
            _ => (Handle::default(), Color::NONE),
        };
        let (_, image) = &mut *icon;
        image.image = art;
        image.color = art_color;
    }
    let (left, alpha) = if waiting {
        let wait = rules::Schedule::of(core.pregame_ticks()).glyph_wait(clock, charges);
        (sweep_left(ready_in, wait), 0.68)
    } else if let Some(from) = origin {
        let left = rules::GLYPH_RECHARGE_TICKS.saturating_sub(view.tick.saturating_sub(from));
        (
            sweep_left(left, rules::GLYPH_RECHARGE_TICKS),
            if left > 0 { 0.45 } else { 0.0 },
        )
    } else {
        (0.0, 0.0)
    };
    let angle = ((1.0 - left) * 720.0) as u16;
    let key = (angle, (alpha * 100.0) as u8);
    if shown.sweep != Some(key) {
        shown.sweep = Some(key);
        **sweep = ui::split_disc(
            f32::from(angle) / 720.0 * TAU,
            Color::NONE,
            Color::srgba(0.0, 0.0, 0.0, alpha),
        );
    }
    let secs = if waiting { ready_in.div_ceil(rate) } else { 0 };
    let (text, node) = &mut *seconds;
    let bottom = if waiting && charges > 0 {
        dota(20.0)
    } else {
        px(0)
    };
    if node.margin.bottom != bottom {
        node.margin.bottom = bottom;
    }
    if shown.seconds != Some(secs) {
        shown.seconds = Some(secs);
        text.0 = if secs > 0 {
            secs.to_string()
        } else {
            String::new()
        };
    }
}

/// A click on the button activates the Glyph.
fn click(
    event: On<PointerPress>,
    mut session: NonSendMut<Session>,
    button: Query<(), With<GlyphButton>>,
) {
    if event.button == PointerButton::Primary && button.contains(event.entity) {
        session.0.activate_glyph();
    }
}

/// The installed tooltip of the player's Glyph.
pub fn tip(texts: &crate::dota_text::Texts, core: &bota_client_core::App) -> Option<String> {
    tip_of(texts, displayed_glyph(core)?, core.tick_rate(), None)
}

pub(crate) fn team_tip(
    texts: &crate::dota_text::Texts,
    core: &bota_client_core::App,
    team: Team,
) -> Option<String> {
    let glyph = core
        .view()?
        .glyphs
        .iter()
        .find(|glyph| glyph.team == team)
        .copied()?;
    tip_of(texts, glyph, core.tick_rate(), Some(team))
}

fn tip_of(
    texts: &crate::dota_text::Texts,
    glyph: GlyphView,
    rate: u16,
    team: Option<Team>,
) -> Option<String> {
    let ready_in = glyph.ready_after?;
    let charges = glyph.charges?;
    let mut title = texts.label("DOTA_Glossary_Map_Structures_Glyph", &[])?;
    let lines = [
        team.and_then(team_buttons::team_label)
            .and_then(|key| texts.label(key, &[])),
        texts.label("DOTA_Glossary_Map_Structures_Glyph_Desc", &[]),
        texts.label(
            "DOTA_HUD_GlyphCharges",
            &[("charges_available", charges.to_string())],
        ),
        (ready_in > 0)
            .then(|| {
                let seconds = ready_in.div_ceil(u32::from(rate.max(1))).to_string();
                if charges > 0
                    && let Some(line) = texts.label(
                        "DOTA_HUD_GlyphUseCooldown",
                        &[("use_cooldown_time", seconds.clone())],
                    )
                {
                    return Some(line);
                }
                texts.label("DOTA_HUD_GlyphCooldown", &[("cooldown_time", seconds)])
            })
            .flatten(),
    ];
    for line in lines.into_iter().flatten() {
        title.push('\n');
        title.push_str(&line);
    }
    Some(title)
}

#[cfg(test)]
mod tests {
    use super::{GlyphObservation, Recharge, Team, sweep_left};
    use bota_core::rules::{self, Schedule};

    fn observation(tick: u32, charges: u8) -> GlyphObservation {
        GlyphObservation {
            game: 1,
            connection: None,
            team: Team::Radiant,
            tick,
            charges,
        }
    }

    #[test]
    fn a_charge_arrival_across_skipped_ticks_starts_the_sweep_at_the_observed_tick() {
        let mut recharge = Recharge::default();
        assert_eq!(recharge.observe(observation(10, 0)), None);
        assert_eq!(recharge.observe(observation(15, 1)), Some(15));
        assert_eq!(recharge.observe(observation(16, 1)), Some(15));
    }

    #[test]
    fn seeing_an_empty_glyph_gain_a_charge_starts_the_sweep() {
        let mut recharge = Recharge::default();
        assert_eq!(recharge.observe(observation(10, 0)), None);
        assert_eq!(recharge.observe(observation(11, 1)), Some(11));
    }

    #[test]
    fn a_glyph_spent_between_observations_starts_the_sweep_at_the_observed_tick() {
        let mut recharge = Recharge::default();
        assert_eq!(recharge.observe(observation(10, 2)), None);
        assert_eq!(recharge.observe(observation(15, 1)), Some(15));
    }

    /// Before the first charge the sweep runs over the first wait (270 s from the pregame's
    /// start on Dota's pregame), not a later charge's 360 s: full at the clock's start, half
    /// way at its middle.
    #[test]
    fn the_sweep_runs_over_the_first_charges_own_wait() {
        let schedule = Schedule::of(rules::DOTA_PREGAME_TICKS);
        let first = schedule.glyph_first_use();
        assert_eq!(first, 270 * rules::TICKS_PER_SECOND);
        let at = |clock: u32| sweep_left(first - clock, schedule.glyph_wait(clock, 0));
        assert_eq!(at(0), 1.0);
        assert_eq!(at(first / 2), 0.5);
        let later = first + 10;
        assert_eq!(
            sweep_left(
                rules::GLYPH_RECHARGE_TICKS / 2,
                schedule.glyph_wait(later, 0)
            ),
            0.5,
            "a later charge's six minutes"
        );
        assert_eq!(
            sweep_left(
                rules::GLYPH_USE_COOLDOWN_TICKS,
                schedule.glyph_wait(later, 1)
            ),
            1.0,
            "the lock's seven seconds"
        );
    }
}
