//! The tally under the menu's buttons in the top-left corner, as Dota's HUD keeps it: one's own
//! kills, deaths and assists on a line, last hits and denies on the next, on a plate that fades to
//! the right. It tells the seat the client plays (or watches through), whoever is picked.

use bevy::prelude::*;

use crate::session::{MatchKnown, Session};
use crate::ui::{self, Fonts};

/// The plate's place and size, in interface pixels.
const PLATE_TOP: f32 = 66.5;
const PLATE: (f32, f32) = (170.0, 44.5);
/// The room left of the lines, and the rows' size.
const INSET: f32 = 7.5;
const FONT: f32 = 17.5;
/// What the plate is made of: dark at the screen's edge, clear at the plate's.
const PLATE_START: Color = Color::srgba(0.0, 0.0, 0.0, 0.42);
const PLATE_END: Color = Color::srgba(0.0, 0.0, 0.0, 0.0);
/// The labels' bluish grey.
const LABEL: Color = Color::srgb(0.72, 0.78, 0.86);

/// The plate.
#[derive(Component)]
struct Tally;

/// What a line of the plate says: the figures it keeps up to date.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Figures {
    Kills,
    LastHits,
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, show.run_if(|known: Res<MatchKnown>| known.0));
}

/// The plate, hidden until the match tells one's seat.
pub fn spawn(commands: &mut Commands, fonts: &Fonts, root: Entity) {
    let plate = commands
        .spawn((
            Tally,
            Name::new("hud tally"),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(PLATE_TOP),
                width: px(PLATE.0),
                height: px(PLATE.1),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                padding: UiRect::left(px(INSET)),
                ..default()
            },
            BackgroundGradient::from(LinearGradient::to_right(vec![
                ColorStop::new(PLATE_START, percent(0)),
                ColorStop::new(PLATE_END, percent(100)),
            ])),
            Visibility::Hidden,
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    for (label, figures) in [
        ("K / D / A", Figures::Kills),
        ("LH / DN", Figures::LastHits),
    ] {
        let line = commands
            .spawn((
                Node {
                    column_gap: px(14),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(plate),
            ))
            .id();
        commands.spawn((
            ui::text(label, &fonts.regular, FONT, LABEL),
            ui::shadow(),
            Node {
                width: px(78),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(line),
        ));
        commands.spawn((
            figures,
            ui::text("0 / 0", &fonts.bold, FONT, ui::INK),
            ui::shadow(),
            Pickable::IGNORE,
            ChildOf(line),
        ));
    }
}

/// The figures of one's own seat; the plate is hidden for a client with none (a spectator of no
/// seat) and for a seat the match has not told.
fn show(
    session: NonSend<Session>,
    mut plate: Single<&mut Visibility, With<Tally>>,
    mut lines: Query<(&Figures, &mut Text)>,
) {
    let core = &session.0;
    let player = core.my_slot.and_then(|slot| core.view()?.player(slot));
    plate.set_if_neq(ui::visible(player.is_some()));
    let Some(player) = player else {
        return;
    };
    for (figures, mut text) in &mut lines {
        let said = match figures {
            Figures::Kills => format!("{} / {} / {}", player.kills, player.deaths, player.assists),
            Figures::LastHits => {
                let (last_hits, denies) = player
                    .creep_score
                    .map_or((0, 0), |score| (score.last_hits, score.denies));
                format!("{last_hits} / {denies}")
            }
        };
        text.set_if_neq(Text(said));
    }
}
