//! The Glyph of Fortification button, as Dota draws it: a round icon in the
//! frame against the minimap's right side (where Scan goes below it, which
//! bota has not), no words on it. Ready, it shows Dota's lit Glyph; waiting,
//! the dimmed one under a dark sweep that clears clockwise, with the seconds
//! left in its middle. A click or the Glyph key activates it.

use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy::{
    picking::{events::PointerPress, hover::Hovered, pointer::PointerButton},
    ui_widgets::Button,
};
use bota_core::rules::Schedule;

use crate::hud::{Blocker, Docks};
use crate::manifest::Manifest;
use crate::session::{MatchKnown, Session};
use crate::ui::{self, Fonts};

/// The frame the button sits in.
#[derive(Component)]
struct GlyphFrame;

/// The button.
#[derive(Component)]
struct GlyphButton;

/// The button's icon: Dota's lit and dimmed Glyph, where the cache has them.
#[derive(Component)]
struct GlyphIcon {
    ready: Option<Handle<Image>>,
    waiting: Option<Handle<Image>>,
}

/// The dark sweep over the icon while the Glyph waits.
#[derive(Component)]
struct GlyphSweep;

/// The seconds left, in the middle.
#[derive(Component)]
struct GlyphSeconds;

/// Logical pixels across the button, and the frame's padding round it.
const BUTTON: f32 = 55.0;
const PAD: f32 = 7.0;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_observer(click)
        .add_systems(Update, show.run_if(|k: Res<MatchKnown>| k.0));
}

fn spawn(
    mut commands: Commands,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    docks: Res<Docks>,
) {
    let frame = commands
        .spawn((
            GlyphFrame,
            Node {
                padding: UiRect::all(px(PAD)),
                border: UiRect::new(px(0), px(2), px(2), px(2)),
                border_radius: BorderRadius::right(px(10)),
                ..default()
            },
            BackgroundColor(ui::PANEL),
            BorderColor::all(ui::EDGE),
            Hovered::default(),
            Blocker,
            Visibility::Hidden,
            ChildOf(docks.minimap_buttons),
        ))
        .id();
    let button = commands
        .spawn((
            GlyphButton,
            Name::new("hud glyph"),
            Button,
            Hovered::default(),
            Node {
                width: px(BUTTON),
                height: px(BUTTON),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(ui::PANEL_DEEP),
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
        ui::text("", &fonts.bold, 16.0, ui::INK),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

/// The share of a wait of `wait` ticks still to go with `ready_in` left: what the dark sweep
/// covers.
fn sweep_left(ready_in: u32, wait: u32) -> f32 {
    (ready_in as f32 / wait.max(1) as f32).clamp(0.0, 1.0)
}

/// The button, shown to a player while the match runs: its icon, its sweep
/// and its seconds.
#[allow(clippy::type_complexity)]
fn show(
    session: NonSend<Session>,
    mut frame: Single<&mut Visibility, With<GlyphFrame>>,
    mut icon: Single<(&GlyphIcon, &mut ImageNode), Without<GlyphSweep>>,
    mut sweep: Single<&mut BackgroundGradient, With<GlyphSweep>>,
    mut seconds: Single<&mut Text, With<GlyphSeconds>>,
) {
    let core = &session.0;
    let rate = u32::from(core.tick_rate()).max(1);
    // One's own side's Glyph, whose wait and charges the view tells.
    let Some((ready_in, charges)) = core
        .my_slot
        .and_then(|_| core.glyph())
        .and_then(|glyph| glyph.ready_after.zip(glyph.charges))
    else {
        frame.set_if_neq(Visibility::Hidden);
        return;
    };
    frame.set_if_neq(Visibility::Inherited);
    let waiting = ready_in > 0;
    let (art, art_color) = match (&icon.0.ready, &icon.0.waiting, waiting) {
        (Some(ready), _, false) => (ready.clone(), Color::WHITE),
        (_, Some(dim), true) => (dim.clone(), Color::WHITE),
        _ => (Handle::default(), Color::NONE),
    };
    let (_, image) = &mut *icon;
    if image.image != art {
        image.image = art;
    }
    if image.color != art_color {
        image.color = art_color;
    }
    // Dark over what is still to wait, clear over what has passed.
    // Over the whole of the wait it is in (since 7.42: the first charge's from the clock's
    // start, a later charge's six minutes, or the seven seconds between two uses).
    let clock = core.view().map_or(0, |view| view.game_tick());
    let wait = Schedule::of(core.pregame_ticks()).glyph_wait(clock, charges);
    let left = sweep_left(ready_in, wait);
    let dark = Color::srgba(0.0, 0.0, 0.0, if waiting { 0.68 } else { 0.0 });
    let wanted = ui::split_disc((1.0 - left) * TAU, Color::NONE, dark);
    if **sweep != wanted {
        **sweep = wanted;
    }
    let text = if waiting {
        ready_in.div_ceil(rate).to_string()
    } else {
        String::new()
    };
    seconds.set_if_neq(Text(text));
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

#[cfg(test)]
mod tests {
    use super::sweep_left;
    use bota_core::rules::{self, Schedule};

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
