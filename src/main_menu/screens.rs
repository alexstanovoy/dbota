//! Drawing the menu: Dota's dashboard as far as plain boxes go (a dark top
//! bar of tabs, the page under it, the search box in a corner and the
//! MATCH FOUND prompt over everything), redrawn whenever its view changes.
//! Heroes' portraits come from the asset cache when it has them.

use bevy::prelude::*;

use super::core::{Act, MenuCore, Page};
use super::shell::Preview;
use super::view::MenuView;
use crate::hud::Blocker;
use crate::manifest::Manifest;
use crate::ui::{self, Fonts};

/// The page layer: covers the screen while no game is on it.
#[derive(Component)]
pub struct PageRoot;

/// The search box and the prompt, over a game too.
#[derive(Component)]
pub struct OverlayRoot;

/// A button and what it does.
#[derive(Component, Clone)]
pub struct MenuButton(pub Act);

/// A button's colours at rest and under the cursor.
#[derive(Component, Clone, Copy)]
pub struct Look {
    rest: Color,
    hover: Color,
}

/// Dota's green, for Play and Accept.
pub const GREEN: (Color, Color) = (Color::srgb(0.16, 0.42, 0.18), Color::srgb(0.22, 0.55, 0.24));
/// For Decline and Leave.
pub const RED: (Color, Color) = (Color::srgb(0.42, 0.12, 0.1), Color::srgb(0.56, 0.16, 0.13));
/// Everything else.
pub const PLAIN: (Color, Color) = (Color::srgb(0.13, 0.15, 0.19), Color::srgb(0.2, 0.23, 0.28));
/// The tab of the page shown.
const TAB_ON: Color = Color::srgb(0.95, 0.96, 0.98);
/// The text of a refusal.
pub const ERROR: Color = Color::srgb(1.0, 0.45, 0.38);

pub fn spawn_roots(mut commands: Commands) {
    commands.spawn((
        PageRoot,
        Blocker,
        Interaction::default(),
        Node {
            flex_direction: FlexDirection::Column,
            ..ui::fill()
        },
        BackgroundGradient::from(LinearGradient::to_bottom(vec![
            Color::srgb(0.07, 0.09, 0.12).into(),
            Color::srgb(0.02, 0.025, 0.035).into(),
        ])),
        GlobalZIndex(60),
        Visibility::Hidden,
    ));
    commands.spawn((OverlayRoot, ui::fill(), Pickable::IGNORE, GlobalZIndex(70)));
}

/// What the builders need.
pub struct Kit<'w, 's, 'a> {
    pub commands: Commands<'w, 's>,
    pub fonts: &'a Fonts,
    pub manifest: &'a Manifest,
    pub server: &'a AssetServer,
}

impl Kit<'_, '_, '_> {
    pub fn text(
        &mut self,
        parent: Entity,
        s: impl Into<String>,
        size: f32,
        color: Color,
    ) -> Entity {
        self.commands
            .spawn((
                ui::text(s, &self.fonts.regular, size, color),
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id()
    }

    pub fn bold(
        &mut self,
        parent: Entity,
        s: impl Into<String>,
        size: f32,
        color: Color,
    ) -> Entity {
        self.commands
            .spawn((
                ui::text(s, &self.fonts.bold, size, color),
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id()
    }

    /// A box laid out by `node`.
    pub fn node(&mut self, parent: Entity, node: Node) -> Entity {
        self.commands
            .spawn((node, Pickable::IGNORE, ChildOf(parent)))
            .id()
    }

    /// A dark panel with an edge.
    pub fn panel(&mut self, parent: Entity, node: Node) -> Entity {
        self.commands
            .spawn((
                Node {
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    ..node
                },
                BackgroundColor(ui::PANEL),
                BorderColor::all(ui::EDGE),
                ChildOf(parent),
            ))
            .id()
    }

    /// A button with `label` in `look`'s colours, `height` tall.
    pub fn button(
        &mut self,
        parent: Entity,
        label: &str,
        act: Act,
        look: (Color, Color),
        height: f32,
    ) -> Entity {
        let button = self
            .commands
            .spawn((
                MenuButton(act),
                Button,
                Look {
                    rest: look.0,
                    hover: look.1,
                },
                Node {
                    height: px(height),
                    padding: UiRect::axes(px(height * 0.45), px(0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(look.0),
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.12)),
                ChildOf(parent),
            ))
            .id();
        self.bold(button, label, (height * 0.38).clamp(12.0, 22.0), ui::INK);
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
            BorderColor::all(ui::EDGE),
            ImageNode::new(self.server.load(path)),
            Pickable::IGNORE,
            ChildOf(parent),
        ));
    }
}

/// Redraws the menu when what it shows changed.
pub fn draw(
    commands: Commands,
    core: Option<NonSend<MenuCore>>,
    preview: Option<Res<Preview>>,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    mut page_root: Query<(Entity, &mut Visibility), With<PageRoot>>,
    overlay_root: Query<Entity, With<OverlayRoot>>,
    mut drawn: Local<Option<MenuView>>,
) {
    let view = match (&preview, &core) {
        (Some(preview), _) => preview.0.clone(),
        (None, Some(core)) => core.view(),
        (None, None) => return,
    };
    if drawn.as_ref() == Some(&view) {
        return;
    }
    let (Ok((page, mut shown)), Ok(overlay)) = (page_root.single_mut(), overlay_root.single())
    else {
        return;
    };
    shown.set_if_neq(ui::visible(view.covers()));
    let mut kit = Kit {
        commands,
        fonts: &fonts,
        manifest: &manifest,
        server: &server,
    };
    kit.commands.entity(page).despawn_children();
    kit.commands.entity(overlay).despawn_children();
    if view.covers() {
        super::pages::draw(&mut kit, page, &view);
    }
    super::overlay::draw(&mut kit, overlay, &view);
    *drawn = Some(view);
}

/// A click on a menu button does what it says.
pub fn clicks(
    core: Option<NonSendMut<MenuCore>>,
    buttons: Query<(&Interaction, &MenuButton), Changed<Interaction>>,
) {
    let Some(mut core) = core else {
        return;
    };
    let pressed: Vec<Act> = buttons
        .iter()
        .filter(|(i, _)| **i == Interaction::Pressed)
        .map(|(_, b)| b.0.clone())
        .collect();
    for act in pressed {
        core.act(act);
    }
}

pub fn hover(
    mut buttons: Query<(&Interaction, &Look, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (interaction, look, mut color) in &mut buttons {
        color.0 = match interaction {
            Interaction::None => look.rest,
            _ => look.hover,
        };
    }
}

/// The colour of a tab's label.
pub fn tab_color(on: bool) -> Color {
    if on { TAB_ON } else { ui::INK_DIM }
}

/// The page a tab belongs to, for drawing it lit: the lobby is under LOBBIES.
pub fn tab_of(page: Page) -> Page {
    match page {
        Page::Lobby => Page::Lobbies,
        Page::Result => Page::Home,
        other => other,
    }
}
