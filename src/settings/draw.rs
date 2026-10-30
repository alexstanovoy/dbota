//! Drawing the settings screen: its header of tabs, the tab's rows and,
//! for the hotkeys, the reset button and what clashes. The layout is built
//! again when the screen's state changes; the values in it are kept up to
//! date in place, so a slider stays held while it is dragged.

use bevy::{prelude::*, ui::RelativeCursorPosition};

use super::model::{Choice, Control, Slider, Tab, TextSetting, Toggle, controls};
use super::screen::Screen;
use crate::config::Settings;
use crate::hud::Blocker;
use crate::keys::{self, Group};
use crate::ui::{self, Fonts};

/// The screen's layer.
#[derive(Component)]
pub struct Root;

/// The scrolling part under the header.
#[derive(Component)]
pub struct Body;

#[derive(Component)]
pub struct TabButton(pub Tab);

#[derive(Component)]
pub struct CloseButton;

#[derive(Component)]
pub struct ResetButton;

/// A toggle's row, and the box that shows it on or off.
#[derive(Component)]
pub struct ToggleRow(pub Toggle);
#[derive(Component)]
pub struct ToggleBox(pub Toggle);

/// A slider's track, its filled part and its value.
#[derive(Component)]
pub struct SliderTrack(pub Slider);
#[derive(Component)]
pub struct SliderFill(pub Slider);
#[derive(Component)]
pub struct SliderValue(pub Slider);

/// A choice's button, its value, and an option of its open list.
#[derive(Component)]
pub struct ChoiceButton(pub Choice);
#[derive(Component)]
pub struct ChoiceValue(pub Choice);
#[derive(Component)]
pub struct ChoiceOption(pub Choice, pub usize);

/// A binding's key field, by its place in [`keys::BINDINGS`], and its text.
#[derive(Component)]
pub struct KeyField(pub usize);
#[derive(Component)]
pub struct KeyText(pub usize);

/// A typed line's field, and its text.
#[derive(Component)]
pub struct NameField(pub TextSetting);
#[derive(Component)]
pub struct NameText(pub TextSetting);

/// The line telling which bindings clash.
#[derive(Component)]
pub struct Clashes;

const ON: Color = Color::srgb(0.95, 0.76, 0.32);
const FIELD: Color = Color::srgb(0.1, 0.12, 0.15);
const CLASH: Color = Color::srgb(1.0, 0.42, 0.36);
const ROW_WIDTH: f32 = 560.0;

pub fn spawn_root(mut commands: Commands) {
    commands.spawn((
        Root,
        Blocker,
        Interaction::default(),
        Node {
            flex_direction: FlexDirection::Column,
            ..ui::fill()
        },
        BackgroundGradient::from(LinearGradient::to_bottom(vec![
            Color::srgb(0.06, 0.08, 0.11).into(),
            Color::srgb(0.02, 0.025, 0.035).into(),
        ])),
        GlobalZIndex(65),
        Visibility::Hidden,
    ));
}

/// Builds the screen again for its state.
pub fn layout(
    mut commands: Commands,
    screen: Res<Screen>,
    fonts: Res<Fonts>,
    manifest: Res<crate::manifest::Manifest>,
    root: Single<Entity, With<Root>>,
) {
    if !screen.is_changed() || !screen.open {
        return;
    }
    commands.entity(*root).despawn_children();
    let mut kit = Kit {
        commands: &mut commands,
        fonts: &fonts,
    };
    header(&mut kit, *root, screen.tab);
    let body = kit
        .commands
        .spawn((
            Body,
            Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::all(px(24)),
                row_gap: px(6),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
            ChildOf(*root),
        ))
        .id();
    match screen.tab {
        Tab::Hotkeys => hotkeys(&mut kit, body, &screen),
        tab => {
            for control in controls(tab) {
                control_row(&mut kit, body, control, screen.dropdown);
            }
            // Where the art and the sound come from.
            if tab == Tab::Options {
                heading(&mut kit, body, "ASSETS");
                let line = labelled(&mut kit, body, "");
                kit.text(line, manifest.census(), 13.0, ui::INK_DIM);
            }
        }
    }
    version_line(&mut kit, *root);
}

/// Which client runs and which assets it reads, small in the corner: the build's commit and the
/// asset fingerprint a cache must carry for it.
fn version_line(kit: &mut Kit, root: Entity) {
    let text = format!(
        "{}  ·  assets {}",
        crate::version(),
        dbota_import::import::fingerprint()
    );
    let line = kit.text(root, text, 11.0, ui::INK_DIM);
    kit.commands.entity(line).insert(Node {
        position_type: PositionType::Absolute,
        right: px(12),
        bottom: px(8),
        ..default()
    });
}

/// What spawning the screen needs.
struct Kit<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    fonts: &'a Fonts,
}

impl Kit<'_, '_, '_> {
    fn text(&mut self, parent: Entity, s: impl Into<String>, size: f32, color: Color) -> Entity {
        self.commands
            .spawn((
                ui::text(s, &self.fonts.regular, size, color),
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id()
    }

    fn bold(&mut self, parent: Entity, s: impl Into<String>, size: f32, color: Color) -> Entity {
        self.commands
            .spawn((
                ui::text(s, &self.fonts.bold, size, color),
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

    /// A button with a label, `tag` telling what it does.
    fn button(&mut self, parent: Entity, label: &str, tag: impl Bundle) -> Entity {
        let button = self
            .commands
            .spawn((
                tag,
                Button,
                Node {
                    height: px(32),
                    padding: UiRect::axes(px(16), px(0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.13, 0.15, 0.19)),
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.12)),
                Name::new(label.to_lowercase()),
                ChildOf(parent),
            ))
            .id();
        self.bold(button, label, 14.0, ui::INK);
        button
    }
}

/// The title, the tabs and the way out.
fn header(kit: &mut Kit, root: Entity, on: Tab) {
    let bar = kit
        .commands
        .spawn((
            Node {
                height: px(60),
                padding: UiRect::axes(px(28), px(0)),
                column_gap: px(30),
                align_items: AlignItems::Center,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            BorderColor::all(ui::EDGE),
            ChildOf(root),
        ))
        .id();
    kit.bold(bar, "SETTINGS", 26.0, ui::INK);
    let tabs = kit.node(
        bar,
        Node {
            column_gap: px(6),
            flex_grow: 1.0,
            ..default()
        },
    );
    for tab in Tab::ALL {
        let lit = tab == on;
        let button = kit
            .commands
            .spawn((
                TabButton(tab),
                Button,
                Node {
                    padding: UiRect::axes(px(14), px(18)),
                    border: UiRect::bottom(px(3)),
                    ..default()
                },
                BorderColor::all(if lit { ui::GOLDEN } else { Color::NONE }),
                Name::new(tab.title().to_lowercase()),
                ChildOf(tabs),
            ))
            .id();
        let color = if lit { ui::INK } else { ui::INK_DIM };
        kit.bold(button, tab.title(), 17.0, color);
    }
    kit.button(bar, "DONE", CloseButton);
}

/// A heading over a group of rows.
fn heading(kit: &mut Kit, body: Entity, title: &str) {
    let line = kit.node(
        body,
        Node {
            width: px(ROW_WIDTH),
            margin: UiRect::top(px(14)),
            padding: UiRect::bottom(px(4)),
            border: UiRect::bottom(px(1)),
            ..default()
        },
    );
    kit.commands.entity(line).insert(BorderColor::all(ui::EDGE));
    kit.bold(line, title, 15.0, ui::GOLDEN);
}

/// One row of the options, video or audio tab.
fn control_row(kit: &mut Kit, body: Entity, control: Control, open: Option<Choice>) {
    match control {
        Control::Heading(title) => heading(kit, body, title),
        Control::Text(field) => name_row(kit, body, field),
        Control::Toggle(toggle) => toggle_row(kit, body, toggle),
        Control::Slider(slider) => slider_row(kit, body, slider),
        Control::Choice(choice) => choice_row(kit, body, choice, open == Some(choice)),
    }
}

/// A row of `ROW_WIDTH` with its label at the left.
fn labelled(kit: &mut Kit, body: Entity, label: &str) -> Entity {
    let row = kit.node(
        body,
        Node {
            width: px(ROW_WIDTH),
            min_height: px(34),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: px(12),
            ..default()
        },
    );
    kit.text(row, label, 15.0, ui::INK);
    row
}

/// A typed line (the profile name): a field a click types into, Enter or Escape done.
fn name_row(kit: &mut Kit, body: Entity, setting: TextSetting) {
    let row = labelled(kit, body, setting.label());
    let width = match setting {
        TextSetting::Name => px(240),
    };
    let field = kit
        .commands
        .spawn((
            NameField(setting),
            Button,
            Name::new(setting.label().to_lowercase()),
            Node {
                width,
                height: px(30),
                padding: UiRect::horizontal(px(10)),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(FIELD),
            BorderColor::all(ui::EDGE),
            ChildOf(row),
        ))
        .id();
    kit.commands.spawn((
        NameText(setting),
        ui::text("", &kit.fonts.regular, 15.0, ui::INK),
        TextLayout::no_wrap(),
        Pickable::IGNORE,
        ChildOf(field),
    ));
}

/// A checkbox and its label: a click anywhere on the row turns it.
fn toggle_row(kit: &mut Kit, body: Entity, toggle: Toggle) {
    let row = kit
        .commands
        .spawn((
            ToggleRow(toggle),
            Button,
            Node {
                width: px(ROW_WIDTH),
                min_height: px(34),
                align_items: AlignItems::Center,
                column_gap: px(12),
                ..default()
            },
            ChildOf(body),
        ))
        .id();
    kit.commands.spawn((
        ToggleBox(toggle),
        Node {
            width: px(20),
            height: px(20),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(3)),
            ..default()
        },
        BorderColor::all(ui::INK_DIM),
        BackgroundColor(Color::NONE),
        Pickable::IGNORE,
        ChildOf(row),
    ));
    kit.text(row, toggle.label(), 15.0, ui::INK);
}

/// A slider: its track, filled to the value, the value beside it.
fn slider_row(kit: &mut Kit, body: Entity, slider: Slider) {
    let row = labelled(kit, body, slider.label());
    let right = kit.node(
        row,
        Node {
            align_items: AlignItems::Center,
            column_gap: px(10),
            ..default()
        },
    );
    let track = kit
        .commands
        .spawn((
            SliderTrack(slider),
            Button,
            RelativeCursorPosition::default(),
            Node {
                width: px(240),
                height: px(10),
                border_radius: BorderRadius::all(px(5)),
                ..default()
            },
            BackgroundColor(FIELD),
            ChildOf(right),
        ))
        .id();
    kit.commands.spawn((
        SliderFill(slider),
        Node {
            height: percent(100),
            width: percent(0),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(ON),
        Pickable::IGNORE,
        ChildOf(track),
    ));
    let value = kit.text(right, "", 14.0, ui::INK);
    kit.commands.entity(value).insert((
        SliderValue(slider),
        Node {
            width: px(48),
            ..default()
        },
    ));
}

/// A choice: its value on a button that opens the list under it.
fn choice_row(kit: &mut Kit, body: Entity, choice: Choice, open: bool) {
    let row = labelled(kit, body, choice.label());
    let anchor = kit.node(
        row,
        Node {
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    let button = kit
        .commands
        .spawn((
            ChoiceButton(choice),
            Button,
            Node {
                width: px(220),
                height: px(30),
                padding: UiRect::axes(px(10), px(0)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(FIELD),
            BorderColor::all(if open { ON } else { ui::EDGE }),
            ChildOf(anchor),
        ))
        .id();
    let value = kit.text(button, "", 14.0, ui::INK);
    kit.commands.entity(value).insert(ChoiceValue(choice));
    kit.text(button, "v", 13.0, ui::INK_DIM);
    if open {
        options_list(kit, anchor, choice);
    }
}

/// The open list of a choice, over the rows below it.
fn options_list(kit: &mut Kit, anchor: Entity, choice: Choice) {
    let list = kit
        .commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(32),
                width: px(220),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.05, 0.06, 0.08)),
            BorderColor::all(ON),
            GlobalZIndex(66),
            ChildOf(anchor),
        ))
        .id();
    for (at, option) in choice.options().into_iter().enumerate() {
        let item = kit
            .commands
            .spawn((
                ChoiceOption(choice, at),
                Button,
                Node {
                    height: px(28),
                    padding: UiRect::axes(px(10), px(0)),
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::NONE),
                ChildOf(list),
            ))
            .id();
        kit.text(item, option, 14.0, ui::INK);
    }
}

/// The bindings by group in columns, each with its key field; the reset
/// button and what clashes under them.
fn hotkeys(kit: &mut Kit, body: Entity, screen: &Screen) {
    let columns = kit.node(
        body,
        Node {
            column_gap: px(36),
            align_items: AlignItems::FlexStart,
            ..default()
        },
    );
    // Abilities and items, units and the shop, the camera, the interface
    // and the replays: three columns.
    let split: [&[Group]; 3] = [
        &[Group::Abilities, Group::Items, Group::Shop],
        &[Group::Units],
        &[Group::Camera, Group::Interface, Group::Watching],
    ];
    for groups in split {
        let column = kit.node(
            columns,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                ..default()
            },
        );
        for group in groups {
            kit.bold(column, group.title(), 14.0, ui::GOLDEN);
            for (at, binding) in keys::all().enumerate().filter(|(_, b)| b.group == *group) {
                key_row(kit, column, at, binding.label, screen.capturing == Some(at));
            }
        }
    }
    let foot = kit.node(
        body,
        Node {
            margin: UiRect::top(px(18)),
            column_gap: px(18),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    kit.button(foot, "RESET TO DEFAULTS", ResetButton);
    kit.text(
        foot,
        "Click a key, then press the new one. Backspace unbinds, Escape keeps it.",
        13.0,
        ui::INK_DIM,
    );
    let clashes = kit.text(body, "", 14.0, CLASH);
    kit.commands.entity(clashes).insert(Clashes);
}

/// One binding: its label and its key field.
fn key_row(kit: &mut Kit, column: Entity, at: usize, label: &str, capturing: bool) {
    let row = kit.node(
        column,
        Node {
            width: px(330),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
    );
    kit.text(row, label, 14.0, ui::INK);
    let field = kit
        .commands
        .spawn((
            KeyField(at),
            Button,
            Node {
                width: px(130),
                height: px(24),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(FIELD),
            BorderColor::all(if capturing { ON } else { ui::EDGE }),
            ChildOf(row),
        ))
        .id();
    let text = kit.bold(field, "", 13.0, ui::INK);
    kit.commands.entity(text).insert(KeyText(at));
}

/// Puts the settings' values into the screen as it is drawn.
#[allow(clippy::type_complexity)]
pub fn refresh(
    screen: Res<Screen>,
    settings: Res<Settings>,
    mut boxes: Query<(&ToggleBox, &mut BackgroundColor, &mut BorderColor)>,
    mut fills: Query<(&SliderFill, &mut Node)>,
    mut texts: Query<(
        &mut Text,
        &mut TextColor,
        Option<&SliderValue>,
        Option<&ChoiceValue>,
        Option<&KeyText>,
        Option<&Clashes>,
        Option<&NameText>,
    )>,
    respawned: Query<
        (),
        Or<(
            Added<KeyText>,
            Added<NameText>,
            Added<SliderValue>,
            Added<ChoiceValue>,
            Added<ToggleBox>,
        )>,
    >,
) {
    // The screen drawn anew is filled in too, whatever changed when: the
    // menu may open the screen after this frame's layout ran and before
    // this, which saw the change on no fields yet and would not again (the
    // key fields stayed blank).
    let fresh = !respawned.is_empty();
    if !screen.open || !(settings.is_changed() || screen.is_changed() || fresh) {
        return;
    }
    for (toggle, mut fill, mut edge) in &mut boxes {
        let on = toggle.0.get(&settings);
        fill.set_if_neq(BackgroundColor(if on { ON } else { Color::NONE }));
        edge.set_if_neq(BorderColor::all(if on { ON } else { ui::INK_DIM }));
    }
    for (slider, mut node) in &mut fills {
        let width = percent(slider.0.share(&settings) * 100.0);
        if node.width != width {
            node.width = width;
        }
    }
    for (mut text, mut color, slider, choice, key, clashes, name) in &mut texts {
        let (s, c) = if let Some(name) = name {
            name_text(&screen, &settings, name.0)
        } else if let Some(slider) = slider {
            (slider.0.shown(&settings), ui::INK)
        } else if let Some(choice) = choice {
            (choice.0.shown(&settings), ui::INK)
        } else if let Some(key) = key {
            key_text(&screen, &settings, key.0)
        } else if clashes.is_some() {
            (clash_line(&settings), CLASH)
        } else {
            continue;
        };
        text.set_if_neq(Text(s));
        color.set_if_neq(TextColor(c));
    }
}

/// What a typed line's field says: the line, a caret while typed into,
/// what holds while it is empty (the name games use, no bot).
fn name_text(screen: &Screen, settings: &Settings, setting: TextSetting) -> (String, Color) {
    let line = setting.get(settings);
    match (screen.naming == Some(setting), line.is_empty()) {
        (true, _) => (format!("{line}|"), ON),
        (false, true) => (setting.placeholder().to_owned(), ui::INK_DIM),
        (false, false) => (line.clone(), ui::INK),
    }
}

/// What a key field says, red when its chord clashes with another's.
fn key_text(screen: &Screen, settings: &Settings, at: usize) -> (String, Color) {
    if screen.capturing == Some(at) {
        return ("Press a key…".into(), ON);
    }
    match settings.keys[at] {
        Some(chord) if keys::conflicts(&settings.keys, at).is_empty() => (chord.name(), ui::INK),
        Some(chord) => (chord.name(), CLASH),
        None => ("—".into(), ui::INK_DIM),
    }
}

/// Every pair of bindings sharing a chord where both work.
fn clash_line(settings: &Settings) -> String {
    let mut pairs = Vec::new();
    for at in 0..keys::COUNT {
        for other in keys::conflicts(&settings.keys, at) {
            let theirs = keys::index_of(other).unwrap_or(at);
            if theirs > at {
                pairs.push(format!(
                    "{} and {} share {}",
                    keys::BINDINGS[at].label,
                    keys::BINDINGS[theirs].label,
                    settings.keys[at].map_or_else(String::new, |c| c.name())
                ));
            }
        }
    }
    pairs.join("   ")
}
