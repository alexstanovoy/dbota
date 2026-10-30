//! The shop panel, laid out as Dota 7.41's: the search box over the BASICS,
//! UPGRADES and NEUTRALS tabs, each tab's categories two abreast with four
//! icons a row, the search results in their place while a search is typed,
//! and the build tree of the item last clicked.

use bevy::{input::mouse::AccumulatedMouseScroll, prelude::*};

use super::catalog::{Catalog, Tab};
use super::cell::{self, CELL, Chosen, GAP, Place};
use crate::dota_text::Texts;
use crate::hud::Blocker;
use crate::input::{Panels, Typing};
use crate::manifest::Manifest;
use crate::session::Session;
use crate::tip_content::Lookup;
use crate::ui::{self, Extent, Fonts};

/// Icons a category's row holds.
const COLUMNS: usize = 4;
/// Icons a row of search results holds.
const RESULT_COLUMNS: usize = 8;
/// Space between the two columns of categories.
const COLUMN_GAP: f32 = 16.0;
/// The panel's inner width: two categories abreast.
pub const INNER: f32 = 2.0 * (COLUMNS as f32 * CELL.0 + (COLUMNS - 1) as f32 * GAP) + COLUMN_GAP;
/// Padding inside the panel.
pub const PAD: f32 = 10.0;

/// Dota's shop colours: the panel's top and foot, its edge, the tabs and the
/// category titles.
const PANEL_TOP: Color = Color::srgba(0.09, 0.11, 0.13, 0.97);
const PANEL_FOOT: Color = Color::srgba(0.05, 0.06, 0.07, 0.97);
const PANEL_EDGE: Color = Color::srgb(0.16, 0.18, 0.21);
const TAB_ON: Color = Color::srgb(0.13, 0.15, 0.18);
const TAB_OFF: Color = Color::srgb(0.06, 0.07, 0.085);
const TITLE: Color = Color::srgb(0.78, 0.81, 0.85);

/// The shop panel.
#[derive(Component)]
pub struct ShopPanel;

/// The scrolling page of the tabs and the results.
#[derive(Component)]
struct Page;

/// A tab's button.
#[derive(Component, Clone, Copy)]
pub struct TabButton(pub Tab);

/// A tab's page of categories.
#[derive(Component, Clone, Copy)]
struct TabPage(Tab);

/// The search results' page, and the grid its icons go in.
#[derive(Component)]
struct Results;
#[derive(Component)]
struct ResultGrid;

/// The search results' title.
#[derive(Component)]
struct ResultTitle;

/// The search box, and its text.
#[derive(Component)]
pub struct SearchBox;
#[derive(Component)]
struct SearchText;

/// The panel's foot, where an item dropped is sold.
#[derive(Component)]
pub struct SellStrip;

/// The tab shown.
#[derive(Resource, Default)]
pub struct Shown(pub Tab);

pub fn plugin(app: &mut App) {
    app.init_resource::<Shown>()
        .add_systems(Startup, spawn)
        .add_systems(
            Update,
            (
                debug_open,
                show,
                above_bottom_row,
                (follow_catalogue, restock).chain(),
                tabs,
                results,
                search_text,
                scroll,
            )
                .run_if(super::match_or_tactical),
        );
}

/// The art the panel draws with.
type Art<'a> = (&'a Fonts, &'a Manifest, &'a AssetServer);

/// Cuts the shop to the items the match plays with whenever they change: the lobby tells them
/// before the start, the match after, and a match shared with an older build has fewer.
fn follow_catalogue(session: NonSend<Session>, mut catalog: ResMut<Catalog>) {
    let catalogue = session.0.catalogue();
    if catalog.bypass_change_detection().offered != catalogue {
        catalog.offer(catalogue);
    }
}

/// Lays the tab pages out anew once the shop was cut to other items.
fn restock(
    mut commands: Commands,
    catalog: Res<Catalog>,
    art: (Res<Fonts>, Res<Manifest>, Res<AssetServer>),
    page: Single<Entity, With<Page>>,
    pages: Query<(Entity, &TabPage)>,
) {
    if !catalog.is_changed() {
        return;
    }
    for (entity, TabPage(tab)) in &pages {
        if *tab != Tab::Neutrals {
            commands.entity(entity).despawn();
        }
    }
    let art = (&*art.0, &*art.1, &*art.2);
    for tab in [Tab::Basics, Tab::Upgrades] {
        spawn_page(&mut commands, art, &catalog, *page, tab);
    }
}

fn spawn(
    mut commands: Commands,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    catalog: Res<Catalog>,
) {
    let art = (&*fonts, &*manifest, &*server);
    let panel = commands
        .spawn((
            ShopPanel,
            Blocker,
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                right: px(ui::CORNER_MARGIN),
                top: px(70),
                bottom: px(0),
                width: px(INNER + 2.0 * PAD + 2.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::auto(PANEL_TOP),
                ColorStop::auto(PANEL_FOOT),
            ])),
            BorderColor::all(PANEL_EDGE),
            Visibility::Hidden,
        ))
        .id();
    cell::backdrop(
        &mut commands,
        (&manifest, &server),
        panel,
        "shop.panel",
        Color::WHITE,
    );
    spawn_search(&mut commands, &fonts, &catalog, panel);
    spawn_tabs(&mut commands, &fonts, &catalog, panel);
    let page = commands
        .spawn((
            Page,
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                min_height: px(0),
                padding: UiRect::axes(px(PAD), px(8)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
            ChildOf(panel),
        ))
        .id();
    for tab in [Tab::Basics, Tab::Upgrades] {
        spawn_page(&mut commands, art, &catalog, page, tab);
    }
    spawn_neutrals(&mut commands, &fonts, &catalog, page);
    spawn_results(&mut commands, &fonts, &catalog, page);
    spawn_foot(&mut commands, &fonts, panel);
    super::tree::spawn(&mut commands, panel);
}

/// The search box: a magnifier and the typed text, or the word for search.
fn spawn_search(commands: &mut Commands, fonts: &Fonts, catalog: &Catalog, panel: Entity) {
    let row = commands
        .spawn((
            Node {
                padding: UiRect::new(px(PAD), px(PAD), px(PAD), px(6)),
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    let search = commands
        .spawn((
            SearchBox,
            Button,
            Node {
                flex_grow: 1.0,
                height: px(28),
                padding: UiRect::horizontal(px(8)),
                column_gap: px(8),
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(2)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
            BorderColor::all(Color::srgb(0.25, 0.28, 0.32)),
            ChildOf(row),
        ))
        .id();
    spawn_magnifier(commands, search);
    commands.spawn((
        SearchText,
        ui::text(
            catalog.word("dota_search", "Search"),
            &fonts.regular,
            14.0,
            ui::INK_DIM,
        ),
        Pickable::IGNORE,
        ChildOf(search),
    ));
}

/// A magnifying glass of two shapes: a ring and its handle.
fn spawn_magnifier(commands: &mut Commands, parent: Entity) {
    let glass = commands
        .spawn((
            Node {
                width: px(16),
                height: px(16),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        BorderColor::all(ui::INK_DIM),
        Node {
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::MAX,
            ..ui::absolute(0.0, 0.0, 11.0, 11.0)
        },
        Pickable::IGNORE,
        ChildOf(glass),
    ));
    commands.spawn((
        ui::absolute(9.0, 11.5, 7.0, 2.5),
        UiTransform::from_rotation(Rot2::degrees(45.0)),
        BackgroundColor(ui::INK_DIM),
        Pickable::IGNORE,
        ChildOf(glass),
    ));
}

/// BASICS, UPGRADES and NEUTRALS.
fn spawn_tabs(commands: &mut Commands, fonts: &Fonts, catalog: &Catalog, panel: Entity) {
    let row = commands
        .spawn((
            Node {
                padding: UiRect::horizontal(px(PAD + 12.0)),
                column_gap: px(6),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(PANEL_EDGE),
            ChildOf(panel),
        ))
        .id();
    for tab in Tab::ALL {
        let (key, plain) = tab.label();
        let button = commands
            .spawn((
                TabButton(tab),
                Button,
                Node {
                    flex_grow: 1.0,
                    height: px(26),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::top(px(3)),
                    ..default()
                },
                BackgroundColor(TAB_OFF),
                ChildOf(row),
            ))
            .id();
        commands.spawn((
            ui::text(catalog.word(key, plain), &fonts.bold, 12.0, ui::INK_DIM),
            Pickable::IGNORE,
            ChildOf(button),
        ));
    }
}

/// A grid of icons `columns` wide.
fn grid(columns: usize) -> Node {
    Node {
        display: Display::Grid,
        grid_template_columns: RepeatedGridTrack::px(columns as u16, CELL.0),
        column_gap: px(GAP),
        row_gap: px(GAP),
        ..default()
    }
}

/// A category's title, as Dota prints it.
fn title(commands: &mut Commands, fonts: &Fonts, parent: Entity, text: &str) -> Entity {
    commands
        .spawn((
            ui::text(text.to_uppercase(), &fonts.bold, 12.0, TITLE),
            Node {
                margin: UiRect::bottom(px(5)),
                ..default()
            },
            ChildOf(parent),
        ))
        .id()
}

/// A tab's categories, two abreast, in Dota's order.
fn spawn_page(commands: &mut Commands, art: Art, catalog: &Catalog, page: Entity, tab: Tab) {
    let fonts = art.0;
    let body = commands
        .spawn((
            TabPage(tab),
            Node {
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::auto(2),
                column_gap: px(COLUMN_GAP),
                row_gap: px(14),
                align_items: AlignItems::Start,
                ..default()
            },
            ChildOf(page),
        ))
        .id();
    for section in catalog.sections.iter().filter(|s| s.tab == tab) {
        let block = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                ChildOf(body),
            ))
            .id();
        title(commands, fonts, block, &section.title);
        let cells = commands.spawn((grid(COLUMNS), ChildOf(block))).id();
        for id in &section.items {
            cell::spawn(commands, art, cells, *id, Place::Grid, CELL);
        }
    }
}

/// The neutral items' tab: bota's 1v1 has no neutral items to hand out.
fn spawn_neutrals(commands: &mut Commands, fonts: &Fonts, catalog: &Catalog, page: Entity) {
    let body = commands
        .spawn((
            TabPage(Tab::Neutrals),
            Node {
                display: Display::Grid,
                padding: UiRect::top(px(24)),
                justify_items: JustifyItems::Center,
                ..default()
            },
            ChildOf(page),
        ))
        .id();
    commands.spawn((
        ui::text(
            catalog.word(
                "dota_shop_neutraltiernotokensavailable",
                "No Neutral Tokens Available",
            ),
            &fonts.regular,
            12.0,
            ui::INK_DIM,
        ),
        ChildOf(body),
    ));
}

/// The search results: a title and a grid, filled as the search changes.
fn spawn_results(commands: &mut Commands, fonts: &Fonts, catalog: &Catalog, page: Entity) {
    let body = commands
        .spawn((
            Results,
            Node {
                flex_direction: FlexDirection::Column,
                display: Display::None,
                ..default()
            },
            ChildOf(page),
        ))
        .id();
    let heading = title(
        commands,
        fonts,
        body,
        catalog.word("dota_shop_search_results_title", "SEARCH RESULTS"),
    );
    commands.entity(heading).insert(ResultTitle);
    commands.spawn((ResultGrid, grid(RESULT_COLUMNS), ChildOf(body)));
}

/// The panel's foot: where to drop an item to sell it.
fn spawn_foot(commands: &mut Commands, fonts: &Fonts, panel: Entity) {
    let foot = commands
        .spawn((
            SellStrip,
            Button,
            Node {
                padding: UiRect::axes(px(PAD), px(6)),
                column_gap: px(6),
                align_items: AlignItems::Center,
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    commands
        .spawn((
            Node {
                width: px(14),
                height: px(14),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(Color::srgb(0.3, 0.33, 0.37)),
            Pickable::IGNORE,
            ChildOf(foot),
        ))
        .with_child((
            ui::text("?", &fonts.bold, 11.0, PANEL_FOOT),
            Pickable::IGNORE,
        ));
    commands.spawn((
        ui::text(
            "Drop an item here to sell it.",
            &fonts.regular,
            12.0,
            ui::INK_DIM,
        ),
        Pickable::IGNORE,
        ChildOf(foot),
    ));
}

/// `--shop TAB[:ITEM|:/SEARCH]`: the shop open at the start, for shots.
fn debug_open(
    args: Res<crate::Args>,
    mut done: Local<bool>,
    mut panels: ResMut<Panels>,
    mut shown: ResMut<Shown>,
    mut chosen: ResMut<Chosen>,
) {
    let Some(spec) = args.shop.as_deref().filter(|_| !*done) else {
        return;
    };
    *done = true;
    panels.shop = true;
    let (tab, rest) = spec.split_once(':').unwrap_or((spec, ""));
    shown.0 = match tab {
        "upgrades" => Tab::Upgrades,
        "neutrals" => Tab::Neutrals,
        _ => Tab::Basics,
    };
    match rest.strip_prefix('/') {
        Some(search) => panels.search = search.chars().take(24).collect(),
        None => chosen.0 = rest.parse().ok(),
    }
}

/// Shows the panel while the shop is open.
fn show(panels: Res<Panels>, mut panel: Single<&mut Visibility, With<ShopPanel>>) {
    panel.set_if_neq(ui::visible(panels.shop));
}

/// Keeps the shop panel clear of the bottom row and the shop's corner,
/// whatever their height.
fn above_bottom_row(
    row: Query<(&Extent, &ComputedNode)>,
    mut panel: Single<&mut Node, With<ShopPanel>>,
) {
    let height = row
        .iter()
        .filter(|(e, _)| matches!(**e, Extent::Bar | Extent::Right))
        .map(|(_, node)| node.size().y * node.inverse_scale_factor())
        .fold(0.0, f32::max);
    let bottom = px(height + 6.0);
    if panel.bottom != bottom {
        panel.bottom = bottom;
    }
}

/// The tab shown, or the results while a search is typed.
fn tabs(
    shown: Res<Shown>,
    panels: Res<Panels>,
    mut buttons: Query<(&TabButton, &Interaction, &mut BackgroundColor, &Children)>,
    mut labels: Query<&mut TextColor>,
    mut pages: Query<(&TabPage, &mut Node), Without<Results>>,
    mut results: Single<&mut Node, With<Results>>,
) {
    let searching = !panels.search.trim().is_empty();
    for (TabButton(tab), interaction, mut ground, children) in &mut buttons {
        let on = *tab == shown.0 && !searching;
        let lit = on || *interaction != Interaction::None;
        ground.set_if_neq(BackgroundColor(if on { TAB_ON } else { TAB_OFF }));
        for child in children {
            if let Ok(mut color) = labels.get_mut(*child) {
                color.set_if_neq(TextColor(if lit { ui::INK } else { ui::INK_DIM }));
            }
        }
    }
    for (TabPage(tab), mut node) in &mut pages {
        let display = if *tab == shown.0 && !searching {
            Display::Grid
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let display = if searching {
        Display::Flex
    } else {
        Display::None
    };
    if results.display != display {
        results.display = display;
    }
}

/// The results of the search typed, rebuilt as it changes.
fn results(
    mut commands: Commands,
    panels: Res<Panels>,
    mut last: Local<Option<String>>,
    catalog: Res<Catalog>,
    texts: Res<Texts>,
    manifest: Res<Manifest>,
    fonts: Res<Fonts>,
    server: Res<AssetServer>,
    grid: Single<Entity, With<ResultGrid>>,
    mut heading: Single<&mut Text, With<ResultTitle>>,
) {
    if last.as_deref() == Some(panels.search.as_str()) && !catalog.is_changed() {
        return;
    }
    *last = Some(panels.search.clone());
    let look = Lookup {
        texts: &texts,
        manifest: &manifest,
    };
    let found = catalog.search(&look, &panels.search);
    commands.entity(*grid).despawn_children();
    for id in &found {
        cell::spawn(
            &mut commands,
            (&fonts, &manifest, &server),
            *grid,
            *id,
            Place::Grid,
            CELL,
        );
    }
    let words = if found.is_empty() {
        catalog.word("dota_shop_search_no_results", "No matches were found.")
    } else {
        catalog.word("dota_shop_search_results_title", "SEARCH RESULTS")
    };
    heading.set_if_neq(Text(words.to_uppercase()));
}

/// The search box's text: what is typed, with a caret while typing, or the
/// word for search.
fn search_text(
    panels: Res<Panels>,
    typing: Res<Typing>,
    catalog: Res<Catalog>,
    mut text: Single<(&mut Text, &mut TextColor), With<SearchText>>,
) {
    let typing = *typing == Typing::Search;
    let (s, color) = match (typing, panels.search.is_empty()) {
        (true, _) => (format!("{}_", panels.search), ui::INK),
        (false, true) => (
            catalog.word("dota_search", "Search").to_string(),
            ui::INK_DIM,
        ),
        (false, false) => (panels.search.clone(), ui::INK),
    };
    text.0.set_if_neq(Text(s));
    text.1.set_if_neq(TextColor(color));
}

/// The wheel scrolls the page while the cursor is over the panel.
fn scroll(
    wheel: Res<AccumulatedMouseScroll>,
    panel: Single<&Interaction, With<ShopPanel>>,
    mut page: Single<&mut ScrollPosition, With<Page>>,
) {
    if **panel == Interaction::None || wheel.delta.y == 0.0 {
        return;
    }
    let step = match wheel.unit {
        bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y * 40.0,
        bevy::input::mouse::MouseScrollUnit::Pixel => wheel.delta.y,
    };
    page.y = (page.y - step).max(0.0);
}
