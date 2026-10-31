//! Dota's toast of a rune appearing at the river (`dota_hud_rune_spawn_toast`): for five
//! seconds, at the right of the screen, the arrow pointing to the top spot or the bottom one, the
//! rune's icon and "Rune Spawned Top" or "Rune Spawned Bottom" in the install's words, on the
//! backdrop of Dota's purchase toast. A Bounty Rune appearing in the jungle has none.

use bevy::prelude::*;
use bota_proto::{EventKind, RuneKind};

use crate::dota_text::Texts;
use crate::hud::HudRoot;
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// Toasts shown at once at most: the river's two Water Runes appear together.
const TOASTS: usize = 4;
/// Seconds a toast shows (the layout's `toast_duration_override`).
const SHOWN_SECS: f32 = 5.0;
/// Its width, padding, icons and words, in interface pixels (the toast's stylesheet).
const WIDTH: f32 = 272.0;
const PADDING: f32 = 8.0;
const ICON: f32 = 24.0;
const WORDS: f32 = 18.0;
/// Where the column of toasts stands, from the screen's right and bottom: over the shop's corner.
const RIGHT: f32 = 10.0;
const BOTTOM: f32 = 320.0;

/// A toast: the rune it tells of, whether it appeared at the bottom spot, and how long it has
/// shown; none while the toast is not in use.
#[derive(Component, Default, Debug, PartialEq)]
pub struct RuneToast(pub Option<(RuneKind, bool, f32)>);

/// The parts of a toast.
#[derive(Component, Clone, Copy)]
enum Part {
    Arrow,
    Rune,
    Words,
}

/// Which toast a part belongs to.
#[derive(Component)]
struct Of(Entity);

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(Update, (toast, dress).chain());
}

/// The column of toasts, all of them hidden.
fn spawn(
    mut commands: Commands,
    root: Single<Entity, With<HudRoot>>,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    texts: Res<Texts>,
) {
    let column = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(RIGHT),
                bottom: px(BOTTOM),
                flex_direction: FlexDirection::ColumnReverse,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(*root),
        ))
        .id();
    let image = |logical: &str| {
        manifest
            .file(logical)
            .map(|p| server.load::<Image>(p))
            .unwrap_or_default()
    };
    let words = texts.style("bluegreyTextBright").unwrap_or(ui::INK);
    for _ in 0..TOASTS {
        let toast = commands
            .spawn((
                RuneToast::default(),
                Node {
                    width: px(WIDTH),
                    padding: UiRect::all(px(PADDING)),
                    margin: UiRect::bottom(px(8)),
                    align_items: AlignItems::Center,
                    ..default()
                },
                ImageNode::new(image("hud.rune_toast.background")),
                BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 0.85)),
                Visibility::Hidden,
                Pickable::IGNORE,
                ChildOf(column),
            ))
            .id();
        let icon = |part: Part, margin: f32| {
            (
                part,
                Of(toast),
                Node {
                    width: px(ICON),
                    height: px(ICON),
                    margin: UiRect::right(px(margin)),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(toast),
            )
        };
        commands.spawn((
            icon(Part::Arrow, 4.0),
            ImageNode::new(image("hud.rune_toast.arrow")).with_color(words),
            UiTransform::default(),
        ));
        commands.spawn((
            icon(Part::Rune, 5.0),
            ImageNode::new(image("icon.minimap.sheet")),
        ));
        commands.spawn((
            Part::Words,
            Of(toast),
            ui::text("", &fonts.bold, WORDS, words),
            ui::shadow(),
            Pickable::IGNORE,
            ChildOf(toast),
        ));
    }
}

/// Starts a toast for each water or power rune the last pump told appeared, and ages the
/// toasts shown.
fn toast(
    session: NonSend<Session>,
    time: Res<Time>,
    mut toasts: Query<(&mut RuneToast, &mut Visibility)>,
) {
    for (mut toast, mut shown) in &mut toasts {
        if let Some((_, _, age)) = &mut toast.0 {
            *age += time.delta_secs();
            if *age >= SHOWN_SECS {
                toast.0 = None;
            }
        }
        shown.set_if_neq(ui::visible(toast.0.is_some()));
    }
    for event in &session.0.told {
        let EventKind::RuneSpawned { kind, at } = *event else {
            continue;
        };
        if kind == RuneKind::Bounty {
            continue;
        }
        // The bottom spot lies on the Radiant's side of the river: below the map's diagonal.
        let bottom = at.y.to_f32() < at.x.to_f32();
        // The oldest gives way when every toast shows.
        let free = toasts
            .iter_mut()
            .max_by(|a, b| age(&a.0).total_cmp(&age(&b.0)));
        if let Some((mut toast, mut shown)) = free {
            toast.0 = Some((kind, bottom, 0.0));
            shown.set_if_neq(Visibility::Inherited);
        }
    }
}

/// How long a toast has shown; a free one counts as the oldest.
fn age(toast: &RuneToast) -> f32 {
    toast.0.map_or(f32::INFINITY, |(_, _, age)| age)
}

/// Dresses each toast for its rune: the arrow turned to its spot, the rune's icon and the words.
fn dress(
    toasts: Query<&RuneToast, Changed<RuneToast>>,
    texts: Res<Texts>,
    mut parts: Query<(
        &Part,
        &Of,
        Option<&mut ImageNode>,
        Option<&mut UiTransform>,
        Option<&mut Text>,
    )>,
) {
    if toasts.is_empty() {
        return;
    }
    for (part, of, image, turn, text) in &mut parts {
        let Ok(RuneToast(Some((kind, bottom, _)))) = toasts.get(of.0) else {
            continue;
        };
        match (part, image, turn, text) {
            (Part::Arrow, _, Some(mut turn), _) => {
                let angle = if *bottom { std::f32::consts::PI } else { 0.0 };
                turn.set_if_neq(UiTransform::from_rotation(Rot2::radians(angle)));
            }
            (Part::Rune, Some(mut image), ..) => {
                let (x, y) = crate::runes::sheet_cell(*kind);
                let cell = crate::runes::SHEET_CELL;
                image.rect = Some(Rect::new(x, y, x + cell, y + cell));
            }
            (Part::Words, .., Some(mut text)) => {
                let (key, own) = if *bottom {
                    ("DOTA_HUD_rune_spawned_bottom", "Rune Spawned Bottom")
                } else {
                    ("DOTA_HUD_rune_spawned_top", "Rune Spawned Top")
                };
                let words = texts.label(key, &[]).unwrap_or_else(|| own.to_owned());
                text.set_if_neq(Text(words));
            }
            _ => {}
        }
    }
}
