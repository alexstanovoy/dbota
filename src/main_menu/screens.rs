//! Drawing the menu: Dota's dashboard (the top bar, the page under it over the menu's
//! background, the search box in a corner and the MATCH FOUND prompt over everything), redrawn
//! whenever its view changes. Dota's art comes from the asset cache when it has it.

use bevy::prelude::*;

use super::core::{Act, MenuCore, Page};
use super::kit::{Kit, MenuButton};
use super::shell::Preview;
use super::view::MenuView;
use crate::hud::Blocker;
use crate::manifest::Manifest;
use crate::theme;
use crate::ui::{self, Fonts};

/// The page layer: covers the screen while no game is on it.
#[derive(Component)]
pub struct PageRoot;

/// The search box and the prompt, over a game too.
#[derive(Component)]
pub struct OverlayRoot;

pub fn spawn_roots(mut commands: Commands) {
    commands.spawn((
        PageRoot,
        Blocker,
        Interaction::default(),
        Node {
            flex_direction: FlexDirection::Column,
            ..ui::fill()
        },
        BackgroundColor(theme::GROUND),
        GlobalZIndex(60),
        Visibility::Hidden,
    ));
    commands.spawn((OverlayRoot, ui::fill(), Pickable::IGNORE, GlobalZIndex(70)));
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
    fold: Res<super::top_bar::TabFold>,
    mut drawn: Local<Option<(MenuView, super::top_bar::TabFold)>>,
) {
    let view = match (&preview, &core) {
        (Some(preview), _) => preview.0.clone(),
        (None, Some(core)) => core.view(),
        (None, None) => return,
    };
    if drawn
        .as_ref()
        .is_some_and(|(shown, folded)| *shown == view && folded == &*fold)
    {
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
        super::pages::draw(&mut kit, page, &view, *fold);
    }
    super::overlay::draw(&mut kit, overlay, &view);
    *drawn = Some((view, *fold));
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

/// The page a tab belongs to, for drawing it lit: the lobby is under LOBBIES.
pub fn tab_of(page: Page) -> Page {
    match page {
        Page::Lobby => Page::Lobbies,
        Page::Result => Page::Home,
        other => other,
    }
}
