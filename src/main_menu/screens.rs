//! Drawing the menu: Dota's dashboard (the top bar, the page under it over the menu's
//! background, the search box in a corner and the MATCH FOUND prompt over everything), redrawn
//! whenever its view changes. Dota's art comes from the asset cache when it has it.

use bevy::picking::hover::{DirectlyHovered, Hovered};
use bevy::prelude::*;

use super::core::{MenuCore, Page};
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
        Hovered::default(),
        DirectlyHovered::default(),
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
    texts: Res<crate::dota_text::Texts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    mut page_root: Query<(Entity, &mut Visibility), With<PageRoot>>,
    overlay_root: Query<Entity, With<OverlayRoot>>,
    fold: Res<super::top_bar::TabFold>,
    mut drawn: Local<Option<(MenuView, super::top_bar::TabFold, u64)>>,
) {
    let view = match (&preview, &core) {
        (Some(preview), _) => preview.0.clone(),
        (None, Some(core)) => core.view(),
        (None, None) => return,
    };
    let clock = super::page_time::Clock::new(preview.is_some());
    if drawn.as_ref().is_some_and(|(shown, folded, minute)| {
        *shown == view && folded == &*fold && *minute == clock.minute_shown(&view.body)
    }) {
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
    // Tab and a controller's pad walk the buttons of Play's open panel, the heroes' pages and the player lists.
    let walkable = (view.play.open && view.page == super::core::Page::Home)
        || matches!(
            view.page,
            super::core::Page::Heroes
                | super::core::Page::Hero(_)
                | super::core::Page::Leaderboard
                | super::core::Page::PlayerProfile(_)
        );
    if walkable && view.offer.is_none() && view.confirm.is_none() {
        kit.commands
            .entity(page)
            .insert(bevy::input_focus::tab_navigation::TabGroup::new(0));
    } else {
        kit.commands
            .entity(page)
            .remove::<bevy::input_focus::tab_navigation::TabGroup>();
    }
    kit.commands.entity(overlay).despawn_children();
    if view.covers() {
        super::pages::draw(&mut kit, page, &view, *fold, &texts, clock);
    }
    super::overlay::draw(&mut kit, overlay, &view);
    let minute = clock.minute_shown(&view.body);
    *drawn = Some((view, *fold, minute));
}

/// A click on a menu button does what it says.
pub fn clicks(
    core: Option<NonSendMut<MenuCore>>,
    buttons: Query<&MenuButton>,
    press: Res<ui::PrimaryPress>,
) {
    let (Some(mut core), Some(target)) = (core, press.0) else {
        return;
    };
    if let Ok(button) = buttons.get(target) {
        core.act(button.0.clone());
    }
}

/// The page a tab belongs to, for drawing it lit: the lobby is under LOBBIES.
pub fn tab_of(page: Page) -> Page {
    match page {
        Page::Lobby => Page::Lobbies,
        Page::Hero(_) => Page::Heroes,
        Page::Result => Page::Home,
        other => other,
    }
}
