//! The Glyph of Fortification button, in the column against the minimap's right
//! side as in Dota (where Scan goes below it, which bota has not): it
//! shows whether one's side may activate the Glyph, the seconds until it
//! may, or the seconds its protection still holds, and a click or the Glyph
//! key activates it.

use bevy::prelude::*;

use crate::config::{Action, Settings, key_name};
use crate::hud::{Blocker, Docks};
use crate::session::{MatchKnown, Session};
use crate::ui::{self, Fonts};

/// The button.
#[derive(Component)]
struct GlyphButton;

/// The button's lines of text.
#[derive(Component)]
enum GlyphText {
    /// The name and the key.
    Name,
    /// Ready, the cooldown or the time left.
    State,
}

/// Logical pixels the button is wide.
const BUTTON_WIDTH: f32 = 74.0;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(Update, (show, click).run_if(|k: Res<MatchKnown>| k.0));
}

fn spawn(mut commands: Commands, fonts: Res<Fonts>, settings: Res<Settings>, docks: Res<Docks>) {
    let button = commands
        .spawn((
            GlyphButton,
            Button,
            Blocker,
            Node {
                width: px(BUTTON_WIDTH),
                padding: UiRect::axes(px(6), px(5)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::right(px(4)),
                ..default()
            },
            BackgroundColor(ui::PANEL),
            BorderColor::all(ui::EDGE),
            Visibility::Hidden,
            ChildOf(docks.minimap_buttons),
        ))
        .id();
    commands.spawn((
        GlyphText::Name,
        ui::text(
            format!("GLYPH [{}]", key_name(settings.key(Action::Glyph))),
            &fonts.bold,
            11.0,
            ui::INK_DIM,
        ),
        ChildOf(button),
    ));
    commands.spawn((
        GlyphText::State,
        ui::text("", &fonts.bold, 16.0, ui::GOLDEN),
        ChildOf(button),
    ));
}

/// Seconds as `m:ss`, rounded up.
fn clock(ticks: u32, rate: u32) -> String {
    let seconds = ticks.div_ceil(rate.max(1));
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The button, shown to a player while the match runs, and what it says.
fn show(
    session: NonSend<Session>,
    mut button: Query<(&mut Visibility, &mut BorderColor), With<GlyphButton>>,
    mut texts: Query<(&GlyphText, &mut Text, &mut TextColor)>,
) {
    let core = &session.0;
    let glyph = core.my_slot.and_then(|_| core.glyph());
    let rate = u32::from(core.tick_rate());
    let Ok((mut visibility, mut border)) = button.single_mut() else {
        return;
    };
    let Some(glyph) = glyph else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    visibility.set_if_neq(Visibility::Inherited);
    let (line, color, edge) = if glyph.active_for > 0 {
        (clock(glyph.active_for, rate), ui::HP_MINE, ui::HP_MINE)
    } else if glyph.ready_in > 0 {
        (clock(glyph.ready_in, rate), ui::INK_DIM, ui::EDGE)
    } else {
        ("READY".to_string(), ui::GOLDEN, ui::GOLDEN)
    };
    *border = BorderColor::all(edge);
    for (which, mut text, mut tint) in &mut texts {
        if let GlyphText::State = which {
            text.set_if_neq(Text(line.clone()));
            tint.set_if_neq(TextColor(color));
        }
    }
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
