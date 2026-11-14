//! The menu's parts, built from the design tokens (`crate::theme`): text in its roles, panels,
//! buttons of each tone, icon buttons, and how they answer the cursor and the keyboard. Every
//! screen builds from these, so a screen never picks a colour or a size of its own for them.

use bevy::picking::hover::{DirectlyHovered, Hovered};
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::core::Act;
use crate::manifest::Manifest;
use crate::theme::{self, Face, Type};
use crate::ui::Fonts;

/// A button that cannot be pressed now.
pub use crate::ui::Disabled;

/// A button and what it does.
#[derive(Component, Clone)]
#[require(
    bevy::input_focus::tab_navigation::TabIndex,
    Button,
    Hovered,
    DirectlyHovered,
    crate::ui::effects::Motion
)]
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
#[require(crate::ui::effects::Motion)]
pub struct Shade {
    rest: (Color, Color),
    hover: (Color, Color),
    pressed: (Color, Color),
    pub edge: BorderColor,
    horizontal: bool,
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
            horizontal: false,
        }
    }

    fn row(rest: (Color, Color), edge: BorderColor) -> Self {
        Self {
            rest,
            hover: (lighter(rest.0), lighter(rest.1)),
            pressed: (darker(rest.0), darker(rest.1)),
            edge,
            horizontal: true,
        }
    }
    fn gradient(&self, colors: (Color, Color)) -> BackgroundGradient {
        if self.horizontal {
            theme::rightward(colors)
        } else {
            theme::downward(colors)
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
#[require(crate::ui::effects::Motion)]
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
    pub texts: &'a crate::dota_text::Texts,
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
pub(super) const POPUP_ANSWER_WIDTH: f32 = 160.0;

impl Kit<'_, '_, '_> {
    pub fn visual(&mut self) -> crate::ui::Draw<'_, '_, '_> {
        crate::ui::Draw {
            commands: self.commands.reborrow(),
            fonts: self.fonts,
        }
    }
    pub fn row_action(&mut self, entity: Entity, name: &str, act: Act, rest: Color) {
        self.row_effect(entity, (rest, rest), BorderColor::all(Color::NONE));
        self.commands
            .entity(entity)
            .remove::<BackgroundColor>()
            .insert((
                Name::new(name.to_owned()),
                MenuButton(act),
                Pickable::default(),
                BorderColor::all(Color::NONE),
                Outline {
                    width: px(1),
                    offset: px(0),
                    color: Color::NONE,
                },
            ));
    }

    /// Text of a role.
    pub fn typed(
        &mut self,
        parent: Entity,
        s: impl Into<String>,
        role: Type,
        color: Color,
    ) -> Entity {
        self.visual().typed(parent, s, role, color)
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

    pub fn player_name(
        &mut self,
        parent: Entity,
        name: &str,
        version: Option<&str>,
        role: Type,
        color: Color,
    ) -> Entity {
        crate::player_name::spawn(
            &mut self.commands,
            self.fonts,
            parent,
            name,
            version,
            role,
            color,
        )
        .root
    }

    /// The name widget sized to its content, up to `maximum` pixels: for a name inside a sentence
    /// or a centred caption, where [`Kit::player_name`] would fill the row.
    pub fn inline_player_name(
        &mut self,
        parent: Entity,
        (name, version): (&str, Option<&str>),
        role: Type,
        color: Color,
        maximum: f32,
    ) -> Entity {
        crate::player_name::inline_within(
            &mut self.commands,
            self.fonts,
            parent,
            (name, version),
            role,
            color,
            maximum,
        )
        .root
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
        self.visual().node(parent, node)
    }

    /// A selectable row uses the same response curve and selected edge as other menu controls.
    pub fn row_effect(&mut self, entity: Entity, colors: (Color, Color), edge: BorderColor) {
        let shade = Shade::row(colors, edge);
        self.commands
            .entity(entity)
            .insert((shade, shade.gradient(colors)));
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
        self.button_role(parent, label, act, tone, height, theme::button(height))
    }

    /// A control whose approved layout assigns a different text role from its height.
    pub fn button_role(
        &mut self,
        parent: Entity,
        label: &str,
        act: Act,
        tone: Tone,
        height: f32,
        role: Type,
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
        let text = self.typed(button, label, role, theme::INK_1);
        self.commands.entity(text).insert(TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        });
        button
    }

    /// A button that does nothing now, greyed: the script names it `Disabled`.
    pub fn disabled_button(&mut self, parent: Entity, label: &str, height: f32) -> Entity {
        self.disabled_button_role(parent, label, height, theme::button(height))
    }
    pub fn disabled_button_role(
        &mut self,
        parent: Entity,
        label: &str,
        height: f32,
        role: Type,
    ) -> Entity {
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
        let text = self.typed(button, label, role, theme::INK_OFF);
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

    /// The wide hero art used by match cards and scoreboards.
    pub fn hero_icon(&mut self, parent: Entity, hero: u16, width: f32) {
        if let Some(path) = self.manifest.hero_icon(hero) {
            self.commands.spawn((
                Node {
                    width: px(width),
                    height: px(width * 9.0 / 16.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                ImageNode::new(self.server.load(path)),
                Pickable::IGNORE,
                ChildOf(parent),
            ));
        }
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
    native: Res<bevy::input_focus::InputFocus>,
    visible: Res<bevy::input_focus::InputFocusVisible>,
    mut buttons: Query<(
        Entity,
        &MenuButton,
        &Shade,
        &mut BorderColor,
        Option<&mut Outline>,
    )>,
) {
    let focus = core.as_ref().and_then(|core| core.button_focus());
    let ring = BorderColor::all(theme::FOCUS);
    for (entity, button, shade, mut border, outline) in &mut buttons {
        let focused = visible.0
            && if let Some(native) = native.get() {
                native == entity
            } else {
                focus == Some(&button.0)
            };
        if let Some(mut outline) = outline {
            let want = if focused { theme::FOCUS } else { Color::NONE };
            if outline.color != want {
                outline.color = want;
            }
        }
        let want = if focused { &ring } else { &shade.edge };
        if *border != *want {
            *border = *want;
        }
    }
}

/// Controls share hover transitions and pressed feedback, regardless of the activating device.
pub fn hover(
    mut shaded: Query<(
        Ref<crate::ui::effects::Motion>,
        Ref<Shade>,
        &mut BackgroundGradient,
    )>,
    washed: Query<(Ref<crate::ui::effects::Motion>, Ref<Wash>)>,
    mut images: Query<&mut ImageNode>,
    mut words: Query<&mut TextColor>,
) {
    use crate::ui::effects::{amount, tint};
    for (motion, shade, mut gradient) in &mut shaded {
        if !motion.is_changed() && !shade.is_changed() {
            continue;
        }
        let color = |index: bool| {
            let (rest, hover, pressed) = if index {
                (shade.rest.1, shade.hover.1, shade.pressed.1)
            } else {
                (shade.rest.0, shade.hover.0, shade.pressed.0)
            };
            tint(
                tint(rest, hover, amount(motion.hover)),
                pressed,
                amount(motion.pressed),
            )
        };
        gradient.set_if_neq(shade.gradient((color(false), color(true))));
    }
    for (motion, wash) in &washed {
        if !motion.is_changed() && !wash.is_changed() {
            continue;
        }
        let color = tint(
            wash.rest,
            wash.hover,
            amount(motion.hover.max(motion.pressed)),
        );
        if let Ok(mut image) = images.get_mut(wash.target)
            && image.color != color
        {
            image.color = color;
        }
        if let Ok(mut ink) = words.get_mut(wash.target) {
            ink.set_if_neq(TextColor(color));
        }
    }
}

/// Absolute menu geometry and type roles shared by dashboard pages and result windows.
pub use crate::ui::absolute as area;

pub fn at(left: f32, top: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(left),
        top: px(top),
        ..default()
    }
}

pub fn rect(kit: &mut Kit, parent: Entity, node: Node, colour: Color) -> Entity {
    let entity = kit.node(parent, node);
    kit.commands.entity(entity).insert(BackgroundColor(colour));
    entity
}

pub fn words(
    kit: &mut Kit,
    parent: Entity,
    text: impl Into<String>,
    node: Node,
    role: Type,
    colour: Color,
) -> Entity {
    let entity = kit.typed(parent, text, role, colour);
    kit.commands
        .entity(entity)
        .insert((node, TextLayout::no_wrap()));
    entity
}

pub fn number(size: f32) -> Type {
    Type {
        size,
        tracking: 0.0,
        face: Face::Regular,
        ..theme::NUMBER
    }
}
pub fn body(size: f32) -> Type {
    Type {
        size,
        tracking: 0.0,
        ..theme::BODY
    }
}
pub fn bold(size: f32) -> Type {
    Type {
        size,
        tracking: 0.0,
        ..theme::HEADING
    }
}
pub fn caption(size: f32) -> Type {
    Type {
        size,
        tracking: 1.0,
        ..theme::CAPTION
    }
}
