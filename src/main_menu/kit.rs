//! The menu's parts, built from the design tokens (`crate::theme`): text in its roles, panels,
//! buttons of each tone, icon buttons, and how they answer the cursor and the keyboard. Every
//! screen builds from these, so a screen never picks a colour or a size of its own for them.

use bevy::prelude::*;

use super::core::Act;
use crate::manifest::Manifest;
use crate::theme::{self, Face, Type};
use crate::ui::Fonts;

/// A button that cannot be pressed now.
#[derive(Component)]
pub struct Disabled;

/// A button and what it does.
#[derive(Component, Clone)]
pub struct MenuButton(pub Act);

/// What a button is for, which its look says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// The action the screen is for: PLAY, ACCEPT, CONTINUE.
    Primary,
    /// Any other action.
    Secondary,
    /// One that leaves or ends: LEAVE GAME, DISCONNECT, DELETE.
    Danger,
}

/// A button's ground at rest, under the cursor and pressed (each top to bottom), and its edge.
#[derive(Component, Clone, Copy)]
pub struct Shade {
    rest: (Color, Color),
    hover: (Color, Color),
    pressed: (Color, Color),
    pub edge: BorderColor,
}

impl Shade {
    fn of(tone: Tone) -> Shade {
        let (rest, edge) = match tone {
            Tone::Primary => (
                theme::GO,
                BorderColor {
                    top: theme::GO_GLINT,
                    ..BorderColor::all(Color::NONE)
                },
            ),
            Tone::Secondary => (theme::SECONDARY, BorderColor::all(theme::SECONDARY_EDGE)),
            Tone::Danger => (theme::STOP, BorderColor::all(theme::STOP_EDGE)),
        };
        Shade {
            rest,
            hover: (lighter(rest.0), lighter(rest.1)),
            pressed: (darker(rest.0), darker(rest.1)),
            edge,
        }
    }
}

/// A colour 8 % lighter, as a button brightens under the cursor.
fn lighter(color: Color) -> Color {
    let mut hsla = Hsla::from(color);
    hsla.lightness = (hsla.lightness + 0.08).min(1.0);
    hsla.into()
}

/// A colour 10 % darker, as a button is pressed.
fn darker(color: Color) -> Color {
    let mut hsla = Hsla::from(color);
    hsla.lightness = (hsla.lightness - 0.1).max(0.0);
    hsla.into()
}

/// A part of a button tinted at rest and brighter under the cursor: its icon or its words.
#[derive(Component, Clone, Copy)]
pub struct Wash {
    pub target: Entity,
    pub rest: Color,
    pub hover: Color,
}

/// What the builders need.
pub struct Kit<'w, 's, 'a> {
    pub commands: Commands<'w, 's>,
    pub fonts: &'a Fonts,
    pub manifest: &'a Manifest,
    pub server: &'a AssetServer,
}

/// A button is this many times as wide as it is tall: wide enough for the longest label of its
/// height (CREATE FOR THE LAN, WAITING…), so every button of a height is as wide as the next.
const BUTTON_WIDTH_PER_HEIGHT: f32 = 5.0;

/// One width for the buttons of a height, pressable or not, whatever they say, so none of them
/// wraps or differs from its neighbour; a lone glyph (the close cross) is square.
fn button_width(label: &str, height: f32) -> f32 {
    if label.chars().count() <= 2 {
        height
    } else {
        height * BUTTON_WIDTH_PER_HEIGHT
    }
}
/// A popup's OK and Cancel, one width.
const POPUP_ANSWER_WIDTH: f32 = 160.0;

impl Kit<'_, '_, '_> {
    /// Text of a role.
    pub fn typed(
        &mut self,
        parent: Entity,
        s: impl Into<String>,
        role: Type,
        color: Color,
    ) -> Entity {
        self.commands
            .spawn((
                theme::text(s, self.fonts, role, color),
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id()
    }

    /// Running text of a size, for today's pages until their screens are redesigned; new screens
    /// use [`Kit::typed`] with a role of `theme`.
    pub fn text(
        &mut self,
        parent: Entity,
        s: impl Into<String>,
        size: f32,
        color: Color,
    ) -> Entity {
        let role = Type {
            size,
            ..theme::BODY
        };
        self.typed(parent, s, role, color)
    }

    /// Bold text of a size, for today's pages until their screens are redesigned (see
    /// [`Kit::text`]).
    pub fn bold(
        &mut self,
        parent: Entity,
        s: impl Into<String>,
        size: f32,
        color: Color,
    ) -> Entity {
        let role = Type {
            face: Face::Bold,
            size,
            tracking: 0.0,
            figures: false,
        };
        self.typed(parent, s, role, color)
    }

    /// A box laid out by `node`.
    pub fn node(&mut self, parent: Entity, node: Node) -> Entity {
        self.commands
            .spawn((node, Pickable::IGNORE, ChildOf(parent)))
            .id()
    }

    /// A panel over the menu's background: square, its edge lit along the top.
    pub fn panel(&mut self, parent: Entity, node: Node) -> Entity {
        self.commands
            .spawn((
                Node {
                    border: UiRect::all(px(1)),
                    ..node
                },
                theme::downward(theme::PANEL),
                BorderColor {
                    top: theme::PANEL_GLINT,
                    ..BorderColor::all(theme::PANEL_EDGE)
                },
                ChildOf(parent),
            ))
            .id()
    }

    /// A button with `label` in `tone`, `height` tall; a script names it by its label.
    pub fn button(
        &mut self,
        parent: Entity,
        label: &str,
        act: Act,
        tone: Tone,
        height: f32,
    ) -> Entity {
        let shade = Shade::of(tone);
        let button = self
            .commands
            .spawn((
                MenuButton(act),
                Button,
                shade,
                Node {
                    width: px(button_width(label, height)),
                    height: px(height),
                    padding: UiRect::axes(px(height * 0.2), px(0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(1)),
                    // Not squeezed by the words beside it either: a long replay name or reason
                    // in its row made its delete button narrower than the others.
                    flex_shrink: 0.0,
                    ..default()
                },
                theme::downward(shade.rest),
                shade.edge,
                Name::new(label.to_lowercase()),
                ChildOf(parent),
            ))
            .id();
        let text = self.typed(button, label, theme::button(height), theme::INK_1);
        self.commands.entity(text).insert(TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        });
        button
    }

    /// A button that does nothing now, greyed: the script names it `Disabled`.
    pub fn disabled_button(&mut self, parent: Entity, label: &str, height: f32) -> Entity {
        let button = self
            .commands
            .spawn((
                Disabled,
                Node {
                    width: px(button_width(label, height)),
                    height: px(height),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(1)),
                    flex_shrink: 0.0,
                    ..default()
                },
                theme::downward(theme::DISABLED),
                BorderColor::all(theme::DISABLED_EDGE),
                Name::new(label.to_lowercase()),
                ChildOf(parent),
            ))
            .id();
        let text = self.typed(button, label, theme::button(height), theme::INK_OFF);
        self.commands.entity(text).insert(TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        });
        button
    }

    /// Words that act when clicked and brighten under the cursor, on no ground: LOG OUT in the
    /// top bar, a link in a line.
    pub fn quiet(
        &mut self,
        parent: Entity,
        label: &str,
        act: Act,
        role: Type,
        ink: (Color, Color),
    ) -> Entity {
        let button = self
            .commands
            .spawn((
                MenuButton(act),
                Button,
                Node {
                    min_width: px(theme::HIT),
                    min_height: px(theme::HIT),
                    justify_content: JustifyContent::Center,
                    padding: UiRect::axes(px(8), px(0)),
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    ..default()
                },
                Name::new(label.to_lowercase()),
                ChildOf(parent),
            ))
            .id();
        let words = self.typed(button, label, role, ink.0);
        self.commands.entity(words).insert(TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        });
        self.commands.entity(button).insert(Wash {
            target: words,
            rest: ink.0,
            hover: ink.1,
        });
        button
    }

    /// A [`theme::HIT`] square button drawn as one of Dota's control icons (`art`), `icon` across,
    /// secondary ink at rest and white under the cursor; without the art, `label` in words on a
    /// button at least as large.
    pub fn icon_button(
        &mut self,
        parent: Entity,
        art: &str,
        label: &str,
        act: Act,
        icon: f32,
    ) -> Entity {
        let Some(path) = self.manifest.file(art) else {
            let ink = (theme::INK_3, theme::INK_1);
            return self.quiet(parent, label, act, theme::CAPTION, ink);
        };
        let button = self
            .commands
            .spawn((
                MenuButton(act),
                Button,
                Node {
                    width: px(theme::HIT),
                    height: px(theme::HIT),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    ..default()
                },
                Name::new(label.to_lowercase()),
                ChildOf(parent),
            ))
            .id();
        let picture = self
            .commands
            .spawn((
                Node {
                    width: px(icon),
                    height: px(icon),
                    ..default()
                },
                ImageNode::new(self.server.load(path)).with_color(theme::INK_3),
                Pickable::IGNORE,
                ChildOf(button),
            ))
            .id();
        self.commands.entity(button).insert(Wash {
            target: picture,
            rest: theme::INK_3,
            hover: theme::INK_1,
        });
        button
    }

    /// Dota's art of `logical` laid out by `node`; nothing drawn where the cache has none.
    pub fn art(&mut self, parent: Entity, logical: &str, node: Node) -> Entity {
        let art = crate::ui::art(self.manifest, self.server, logical);
        self.commands
            .spawn((node, art, Pickable::IGNORE, ChildOf(parent)))
            .id()
    }

    /// A popup's answer (OK, Cancel): a secondary button of its words, as wide as the other's.
    pub fn popup_button(&mut self, parent: Entity, label: &str, act: Act) -> Entity {
        let button = self.button(parent, &label.to_uppercase(), act, Tone::Secondary, 44.0);
        self.commands
            .entity(button)
            .entry::<Node>()
            .and_modify(|mut node| node.width = px(POPUP_ANSWER_WIDTH));
        button
    }

    /// A hero's portrait from the cache, `width` wide; nothing without it.
    pub fn portrait(&mut self, parent: Entity, hero: u16, width: f32) {
        let Some(path) = self.manifest.portrait(hero) else {
            return;
        };
        self.commands.spawn((
            Node {
                width: px(width),
                height: px(width * 9.0 / 16.0),
                border: UiRect::all(px(1)),
                ..default()
            },
            BorderColor::all(theme::PANEL_EDGE),
            ImageNode::new(self.server.load(path)),
            Pickable::IGNORE,
            ChildOf(parent),
        ));
    }
}

/// The edge round the button that has the keyboard (Tab visits the form's buttons).
pub fn focus_ring(
    core: Option<NonSend<super::MenuCore>>,
    mut buttons: Query<(&MenuButton, &Shade, &mut BorderColor)>,
) {
    let focus = core.as_ref().and_then(|core| core.button_focus());
    let ring = BorderColor::all(theme::FOCUS);
    for (button, shade, mut border) in &mut buttons {
        let want = if focus == Some(&button.0) {
            &ring
        } else {
            &shade.edge
        };
        if *border != *want {
            *border = *want;
        }
    }
}

/// Buttons brighten under the cursor and darken pressed; washed icons and words brighten.
pub fn hover(
    mut shaded: Query<(&Interaction, &Shade, &mut BackgroundGradient), Changed<Interaction>>,
    washed: Query<(&Interaction, &Wash), Changed<Interaction>>,
    mut images: Query<&mut ImageNode>,
    mut words: Query<&mut TextColor>,
) {
    for (interaction, shade, mut gradient) in &mut shaded {
        let colors = match interaction {
            Interaction::None => shade.rest,
            Interaction::Hovered => shade.hover,
            Interaction::Pressed => shade.pressed,
        };
        *gradient = theme::downward(colors);
    }
    for (interaction, wash) in &washed {
        let color = match interaction {
            Interaction::None => wash.rest,
            _ => wash.hover,
        };
        if let Ok(mut image) = images.get_mut(wash.target) {
            image.color = color;
        }
        if let Ok(mut ink) = words.get_mut(wash.target) {
            ink.0 = color;
        }
    }
}
