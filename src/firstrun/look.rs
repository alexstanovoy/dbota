//! How the first-run screen looks: one panel in the middle of a dark ground, in the menu's own
//! colours, faces and buttons (`theme`, `main_menu::kit`), so that the first screen and the menu
//! read as one product. Before any copy exists there is none of Dota's art or typefaces, only what
//! dbota ships: the panel holds the one-line pitch, the requirement, what was found (a path the
//! eye can skip), what went wrong and what to do, the work under way with its bar, the screen's
//! main action and quieter ones, and the privacy promise last. Text is larger than the menu's: it
//! is read once, closely, by someone who has just started dbota.

use std::fmt::Write as _;

use bevy::{picking::hover::Hovered, prelude::*, ui_widgets::Button};

use super::{FirstRun, Note, Press, Screen, Shown, Work, shown};
use crate::main_menu::kit::Shade;
use crate::main_menu::kit::Tone;
use crate::theme::{self, Face, Type};
use crate::ui::{self, Fonts};

/// The screen.
#[derive(Component)]
pub(super) struct Root;

/// The parts of the work under way that change as it goes, set in place (`bar`).
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Live {
    /// "N of M".
    Count,
    /// The bar's fill.
    Fill,
    /// What is read now.
    Item,
    /// How much is written and left.
    Left,
}

const PANEL_WIDTH: f32 = 860.0;
const TITLE: Type = Type {
    face: Face::Display,
    size: 40.0,
    tracking: 1.5,
    figures: false,
};
const WORDMARK: Type = Type {
    face: Face::Display,
    size: 20.0,
    tracking: 6.0,
    figures: false,
};
const PITCH: Type = Type {
    face: Face::Regular,
    size: 23.0,
    tracking: 0.0,
    figures: false,
};
const LEAD: Type = Type {
    face: Face::Regular,
    size: 21.0,
    tracking: 0.0,
    figures: false,
};
const HEADING: Type = Type {
    face: Face::Semibold,
    size: 16.0,
    tracking: 2.5,
    figures: false,
};
const LINE: Type = Type {
    face: Face::Regular,
    size: 19.0,
    tracking: 0.0,
    figures: false,
};
const SMALL: Type = Type {
    face: Face::Regular,
    size: 17.0,
    tracking: 0.0,
    figures: false,
};
const COUNT: Type = Type {
    face: Face::Semibold,
    size: 17.0,
    tracking: 0.0,
    figures: true,
};
const BUTTON_HEIGHT: f32 = 52.0;
const BUTTON: Type = Type {
    face: Face::Bold,
    size: 19.0,
    tracking: 2.0,
    figures: false,
};

/// Draws the screen anew when what it shows changes (the work's figures aside).
pub(super) fn draw(
    mut commands: Commands,
    run: Res<FirstRun>,
    fonts: Res<Fonts>,
    root: Query<Entity, With<Root>>,
    mut drawn: Local<Option<Shown>>,
) {
    if !run.is_changed() {
        return;
    }
    if matches!(run.screen, Screen::Working(_))
        && drawn
            .as_ref()
            .is_some_and(|shown| shown.working && shown.note == run.note)
    {
        return;
    }
    let now = shown(&run);
    if drawn.as_ref() == Some(&now) {
        return;
    }
    info!(
        "first run: {} | {}{}",
        screen_name(&run.screen),
        now.state
            .as_ref()
            .map_or("", |(heading, _)| heading.as_str()),
        now.note
            .as_ref()
            .map_or(String::new(), |n| format!(" | note: {}", n.text()))
    );
    for entity in &root {
        commands.entity(entity).despawn();
    }
    let mut parts = Parts {
        commands: &mut commands,
        fonts: &fonts,
    };
    parts.screen(&now, now.working);
    *drawn = Some(now);
}

/// The screen's state by name, for the log.
fn screen_name(screen: &Screen) -> &'static str {
    match screen {
        Screen::Looking => "looking",
        Screen::Ready => "ready",
        Screen::Choosing => "choosing",
        Screen::Typing => "typing",
        Screen::Working(_) => "working",
        Screen::Waiting => "waiting",
        Screen::Stopping => "stopping",
        Screen::Partial { .. } => "partial",
        Screen::Restarting => "restarting",
    }
}

/// The work's figures, set in place each frame they change: the count, the bar, what is read and
/// how much is left.
pub(super) fn bar(
    run: Res<FirstRun>,
    mut texts: Query<(&Live, &mut Text)>,
    mut fills: Query<(&Live, &mut Node), Without<Text>>,
) {
    if !run.is_changed() {
        return;
    }
    let Screen::Working(work) = &run.screen else {
        return;
    };
    for (live, mut text) in &mut texts {
        figures(&mut text.0, work, *live);
    }
    let share = work.done as f32 / work.total.max(1) as f32;
    for (_, mut node) in fills.iter_mut().filter(|(live, _)| **live == Live::Fill) {
        let width = percent(share.clamp(0.0, 1.0) * 100.0);
        if node.width != width {
            node.width = width;
        }
    }
}

/// The words of a figure of the work.
fn figures(text: &mut String, work: &Work, live: Live) {
    text.clear();
    match (live, work.total) {
        (Live::Count, 0) | (Live::Left, 0) | (Live::Fill, _) => {}
        (Live::Count, total) => {
            write!(text, "{} of {total}", work.done).expect("formatting into a String")
        }
        (Live::Item, 0) => text.push_str("Reading the files..."),
        (Live::Item, _) => {
            let group = work.item.split(['.', '/']).next().unwrap_or("");
            write!(text, "Reading {group}: {}", work.item).expect("formatting into a String");
        }
        (Live::Left, total) => write!(
            text,
            "{} MB written  ·  {} left",
            work.bytes >> 20,
            total.saturating_sub(work.done)
        )
        .expect("formatting into a String"),
    }
}

/// What the screen is built with.
struct Parts<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    fonts: &'a Fonts,
}

impl Parts<'_, '_, '_> {
    fn text(&mut self, parent: Entity, words: impl Into<String>, role: Type, ink: Color) -> Entity {
        self.commands
            .spawn((
                theme::text(words, self.fonts, role, ink),
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id()
    }

    fn node(&mut self, parent: Entity, node: Node) -> Entity {
        self.commands
            .spawn((node, Pickable::IGNORE, ChildOf(parent)))
            .id()
    }

    /// The ground, the panel and what it holds.
    fn screen(&mut self, shown: &Shown, working: bool) {
        let ground = self
            .commands
            .spawn((
                Root,
                crate::hud::Blocker,
                Hovered::default(),
                Node {
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    padding: UiRect::all(px(24)),
                    ..ui::fill()
                },
                // Opaque: nothing of what it holds shows through.
                BackgroundColor(theme::GROUND),
                ground_light(),
                GlobalZIndex(90),
            ))
            .id();
        let panel = self
            .commands
            .spawn((
                Node {
                    width: px(PANEL_WIDTH),
                    max_width: percent(100),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(52), px(44)),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                theme::downward(theme::PANEL),
                BorderColor {
                    top: theme::PANEL_GLINT,
                    ..BorderColor::all(theme::PANEL_EDGE)
                },
                theme::window_shadow(),
                Pickable::IGNORE,
                ChildOf(ground),
            ))
            .id();
        self.heading(panel, shown);
        if let Some((heading, line)) = &shown.state {
            self.state(panel, heading, line, shown.path.as_deref());
        }
        if working {
            self.work(panel, shown.path.as_deref());
        }
        if let Some(note) = &shown.note {
            self.note(panel, note);
        }
        self.buttons(panel, &shown.buttons);
        let rule = self.node(
            panel,
            Node {
                height: px(1),
                margin: UiRect::new(px(0), px(0), px(30), px(16)),
                ..default()
            },
        );
        self.commands
            .entity(rule)
            .insert(BackgroundColor(theme::DIVIDER));
        self.text(panel, shown.privacy, SMALL, theme::INK_5);
    }

    /// The wordmark, the title, the pitch and the requirement.
    fn heading(&mut self, panel: Entity, shown: &Shown) {
        let mark = self.text(panel, "DBOTA", WORDMARK, theme::INK_4);
        self.commands.entity(mark).insert(Node {
            margin: UiRect::bottom(px(18)),
            ..default()
        });
        let title = self.text(panel, shown.title.clone(), TITLE, theme::INK_1);
        self.commands.entity(title).insert(Node {
            margin: UiRect::bottom(px(16)),
            ..default()
        });
        if let Some(pitch) = shown.pitch {
            let pitch = self.text(panel, pitch, PITCH, theme::INK_3);
            self.commands.entity(pitch).insert(Node {
                margin: UiRect::bottom(px(20)),
                ..default()
            });
        }
        let lead = self.node(
            panel,
            Node {
                border: UiRect::left(px(3)),
                padding: UiRect::left(px(16)),
                margin: UiRect::bottom(px(28)),
                ..default()
            },
        );
        self.commands
            .entity(lead)
            .insert(BorderColor::all(theme::GOLD));
        self.text(lead, shown.lead.clone(), LEAD, theme::INK_2);
    }

    /// What the screen waits for or found: a heading in capitals, its line, and a path.
    fn state(&mut self, panel: Entity, heading: &str, line: &str, path: Option<&str>) {
        let block = self.node(
            panel,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                margin: UiRect::bottom(px(24)),
                ..default()
            },
        );
        let ink = if heading == super::side::FOUND {
            theme::RADIANT_TEXT
        } else {
            theme::GOLD
        };
        self.text(block, heading, HEADING, ink);
        if !line.is_empty() {
            self.text(block, line, LINE, theme::INK_3);
        }
        if let Some(path) = path {
            self.text(block, path, SMALL, theme::INK_4);
        }
    }

    /// The work under way: its heading and count, the bar, what is read and how much is left.
    fn work(&mut self, panel: Entity, path: Option<&str>) {
        let block = self.node(
            panel,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                margin: UiRect::bottom(px(24)),
                ..default()
            },
        );
        let top = self.node(
            block,
            Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Baseline,
                ..default()
            },
        );
        self.text(top, "MAKING YOUR PRIVATE COPY", HEADING, theme::GOLD);
        let count = self.text(top, String::with_capacity(64), COUNT, theme::INK_2);
        self.commands.entity(count).insert(Live::Count);
        let track = self.node(
            block,
            Node {
                height: px(10),
                overflow: Overflow::clip(),
                ..default()
            },
        );
        self.commands
            .entity(track)
            .insert(BackgroundColor(theme::LOADING_TRACK));
        let fill = self.node(
            track,
            Node {
                width: percent(0),
                height: percent(100),
                ..default()
            },
        );
        self.commands
            .entity(fill)
            .insert((Live::Fill, theme::rightward((theme::GO.1, theme::GO.0))));
        let below = self.node(
            block,
            Node {
                justify_content: JustifyContent::SpaceBetween,
                column_gap: px(24),
                ..default()
            },
        );
        let item = self.text(below, String::with_capacity(512), SMALL, theme::INK_3);
        self.commands.entity(item).insert((
            Live::Item,
            Node {
                flex_shrink: 1.0,
                ..default()
            },
        ));
        let left = self.text(below, String::with_capacity(64), SMALL, theme::INK_4);
        self.commands.entity(left).insert((
            Live::Left,
            TextLayout {
                linebreak: bevy::text::LineBreak::NoWrap,
                ..default()
            },
        ));
        if let Some(path) = path {
            self.text(block, format!("From {path}"), SMALL, theme::INK_5);
        }
    }

    /// What went wrong and what to do now, or what happened.
    fn note(&mut self, panel: Entity, note: &Note) {
        let (ground, edge, ink) = match note {
            Note::Trouble(_) => (theme::KEY_CLASH.0, theme::KEY_CLASH.1, theme::ERROR_TEXT),
            Note::Info(_) => (theme::CARD, theme::FOCUS, theme::INK_2),
        };
        let block = self.node(
            panel,
            Node {
                border: UiRect::left(px(3)),
                padding: UiRect::axes(px(16), px(12)),
                margin: UiRect::bottom(px(24)),
                ..default()
            },
        );
        self.commands
            .entity(block)
            .insert((BackgroundColor(ground), BorderColor::all(edge)));
        self.text(block, note.text(), LINE, ink);
    }

    /// The buttons: the first is the screen's main action, the others quieter, QUIT a word at
    /// the far end.
    fn buttons(&mut self, panel: Entity, buttons: &[(&'static str, Press)]) {
        let row = self.node(
            panel,
            Node {
                column_gap: px(12),
                row_gap: px(12),
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                ..default()
            },
        );
        for (at, (label, press)) in buttons.iter().enumerate() {
            if *press == Press::Quit {
                let spacer = self.node(row, Node::default());
                self.commands.entity(spacer).insert(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                self.quit(row, label);
                continue;
            }
            let tone = match (at, press) {
                (_, Press::Cancel | Press::Back | Press::Keep) => Tone::Secondary,
                (0, _) => Tone::Primary,
                _ => Tone::Secondary,
            };
            self.button(row, label, *press, tone);
        }
    }

    /// A button of the menu's shape: its gradient, edge, and light under the cursor.
    fn button(&mut self, row: Entity, label: &str, press: Press, tone: Tone) {
        let shade = Shade::of(tone);
        let button = self
            .commands
            .spawn((
                press,
                Button,
                Hovered::default(),
                shade,
                Node {
                    height: px(BUTTON_HEIGHT),
                    padding: UiRect::axes(px(28), px(0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(1)),
                    flex_shrink: 0.0,
                    ..default()
                },
                theme::downward(shade.rest()),
                shade.edge,
                Name::new(label.to_lowercase()),
                ChildOf(row),
            ))
            .id();
        let words = self.text(button, label, BUTTON, theme::INK_1);
        self.commands.entity(words).insert(TextLayout {
            linebreak: bevy::text::LineBreak::NoWrap,
            ..default()
        });
    }

    /// QUIT as a word that brightens under the cursor; named `quit dbota` for scripts, which the
    /// menu's own `quit` must not be taken for.
    fn quit(&mut self, row: Entity, label: &str) {
        let button = self
            .commands
            .spawn((
                Press::Quit,
                Button,
                Hovered::default(),
                Node {
                    min_height: px(BUTTON_HEIGHT),
                    padding: UiRect::axes(px(12), px(0)),
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    ..default()
                },
                Name::new("quit dbota"),
                ChildOf(row),
            ))
            .id();
        let words = self.text(button, label, BUTTON, theme::INK_4);
        self.commands.entity(button).insert(crate::main_menu::Wash {
            target: words,
            rest: theme::INK_4,
            hover: theme::INK_1,
        });
    }
}

/// A faint light over the top of the ground, behind the panel.
fn ground_light() -> BackgroundGradient {
    BackgroundGradient::from(RadialGradient::new(
        UiPosition {
            anchor: Vec2::new(-0.5, -0.5),
            x: percent(50),
            y: percent(0),
        },
        RadialGradientShape::FarthestCorner,
        vec![
            ColorStop::new(Color::srgb_u8(0x16, 0x22, 0x2C), percent(0)),
            ColorStop::new(
                Color::srgb_u8(0x16, 0x22, 0x2C).with_alpha(0.0),
                percent(60),
            ),
        ],
    ))
}
