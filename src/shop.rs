//! The shop, Dota's way: the gold and the shop button in the corner, a panel
//! of item icons by group with the price under each, a search box, buying on
//! a click, the quick-buy item on a right click, and selling by dropping an
//! item on the panel's foot.

use bevy::{input::mouse::AccumulatedMouseScroll, prelude::*};
use bota_client_core::{ITEMS, Intent, SHOP_GROUPS, answers, group_of};

use crate::config::{Action, Settings, key_name};
use crate::hud::Blocker;
use crate::input::{Panels, Typing};
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// The shop panel.
#[derive(Component)]
struct ShopPanel;

/// The scrolling grid inside the shop panel.
#[derive(Component)]
struct ShopGrid;

/// An item's cell in the shop, by item id.
#[derive(Component, Clone, Copy)]
pub struct ShopCell(pub u16);

/// The price under a cell.
#[derive(Component)]
struct Price(u16);

/// A group's heading and row in the shop, by group.
#[derive(Component)]
struct GroupBlock(usize);

/// The shop button in the corner.
#[derive(Component)]
struct ShopButton;

/// The search box.
#[derive(Component)]
struct SearchBox;

/// The sell strip at the panel's foot.
#[derive(Component)]
pub struct SellStrip;

/// A text of the shop kept up to date.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum ShopText {
    Gold,
    Search,
    Quick,
    Courier,
}

/// The quick-buy strip and the courier button over the shop button.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum CornerButton {
    Quick,
    Courier,
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(
        Update,
        (show, prices, filter, clicks, scroll, texts)
            .run_if(|k: Res<crate::session::MatchKnown>| k.0),
    );
}

fn spawn(
    mut commands: Commands,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    settings: Res<Settings>,
) {
    let corner = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(8),
                bottom: px(8),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: px(6),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .id();
    for (button, text) in [
        (CornerButton::Courier, ShopText::Courier),
        (CornerButton::Quick, ShopText::Quick),
    ] {
        let b = commands
            .spawn((
                button,
                Button,
                Blocker,
                Node {
                    padding: UiRect::axes(px(10), px(5)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(ui::PANEL),
                BorderColor::all(ui::EDGE),
                ChildOf(corner),
            ))
            .id();
        commands.spawn((text, ui::text("", &fonts.bold, 13.0, ui::INK), ChildOf(b)));
    }
    let button = commands
        .spawn((
            ShopButton,
            Button,
            Blocker,
            Node {
                width: px(190),
                height: px(48),
                padding: UiRect::horizontal(px(12)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(ui::PANEL),
            BorderColor::all(Color::srgb(0.45, 0.36, 0.15)),
            ChildOf(corner),
        ))
        .id();
    if let Some(coin) = manifest.file("hud.gold") {
        commands.spawn((
            ImageNode::new(server.load(coin)),
            Node {
                width: px(26),
                height: px(26),
                ..default()
            },
            ChildOf(button),
        ));
    }
    commands.spawn((
        ShopText::Gold,
        ui::text("0", &fonts.bold, 24.0, ui::GOLDEN),
        ChildOf(button),
    ));
    commands.spawn((
        ui::text(
            format!("SHOP [{}]", key_name(settings.key(Action::Shop))),
            &fonts.bold,
            11.0,
            ui::INK_DIM,
        ),
        ChildOf(button),
    ));
    spawn_panel(&mut commands, &fonts, &manifest, &server);
}

fn spawn_panel(commands: &mut Commands, fonts: &Fonts, manifest: &Manifest, server: &AssetServer) {
    let panel = commands
        .spawn((
            ShopPanel,
            Blocker,
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                right: px(8),
                top: px(70),
                bottom: px(150),
                width: px(392),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(10)),
                row_gap: px(8),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.05, 0.07, 0.97)),
            BorderColor::all(ui::EDGE),
            Visibility::Hidden,
        ))
        .id();
    let search = commands
        .spawn((
            SearchBox,
            Button,
            Node {
                height: px(30),
                padding: UiRect::horizontal(px(8)),
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            BorderColor::all(ui::EDGE),
            ChildOf(panel),
        ))
        .id();
    commands.spawn((
        ShopText::Search,
        ui::text("", &fonts.regular, 14.0, ui::INK_DIM),
        ChildOf(search),
    ));
    let grid = commands
        .spawn((
            ShopGrid,
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                row_gap: px(6),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
            ChildOf(panel),
        ))
        .id();
    for (group, name) in SHOP_GROUPS.iter().enumerate() {
        let block = commands
            .spawn((
                GroupBlock(group),
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    ..default()
                },
                ChildOf(grid),
            ))
            .id();
        commands.spawn((
            ui::text(name.to_ascii_uppercase(), &fonts.bold, 11.0, ui::GOLDEN),
            ChildOf(block),
        ));
        let row = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(5),
                    row_gap: px(5),
                    ..default()
                },
                ChildOf(block),
            ))
            .id();
        for face in ITEMS.iter().filter(|f| group_of(f.id) == group) {
            let cell = commands
                .spawn((
                    ShopCell(face.id),
                    Button,
                    Node {
                        width: px(68),
                        height: px(64),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.07, 0.08, 0.11)),
                    BorderColor::all(Color::srgb(0.15, 0.17, 0.21)),
                    ChildOf(row),
                ))
                .id();
            let icon = manifest
                .item_icon(face.id)
                .map(|p| server.load(p))
                .unwrap_or_default();
            commands.spawn((
                ImageNode::new(icon),
                Node {
                    width: px(64),
                    height: px(46),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(cell),
            ));
            commands.spawn((
                Price(face.id),
                ui::text("", &fonts.bold, 12.0, ui::GOLDEN),
                Pickable::IGNORE,
                ChildOf(cell),
            ));
            // What the side's shop has left, for an item of a limited stock.
            commands.spawn((
                Stock(face.id),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(3),
                    top: px(1),
                    ..default()
                },
                ui::text("", &fonts.bold, 13.0, ui::INK),
                ui::shadow(),
                Pickable::IGNORE,
                ChildOf(cell),
            ));
        }
    }
    let sell = commands
        .spawn((
            SellStrip,
            Button,
            Node {
                height: px(32),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4)),
            BorderColor::all(ui::EDGE),
            ChildOf(panel),
        ))
        .id();
    commands.spawn((
        ui::text(
            "drop an item here to sell it   right-click: quick-buy",
            &fonts.regular,
            12.0,
            ui::INK_DIM,
        ),
        ChildOf(sell),
    ));
}

/// Shows the panel while the shop is open.
fn show(panels: Res<Panels>, mut panel: Single<&mut Visibility, With<ShopPanel>>) {
    let want = if panels.shop {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if **panel != want {
        **panel = want;
    }
}

/// What the side's shop has left of a cell's item, over its icon.
#[derive(Component)]
struct Stock(u16);

/// The price of every cell for this seat, dimmed when out of reach; the
/// stock left of an item sold from a limited one, and the wait for the next
/// when there is none.
fn prices(
    session: NonSend<Session>,
    mut texts: Query<(&Price, &mut Text, &mut TextColor), Without<Stock>>,
    mut stocks: Query<(&Stock, &mut Text, &mut TextColor), Without<Price>>,
    panels: Res<Panels>,
) {
    if !panels.shop {
        return;
    }
    let core = &session.0;
    let gold = core
        .view
        .as_ref()
        .and_then(|v| v.players.iter().find(|p| Some(p.slot) == core.my_slot))
        .and_then(|p| p.gold)
        .unwrap_or(0);
    let rate = u32::from(core.tick_rate.max(1));
    for (stock, mut text, mut color) in &mut stocks {
        let (s, c) = match core.stock_of(stock.0) {
            Some(st) if st.left > 0 => (st.left.to_string(), ui::INK),
            Some(st) => (
                format!("0  {}s", st.restock_left.div_ceil(rate)),
                Color::srgb(1.0, 0.45, 0.35),
            ),
            None => (String::new(), ui::INK),
        };
        if text.0 != s {
            text.0 = s;
        }
        if color.0 != c {
            color.0 = c;
        }
    }
    for (price, mut text, mut color) in &mut texts {
        let (now, _) = core.price_of(price.0);
        let s = now.to_string();
        if text.0 != s {
            text.0 = s;
        }
        let c = if gold >= now {
            ui::GOLDEN
        } else {
            Color::srgb(0.55, 0.45, 0.3)
        };
        if color.0 != c {
            color.0 = c;
        }
    }
}

/// Hides the cells and groups the search does not answer.
fn filter(
    panels: Res<Panels>,
    mut cells: Query<(&ShopCell, &mut Node), Without<GroupBlock>>,
    mut blocks: Query<(&GroupBlock, &mut Node), Without<ShopCell>>,
) {
    if !panels.is_changed() {
        return;
    }
    let mut groups = [false; SHOP_GROUPS.len()];
    for (cell, mut node) in &mut cells {
        let face = ITEMS.iter().find(|f| f.id == cell.0);
        let shown = face.is_some_and(|f| answers(f, &panels.search));
        if shown {
            groups[group_of(cell.0)] = true;
        }
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for (block, mut node) in &mut blocks {
        let display = if groups[block.0] {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

/// Clicks on the shop and its buttons.
fn clicks(
    mut session: NonSendMut<Session>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut panels: ResMut<Panels>,
    mut typing: ResMut<Typing>,
    cells: Query<(&Interaction, &ShopCell)>,
    shop_button: Single<&Interaction, With<ShopButton>>,
    search: Single<&Interaction, With<SearchBox>>,
    sell: Single<&Interaction, With<SellStrip>>,
    corner: Query<(&Interaction, &CornerButton)>,
) {
    let core = &mut session.0;
    let live = matches!(core.source, bota_client_core::Source::Live(_));
    let left = buttons.just_pressed(MouseButton::Left);
    let right = buttons.just_pressed(MouseButton::Right);
    if left && **shop_button != Interaction::None {
        panels.shop = !panels.shop;
    }
    if left && **search != Interaction::None && panels.shop {
        *typing = Typing::Search;
    } else if left && *typing == Typing::Search {
        *typing = Typing::No;
    }
    for (interaction, button) in &corner {
        if left && *interaction != Interaction::None && live {
            match button {
                CornerButton::Quick => core.act(Intent::QuickBuy),
                CornerButton::Courier => core.act(Intent::CourierDeliver),
            }
        }
    }
    if !panels.shop {
        return;
    }
    if let Some((_, cell)) = cells.iter().find(|(i, _)| **i != Interaction::None) {
        if left && live {
            core.act(Intent::Buy(cell.0));
        } else if right {
            core.quick_buy = if core.quick_buy == Some(cell.0) {
                None
            } else {
                Some(cell.0)
            };
        }
    }
    if buttons.just_released(MouseButton::Left)
        && **sell != Interaction::None
        && let Some(slot) = core.held_item.take()
        && live
    {
        core.act(Intent::Sell(slot));
    }
}

/// The wheel scrolls the shop grid while the cursor is over it.
fn scroll(
    wheel: Res<AccumulatedMouseScroll>,
    panel: Single<&Interaction, With<ShopPanel>>,
    mut grid: Single<&mut ScrollPosition, With<ShopGrid>>,
) {
    if **panel == Interaction::None || wheel.delta.y == 0.0 {
        return;
    }
    let step = match wheel.unit {
        bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y * 40.0,
        bevy::input::mouse::MouseScrollUnit::Pixel => wheel.delta.y,
    };
    grid.y = (grid.y - step).max(0.0);
}

/// The gold, the search line, the quick-buy item and the courier button.
fn texts(
    session: NonSend<Session>,
    panels: Res<Panels>,
    typing: Res<Typing>,
    settings: Res<Settings>,
    mut texts: Query<(&ShopText, &mut Text)>,
    mut corner: Query<(&CornerButton, &mut Visibility)>,
) {
    let core = &session.0;
    let me = core
        .view
        .as_ref()
        .and_then(|v| v.players.iter().find(|p| Some(p.slot) == core.my_slot));
    let quick = core
        .quick_buy
        .and_then(|id| ITEMS.iter().find(|f| f.id == id));
    let stash_full = me
        .and_then(|p| p.stash.as_ref())
        .is_some_and(|s| s.iter().any(Option::is_some));
    for (which, mut text) in &mut texts {
        let s = match which {
            ShopText::Gold => me
                .and_then(|p| p.gold)
                .map_or("-".to_string(), |g| g.to_string()),
            ShopText::Search => match (*typing == Typing::Search, panels.search.is_empty()) {
                (true, _) => format!("{}_", panels.search),
                (false, true) => "Search (click here)".to_string(),
                (false, false) => panels.search.clone(),
            },
            ShopText::Quick => quick.map_or(String::new(), |f| {
                let missing: Vec<&str> = core
                    .missing_for(f.id)
                    .iter()
                    .filter_map(|p| ITEMS.iter().find(|i| i.id == p.0).map(|i| i.name))
                    .collect();
                format!(
                    "Quick-buy {} [{}]: {}",
                    f.name,
                    key_name(settings.key(Action::QuickBuy)),
                    missing.join(", ")
                )
            }),
            ShopText::Courier => format!(
                "Courier: deliver [{}]",
                key_name(settings.key(Action::CourierDeliver))
            ),
        };
        if text.0 != s {
            text.0 = s;
        }
    }
    for (button, mut vis) in &mut corner {
        let show = match button {
            CornerButton::Quick => quick.is_some(),
            CornerButton::Courier => stash_full && !core.at_home_shop(),
        };
        let want = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
    }
}
