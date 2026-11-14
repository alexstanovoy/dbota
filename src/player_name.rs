//! A player's name and optional bot version, with one shared presentation.
use crate::{main_menu::text, theme, ui::Fonts};
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct VersionRun {
    full: String,
    width: f32,
}

impl VersionRun {
    pub(crate) fn set(&mut self, value: Option<&str>, text: &mut Text) {
        let full = version_text(value.unwrap_or(""), 32);
        if full != self.full {
            self.full = full;
            self.width = -1.0;
            text.0.clone_from(&self.full);
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Label {
    pub root: Entity,
    pub name: Entity,
    pub version: Entity,
}

pub(crate) fn plain(name: &str, version: Option<&str>) -> String {
    let name = text::line(name, text::NAME);
    match version.filter(|value| !value.is_empty()) {
        Some(version) => format!("{name} {}", version_text(version, 32)),
        None => name,
    }
}

pub(crate) fn version_text(value: &str, maximum: usize) -> String {
    let mut printable = value
        .bytes()
        .take(132)
        .filter(|byte| (32..=126).contains(byte));
    let mut output: String = printable.by_ref().take(maximum).map(char::from).collect();
    if printable.next().is_some() {
        output.push('…');
    }
    output
}

pub(crate) fn spawn(
    commands: &mut Commands,
    fonts: &Fonts,
    parent: Entity,
    name: &str,
    version: Option<&str>,
    role: theme::Type,
    color: Color,
) -> Label {
    let root = commands
        .spawn((
            Node {
                min_width: px(0),
                max_width: percent(100),
                flex_grow: 1.0,
                align_items: AlignItems::Center,
                column_gap: px(6),
                overflow: Overflow::clip(),
                ..default()
            },
            TextColor(color),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    let name = commands
        .spawn((
            theme::text(text::line(name, text::NAME), fonts, role, color),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
            TextLayout::no_wrap(),
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    let full = version_text(version.unwrap_or(""), 32);
    let version = commands
        .spawn((
            theme::text(&full, fonts, role, theme::version_color(color)),
            VersionRun { full, width: -1.0 },
            Node {
                min_width: px(0),
                flex_basis: px(0),
                flex_grow: 1.0,
                overflow: Overflow::clip(),
                ..default()
            },
            TextLayout::no_wrap(),
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    Label {
        root,
        name,
        version,
    }
}

/// Fit using the font's actual measured width. The name never shrinks to make room for a version.
pub(crate) fn fit(
    mut runs: Query<(
        &ComputedNode,
        &bevy::text::ComputedTextBlock,
        &mut VersionRun,
        &mut Text,
    )>,
) {
    for (node, layout, mut run, mut text) in &mut runs {
        let width = node.size().x;
        if !width.is_finite() || width <= 0.0 {
            continue;
        }
        if (run.width - width).abs() > 0.5 {
            run.width = width;
            if text.0 != run.full {
                text.0.clone_from(&run.full);
                continue;
            }
        }
        let measured = layout.buffer().full_width();
        if measured > width + 0.5 && text.0 == "…" {
            text.0.clear();
            continue;
        }
        if measured > width + 0.5 && !text.0.is_empty() {
            let characters = text.0.chars().count();
            let keep = ((characters as f32 * width / measured).floor() as usize)
                .min(characters.saturating_sub(1));
            text.0 = version_text(&run.full, keep.saturating_sub(1));
        }
    }
}

#[derive(Component)]
pub(crate) struct InlineName {
    label: Label,
    measured: Option<(String, String, TextFont, TextFont, f32)>,
    maximum: f32,
}

/// The same widget in a sentence: fit its full content up to a bounded width.
pub(crate) fn inline(
    commands: &mut Commands,
    fonts: &Fonts,
    parent: Entity,
    name: &str,
    version: Option<&str>,
    role: theme::Type,
    color: Color,
) -> Label {
    inline_within(commands, fonts, parent, (name, version), role, color, 260.0)
}

/// [`inline`] with the widest the widget may grow, for places that are not a line of a sentence
/// (a headline, a card caption).
pub(crate) fn inline_within(
    commands: &mut Commands,
    fonts: &Fonts,
    parent: Entity,
    (name, version): (&str, Option<&str>),
    role: theme::Type,
    color: Color,
    maximum: f32,
) -> Label {
    let label = spawn(commands, fonts, parent, name, version, role, color);
    commands.entity(label.root).insert((
        InlineName {
            label,
            measured: None,
            maximum,
        },
        Node {
            min_width: px(0),
            flex_grow: 0.0,
            align_items: AlignItems::Center,
            column_gap: px(6),
            overflow: Overflow::clip(),
            ..default()
        },
    ));
    label
}

pub(crate) fn measure_inline(
    mut roots: Query<(&mut InlineName, &mut Node, &ComputedNode)>,
    mut pieces: Query<(
        &mut Text,
        &bevy::text::ComputedTextBlock,
        Option<&VersionRun>,
        &TextFont,
    )>,
) {
    for (mut inline, mut node, computed) in &mut roots {
        let Ok((name, name_layout, _, name_font)) = pieces.get(inline.label.name) else {
            continue;
        };
        let Ok((version, version_layout, Some(source), version_font)) =
            pieces.get(inline.label.version)
        else {
            continue;
        };
        if inline.measured.as_ref().is_some_and(
            |(old_name, old_version, old_name_font, old_version_font, scale)| {
                old_name == &name.0
                    && old_version == &source.full
                    && old_name_font == name_font
                    && old_version_font == version_font
                    && *scale == computed.inverse_scale_factor()
            },
        ) {
            continue;
        }
        if version.0 != source.full {
            let full = source.full.clone();
            if let Ok((mut version, _, _, _)) = pieces.get_mut(inline.label.version) {
                version.0 = full;
            }
            continue;
        }
        let name_width = name_layout.buffer().full_width() * computed.inverse_scale_factor();
        // An empty text still measures a few pixels (its caret box); it draws and occupies nothing.
        let version_width = if source.full.is_empty() {
            0.0
        } else {
            version_layout.buffer().full_width() * computed.inverse_scale_factor()
        };
        if (!name.0.is_empty() && name_width <= 0.0)
            || (!source.full.is_empty() && version_width <= 0.0)
        {
            continue;
        }
        let gap = if source.full.is_empty() { 0.0 } else { 6.0 };
        // Each text box rounds independently when UI scale maps it to screen pixels.
        let width = (name_width.ceil() + gap + version_width.ceil() + 1.0)
            .min(inline.maximum.max(name_width.ceil()));
        inline.measured = Some((
            name.0.clone(),
            source.full.clone(),
            name_font.clone(),
            version_font.clone(),
            computed.inverse_scale_factor(),
        ));
        node.width = px(width);
    }
}

#[cfg(test)]
pub(crate) fn assert_drawn_version(
    client: &mut crate::e2e::harness::Client,
    name: &str,
    version: &str,
) {
    let world = client.world_mut();
    let mut runs = world.query::<(
        &Text,
        &TextFont,
        &TextColor,
        &ComputedNode,
        &InheritedVisibility,
        &UiGlobalTransform,
    )>();
    let names: Vec<_> = runs
        .iter(world)
        .filter(|(text, _, _, node, shown, _)| shown.get() && node.size().x > 0.0 && text.0 == name)
        .collect();
    let versions: Vec<_> = runs
        .iter(world)
        .filter(|(text, _, _, node, shown, _)| {
            shown.get() && node.size().x > 0.0 && text.0 == version
        })
        .collect();
    assert!(!names.is_empty(), "drawn name {name}");
    assert!(!versions.is_empty(), "drawn version {version}");
    assert!(
        names
            .iter()
            .any(|name| versions.iter().any(|version| name.1 == version.1
                && (name.5.translation.y - version.5.translation.y).abs() < 0.5
                && version.5.translation.x > name.5.translation.x
                && LinearRgba::from(version.2.0).red < LinearRgba::from(name.2.0).red)),
        "one shared same-line name widget has the same font and dimmer version"
    );
}

#[cfg(test)]
pub(crate) fn describe(world: &World, label: Label) -> String {
    [label.root, label.name, label.version]
        .map(|entity| {
            format!(
                "node={:?} size={:?} width={:?} text={:?} scale={:?}",
                world.get::<Node>(entity).map(|n| n.width),
                world.get::<ComputedNode>(entity).map(|n| n.size()),
                world
                    .get::<bevy::text::ComputedTextBlock>(entity)
                    .map(|b| b.buffer().full_width()),
                world.get::<Text>(entity),
                world
                    .get::<ComputedNode>(entity)
                    .map(|n| n.inverse_scale_factor())
            )
        })
        .join(" / ")
}
