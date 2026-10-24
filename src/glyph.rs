//! The Glyph of Fortification button, as Dota draws it: a round icon in the
//! frame against the minimap's right side (where Scan goes below it, which
//! bota has not), no words on it. Ready, it shows Dota's lit Glyph; waiting,
//! the dimmed one under a dark sweep that clears clockwise, with the seconds
//! left in its middle. A click or the Glyph key activates it.

use std::f32::consts::TAU;

use bevy::prelude::*;
use bota_core::rules::GLYPH_COOLDOWN_TICKS;

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
const BUTTON: f32 = 42.0;
const PAD: f32 = 7.0;
/// The stand-in's colours without the cache: lit gold, dimmed grey.
const READY: Color = Color::srgb(0.85, 0.68, 0.32);
const WAITING: Color = Color::srgb(0.32, 0.34, 0.38);

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(Update, (show, click).run_if(|k: Res<MatchKnown>| k.0));
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
            Interaction::default(),
            Blocker,
            Visibility::Hidden,
            ChildOf(docks.minimap_buttons),
        ))
        .id();
    let button = commands
        .spawn((
            GlyphButton,
            Button,
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

/// The button, shown to a player while the match runs: its icon, its sweep
/// and its seconds.
#[allow(clippy::type_complexity)]
fn show(
    session: NonSend<Session>,
    mut frame: Single<&mut Visibility, With<GlyphFrame>>,
    mut icon: Single<(&GlyphIcon, &mut ImageNode, &mut BackgroundColor), Without<GlyphSweep>>,
    mut sweep: Single<&mut BackgroundGradient, With<GlyphSweep>>,
    mut seconds: Single<&mut Text, With<GlyphSeconds>>,
) {
    let core = &session.0;
    let rate = u32::from(core.tick_rate()).max(1);
    // One's own side's Glyph, whose wait the view tells.
    let Some(ready_in) = core
        .my_slot
        .and_then(|_| core.glyph())
        .and_then(|glyph| glyph.ready_after)
    else {
        frame.set_if_neq(Visibility::Hidden);
        return;
    };
    frame.set_if_neq(Visibility::Inherited);
    let waiting = ready_in > 0;
    let (art, art_color, stand_in) = match (&icon.0.ready, &icon.0.waiting, waiting) {
        (Some(ready), _, false) => (ready.clone(), Color::WHITE, Color::NONE),
        (_, Some(dim), true) => (dim.clone(), Color::WHITE, Color::NONE),
        (_, _, waiting) => (
            Handle::default(),
            Color::NONE,
            if waiting { WAITING } else { READY },
        ),
    };
    let (_, image, background) = &mut *icon;
    if image.image != art {
        image.image = art;
    }
    if image.color != art_color {
        image.color = art_color;
    }
    background.set_if_neq(BackgroundColor(stand_in));
    // Dark over what is still to wait, clear over what has passed.
    let left = (ready_in as f32 / GLYPH_COOLDOWN_TICKS as f32).clamp(0.0, 1.0);
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
    mut session: NonSendMut<Session>,
    button: Query<&Interaction, (With<GlyphButton>, Changed<Interaction>)>,
) {
    if button.iter().any(|i| *i == Interaction::Pressed) {
        session.0.activate_glyph();
    }
}
