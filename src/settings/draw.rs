//! Drawing the settings screen, a window over the dimmed screen: its side navigation (the search
//! field, a tab per row, the open tab's sections), the open tab's title and close cross, and the
//! tab's sections as cards of rows. The layout is built again when the screen's state changes; the
//! values in it are kept up to date in place (`looks`), so a slider stays held while it is dragged.

use bevy::picking::hover::{DirectlyHovered, Hovered};
use bevy::ui_widgets::Button;
use bevy::{prelude::*, ui::RelativeCursorPosition};

use super::model::{self, Choice, Row, Section, Slider, Tab, TextSetting, Toggle};
use super::screen::{InView, Jump, Screen, Scroll};
use crate::config::Settings;
use crate::hud::Blocker;
use crate::keys;
use crate::manifest::Manifest;
use crate::theme::{self, Face, Type};
use crate::ui::{self, Fonts};
use crate::window::Displays;

/// The screen's layer.
#[derive(Component)]
pub struct Root;

/// The list at the left and the header, which follow the tab in view and are built again when it
/// changes; the body, which does not depend on it, is kept.
#[derive(Component)]
pub struct NavRoot;
#[derive(Component)]
pub struct HeaderRoot;

/// The scrolling part under the header.
#[derive(Component)]
pub struct Body;

/// The rail of the scroll bar and its thumb.
#[derive(Component)]
pub struct ScrollRail;
#[derive(Component)]
pub struct ScrollThumb;

#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct TabButton(pub Tab);

/// A link of the navigation to the card at this place of the open view, and where a click on it
/// scrolls to: the card, or the start of its tab when it is the tab's first.
#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct NavLink {
    pub card: usize,
    pub jump: Jump,
}

/// A card of the open view, by its place; the navigation scrolls to it.
#[derive(Component)]
pub struct CardAnchor(pub usize);

/// The tab a card belongs to.
#[derive(Component)]
pub struct CardTab(pub Tab);

/// Where a tab starts in the body, which a click on the tab scrolls to.
#[derive(Component)]
pub struct TabAnchor(pub Tab);

/// A slider drawn upright, as a mixer's fader: its top is its most.
#[derive(Component)]
pub struct Vertical;

/// A mixer strip's mute button, over the toggle it turns.
#[derive(Component)]
pub struct MuteBox(pub Toggle);

#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct CloseButton;

/// The search field, its text and its clear button.
#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct SearchField;
#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct SearchClear;

/// The hotkeys' reset button.
#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct ResetButton;

/// A row's reset arrow and the mark of a changed row.
#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct RowReset(pub Row);
#[derive(Component)]
pub struct RowBar(pub Row);

/// A row's label in pieces (before the match, the match, after it).
#[derive(Component)]
pub struct LabelPart(pub Row, pub bool);

/// A toggle's row, the box that shows it on or off and the tick in it.
#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct ToggleRow(pub Toggle);
#[derive(Component)]
pub struct ToggleBox(pub Toggle);
#[derive(Component)]
pub struct ToggleMark(pub Toggle);

/// A slider's track, its filled part, its thumb and its value.
#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct SliderTrack(pub Slider);
#[derive(Component)]
pub struct SliderFill(pub Slider);
#[derive(Component)]
pub struct SliderThumb(pub Slider);
#[derive(Component)]
pub struct SliderValue(pub Slider);

/// A choice's button, its value, and an option of its open list.
#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct ChoiceButton(pub Choice);
#[derive(Component)]
pub struct ChoiceValue(pub Choice);
#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct ChoiceOption(pub Choice, pub usize);
/// The open list of a choice: below its button, or above it where the window has no room below.
#[derive(Component)]
pub struct OptionsList;

/// A binding's key field or HUD slot, by its place in [`keys::BINDINGS`]; a click captures its key.
#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct KeyField(pub usize);
/// The field of a row, as against a slot.
#[derive(Component)]
pub struct RowKey;
#[derive(Component)]
pub struct KeyText(pub usize);
/// A slot's key badge and its text.
#[derive(Component)]
pub struct SlotBadge(pub usize);
#[derive(Component)]
pub struct SlotKey(pub usize);

/// A typed line's field, its text, its caret and its counter.
#[derive(Component)]
#[require(bevy::ui_widgets::Button, DirectlyHovered, Hovered)]
pub struct NameField(pub TextSetting);
#[derive(Component)]
pub struct NameText(pub TextSetting);
#[derive(Component)]
pub struct NameCaret(pub TextSetting);
#[derive(Component)]
pub struct NameCount(pub TextSetting);

/// The bar telling which bindings clash, and its text.
#[derive(Component)]
pub struct ClashBar;
#[derive(Component)]
pub struct ClashText;

/// The window over the dimmed screen, as Dota's: under the top bar, the side navigation at its
/// left and the tab's cards at its right.
/// The window's proportions, measured on the original game's settings window as shares of the
/// screen's height (it is 1.205 high wide and 0.942 high, from 0.022 below the top, its list 0.30
/// wide): shares of the window's height, so they hold whatever the interface's scale is, in a
/// menu or over a game.
const WINDOW_WIDTH: f32 = 120.5;
const WINDOW_TOP: f32 = 2.2;
const WINDOW_FOOT: f32 = 3.6;
const NAV_WIDTH: f32 = 30.0;
const HEADER_HEIGHT: f32 = 76.0;
const NAV_TAB_HEIGHT: f32 = 48.0;
const NAV_LINK_HEIGHT: f32 = 30.0;
/// The window's close cross, across.
const CLOSE_ICON: f32 = 20.0;
/// How wide the cards are at most.
const CARD_WIDTH: f32 = 700.0;
const ROW_HEIGHT: f32 = 48.0;
const KEY_ROW_HEIGHT: f32 = 40.0;
const FIELD_WIDTH: f32 = 220.0;
const FIELD_HEIGHT: f32 = 38.0;
const SLIDER_WIDTH: f32 = 200.0;
/// The mixer's strips: how wide each is, and its fader's height, width and cap.
const STRIP_WIDTH: f32 = 92.0;
const FADER_HEIGHT: f32 = 150.0;
pub const FADER_WIDTH: f32 = 40.0;
const FADER_CAP: (f32, f32) = (32.0, 14.0);
const MUTE_SIZE: (f32, f32) = (56.0, 28.0);
const CLASH_BAR_HEIGHT: f32 = 44.0;
/// The scroll bar's room at the body's right.
const SCROLL_GUTTER: f32 = 14.0;
pub const THUMB: f32 = 16.0;
pub const THUMB_HELD: f32 = 22.0;
/// A fader's cap while held: larger, as a slider's thumb.
pub const CAP_HELD: (f32, f32) = (36.0, 18.0);
pub const CAP: (f32, f32) = FADER_CAP;

/// Slots of the HUD at the top of the hotkeys: ability squares, item squares, the town portal
/// and the neutral item's round one.
const ABILITY_SLOT: (f32, f32) = (64.0, 64.0);
const ITEM_SLOT: (f32, f32) = (54.0, 40.0);
const PORTAL_SLOT: (f32, f32) = (46.0, 40.0);
const NEUTRAL_SLOT: f32 = 44.0;

pub fn spawn_root(mut commands: Commands) {
    commands.spawn((
        Root,
        Blocker,
        Hovered::default(),
        DirectlyHovered::default(),
        Node {
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Stretch,
            ..ui::fill()
        },
        BackgroundColor(theme::scrim()),
        GlobalZIndex(65),
        Visibility::Hidden,
    ));
}

/// What the view has to show: the cards, and for each the title it has.
pub struct Cards {
    pub title: &'static str,
    pub cards: Vec<Card>,
}

pub struct Card {
    pub title: String,
    pub tab: Tab,
    pub section: Section,
}

/// The cards the screen shows now: a search's matches, or the sections of every tab one after
/// the other, the tab in view named in the title.
pub fn cards(screen: &Screen) -> Cards {
    if screen.is_searching() {
        let cards = model::search(&screen.query)
            .into_iter()
            .map(|found| Card {
                title: format!("{} · {}", found.tab.title(), found.section.title),
                tab: found.tab,
                section: found.section,
            })
            .collect();
        return Cards {
            title: "SEARCH",
            cards,
        };
    }
    let cards = Tab::ALL
        .into_iter()
        .flat_map(|tab| {
            model::sections(tab, false)
                .into_iter()
                .map(move |section| Card {
                    title: section.title.to_owned(),
                    tab,
                    section,
                })
        })
        .collect();
    Cards {
        title: screen.tab.title(),
        cards,
    }
}

/// What spawning the screen needs.
struct Kit<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    fonts: &'a Fonts,
    manifest: &'a Manifest,
    server: &'a AssetServer,
}

impl Kit<'_, '_, '_> {
    fn visual(&mut self) -> crate::ui::Draw<'_, '_, '_> {
        crate::ui::Draw {
            commands: self.commands.reborrow(),
            fonts: self.fonts,
        }
    }
    fn typed(&mut self, parent: Entity, s: impl Into<String>, role: Type, color: Color) -> Entity {
        self.visual().typed(parent, s, role, color)
    }

    fn node(&mut self, parent: Entity, node: Node) -> Entity {
        self.visual().node(parent, node)
    }

    /// Dota's icon `logical` tinted `color`, `size` across; none where the cache has none.
    fn icon(&mut self, parent: Entity, logical: &str, size: f32, color: Color) -> Option<Entity> {
        let path = self.manifest.file(logical)?;
        Some(
            self.commands
                .spawn((
                    Node {
                        width: px(size),
                        height: px(size),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    ImageNode::new(self.server.load(path)).with_color(color),
                    Pickable::IGNORE,
                    ChildOf(parent),
                ))
                .id(),
        )
    }

    /// A text of `role` in pieces: before, match and after, each drawn in its own colour.
    fn label(&mut self, parent: Entity, row: Row, query: &str, role: Type) -> Entity {
        let label = row.label();
        let (before, hit, after) = model::split_match(label, query).unwrap_or((label, "", ""));
        let root = self
            .commands
            .spawn((
                theme::text(before, self.fonts, role, theme::ROW_INK),
                LabelPart(row, false),
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id();
        let bold = Type {
            face: Face::Bold,
            ..role
        };
        for (piece, face, hit) in [(hit, bold, true), (after, role, false)] {
            let font = TextFont {
                font: self.fonts.face(face.face).clone().into(),
                font_size: FontSize::Px(face.size),
                ..default()
            };
            let ink = if hit { theme::SELECT } else { theme::ROW_INK };
            self.commands.spawn((
                TextSpan::new(piece),
                font,
                TextColor(ink),
                LabelPart(row, hit),
                ChildOf(root),
            ));
        }
        root
    }
}

/// Builds the screen again for its state.
#[allow(clippy::too_many_arguments)]
pub fn layout(
    mut commands: Commands,
    screen: Res<Screen>,
    scroll: Res<Scroll>,
    in_view: Res<InView>,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    settings: Res<Settings>,
    displays: Res<Displays>,
    session: NonSend<crate::session::Session>,
    bag: Res<crate::bag::TacticalBag>,
    root: Single<Entity, With<Root>>,
    chrome: Query<(Entity, Has<NavRoot>), Or<(With<NavRoot>, With<HeaderRoot>)>>,
    mut before: Local<Option<Screen>>,
) {
    if !screen.is_changed() || !screen.open {
        return;
    }
    // Scrolling into another tab changes only what the list and the header say of it: they are
    // built again and the body's thousands of nodes stay.
    let only_the_tab = before.as_ref().is_some_and(|was| {
        let mut was = was.clone();
        was.tab = screen.tab;
        was == *screen
    }) && chrome.iter().count() == 2;
    *before = Some((*screen).clone());
    if only_the_tab {
        let view = cards(&screen);
        let mut kit = Kit {
            commands: &mut commands,
            fonts: &fonts,
            manifest: &manifest,
            server: &server,
        };
        for (entity, is_nav) in &chrome {
            kit.commands.entity(entity).despawn_children();
            if is_nav {
                nav_contents(&mut kit, entity, &screen, &view, in_view.0);
            } else {
                header_contents(&mut kit, entity, &screen, view.title);
            }
        }
        return;
    }
    commands.entity(*root).despawn_children();
    let icons = if !screen.is_searching() {
        crate::hud::slot_icons(&session.0, &bag, &manifest)
    } else {
        Vec::new()
    };
    let mut kit = Kit {
        commands: &mut commands,
        fonts: &fonts,
        manifest: &manifest,
        server: &server,
    };
    let view = cards(&screen);
    let window = window_frame(&mut kit, *root);
    navigation(&mut kit, window, &screen, &view, in_view.0);
    let content = kit.node(
        window,
        Node {
            flex_grow: 1.0,
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    header(&mut kit, content, &screen, view.title);
    let (area, body) = scroll_area(&mut kit, content, scroll.y);
    scroll_bar(&mut kit, area);
    let on = Shown {
        screen: &screen,
        settings: &settings,
        displays: &displays,
        icons: &icons,
    };
    fill_body(&mut kit, body, &view, &on);
    clash_bar(&mut kit, content);
}

/// The window itself: its frame and ground over the dimmed screen.
fn window_frame(kit: &mut Kit, root: Entity) -> Entity {
    kit.commands
        .spawn((
            Node {
                width: vh(WINDOW_WIDTH),
                max_width: percent(100),
                margin: UiRect::new(px(0), px(0), vh(WINDOW_TOP), vh(WINDOW_FOOT)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::WINDOW),
            BorderColor::all(theme::WINDOW_EDGE),
            theme::window_shadow(),
            Name::new("settings window"),
            ChildOf(root),
        ))
        .id()
}

/// The room under the header: the scrolling body, kept at `scrolled`, and the area it fills.
fn scroll_area(kit: &mut Kit, content: Entity, scrolled: f32) -> (Entity, Entity) {
    let area = kit.node(
        content,
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            ..default()
        },
    );
    let body = kit
        .commands
        .spawn((
            Body,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                bottom: px(0),
                right: px(SCROLL_GUTTER),
                flex_direction: FlexDirection::Column,
                padding: UiRect::new(px(40), px(26), px(24), px(24)),
                row_gap: px(16),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition(Vec2::new(0.0, scrolled)),
            ChildOf(area),
        ))
        .id();
    (area, body)
}

/// What the screen is drawn from, apart from its view of cards.
struct Shown<'a> {
    screen: &'a Screen,
    settings: &'a Settings,
    displays: &'a Displays,
    /// The art of the slots at the top of the hotkeys.
    icons: &'a [(keys::Action, String)],
}

/// The body: the cards of every tab one after the other, each tab started by its heading (the
/// hotkeys by their slots and ended by their reset), or why a search found none.
fn fill_body(kit: &mut Kit, body: Entity, view: &Cards, on: &Shown) {
    let searching = on.screen.is_searching();
    if searching && view.cards.is_empty() {
        let line = format!("No setting is called “{}”", on.screen.query.trim());
        kit.typed(body, line, theme::BODY, theme::INK_4);
    }
    let mut tab = None;
    for (at, card) in view.cards.iter().enumerate() {
        if !searching && tab != Some(card.tab) {
            tab_start(kit, body, card.tab, tab.is_none(), on.icons);
            tab = Some(card.tab);
        }
        let ctx = RowCtx {
            screen: on.screen,
            settings: on.settings,
            displays: on.displays,
            dense: card.tab == Tab::Hotkeys,
        };
        card_view(kit, body, at, card, &ctx);
    }
}

/// Where a tab starts: its heading (the first tab's is the window's header) and, for the
/// hotkeys, the slots of the HUD. The navigation scrolls to it.
fn tab_start(kit: &mut Kit, body: Entity, tab: Tab, first: bool, icons: &[(keys::Action, String)]) {
    let start = kit.node(
        body,
        Node {
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(16),
            ..default()
        },
    );
    kit.commands.entity(start).insert(TabAnchor(tab));
    if !first {
        let heading = kit.node(
            start,
            Node {
                max_width: px(CARD_WIDTH),
                margin: UiRect::top(px(28)),
                padding: UiRect::bottom(px(10)),
                border: UiRect::bottom(px(1)),
                ..default()
            },
        );
        kit.commands
            .entity(heading)
            .insert(BorderColor::all(theme::DIVIDER));
        let name = Type {
            size: 15.0,
            tracking: 3.0,
            ..theme::HEADING
        };
        kit.typed(heading, tab.title(), name, theme::INK_4);
    }
    if tab == Tab::Hotkeys {
        hud_panel(kit, start, icons);
    }
}

/// What drawing a row depends on.
struct RowCtx<'a> {
    screen: &'a Screen,
    settings: &'a Settings,
    displays: &'a Displays,
    /// The hotkeys' rows are lower.
    dense: bool,
}

/// The window's left column: SETTINGS, the search, a tab per row (the open one lit, with its
/// sections under it), the version line.
fn navigation(kit: &mut Kit, window: Entity, screen: &Screen, view: &Cards, in_view: usize) {
    let nav = kit
        .commands
        .spawn((
            Node {
                width: vh(NAV_WIDTH),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                padding: UiRect::top(px(20)),
                ..default()
            },
            theme::downward(theme::WINDOW_NAV),
            Pickable::IGNORE,
            NavRoot,
            ChildOf(window),
        ))
        .id();
    nav_contents(kit, nav, screen, view, in_view);
}

/// What the list at the left holds: SETTINGS, the search, a tab per row and the version line.
fn nav_contents(kit: &mut Kit, nav: Entity, screen: &Screen, view: &Cards, in_view: usize) {
    let title_role = Type {
        size: 26.0,
        tracking: 3.0,
        ..theme::TITLE
    };
    let title = kit.typed(nav, "SETTINGS", title_role, theme::INK_1);
    kit.commands.entity(title).insert(Node {
        margin: UiRect::new(px(20), px(20), px(2), px(12)),
        ..default()
    });
    search_field(kit, nav, screen);
    for tab in Tab::ALL {
        nav_section(kit, nav, tab, screen, view, in_view);
    }
    kit.node(
        nav,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    version_line(kit, nav);
}

/// A tab of the navigation and the links under it: its sections when it is open, its hits (with
/// their counts) while a search runs, none otherwise.
fn nav_section(
    kit: &mut Kit,
    nav: Entity,
    tab: Tab,
    screen: &Screen,
    view: &Cards,
    in_view: usize,
) {
    let searching = screen.is_searching();
    let hits: Vec<(usize, &Card)> = view
        .cards
        .iter()
        .enumerate()
        .filter(|(_, card)| card.tab == tab)
        .collect();
    let lit = !searching && tab == screen.tab;
    let tab_box = kit.node(
        nav,
        Node {
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    nav_tab(kit, tab_box, tab, lit, searching && hits.is_empty());
    let links: Vec<(usize, String)> = if searching {
        hits.iter()
            .map(|(at, card)| {
                let count = card.section.rows.len();
                (*at, format!("{} · {count}", card.section.link))
            })
            .collect()
    } else if lit {
        hits.iter()
            .map(|(at, card)| (*at, card.section.link.to_owned()))
            .collect()
    } else {
        Vec::new()
    };
    if links.is_empty() {
        return;
    }
    let list = kit.node(
        tab_box,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::new(px(26), px(0), px(0), px(4)),
            ..default()
        },
    );
    let first = hits.first().map(|(at, _)| *at);
    for (at, label) in links {
        // The first section of a tab is its start: the slots above it count.
        let jump = if !searching && Some(at) == first {
            Jump::Tab(tab)
        } else {
            Jump::Card(at)
        };
        nav_link(kit, list, (at, jump), &label, searching || at == in_view);
    }
}

/// Which client runs and which assets it reads, small at the foot of the navigation: the build's
/// commit and the asset fingerprint a cache must carry for it.
fn version_line(kit: &mut Kit, nav: Entity) {
    let text = format!(
        "{}  ·  assets {}",
        crate::version(),
        dbota_import::import::fingerprint()
    );
    let role = Type {
        size: 11.0,
        figures: true,
        ..theme::BODY
    };
    let line = kit.typed(nav, text, role, theme::INK_6);
    kit.commands.entity(line).insert(Node {
        margin: UiRect::new(px(20), px(12), px(0), px(14)),
        ..default()
    });
}

/// The field that finds a setting by its label, with its clear button once something is typed.
fn search_field(kit: &mut Kit, nav: Entity, screen: &Screen) {
    let lit = screen.searching || screen.is_searching();
    let field = kit
        .commands
        .spawn((
            SearchField,
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            Node {
                margin: UiRect::new(px(16), px(16), px(0), px(12)),
                height: px(44.0),
                padding: UiRect::new(px(34), px(40), px(0), px(0)),
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::FIELD_SUNKEN),
            BorderColor::all(if lit { theme::FOCUS } else { theme::FIELD_EDGE }),
            Name::new("search settings"),
            ChildOf(nav),
        ))
        .id();
    if let Some(icon) = kit.icon(field, "menu.search", 16.0, theme::INK_5) {
        kit.commands.entity(icon).insert(Node {
            position_type: PositionType::Absolute,
            left: px(11),
            top: px(13),
            width: px(16),
            height: px(16),
            ..default()
        });
    }
    let (words, ink) = match (screen.query.is_empty(), screen.searching) {
        (true, false) => ("Search settings".to_owned(), theme::INK_5),
        (_, true) => (format!("{}|", screen.query), theme::INK_1),
        (false, false) => (screen.query.clone(), theme::INK_1),
    };
    let text = kit.typed(field, words, theme::BODY, ink);
    kit.commands.entity(text).insert(TextLayout::no_wrap());
    if screen.query.is_empty() {
        return;
    }
    let clear = kit
        .commands
        .spawn((
            SearchClear,
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            Node {
                position_type: PositionType::Absolute,
                right: px(3),
                top: px(3),
                width: px(36),
                height: px(36),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Name::new("clear search"),
            ChildOf(field),
        ))
        .id();
    let cross = kit
        .icon(clear, "menu.window_close", 14.0, theme::INK_3)
        .unwrap_or_else(|| kit.typed(clear, "×", theme::BODY, theme::INK_3));
    kit.commands.entity(clear).insert(crate::main_menu::Wash {
        target: cross,
        rest: theme::INK_3,
        hover: theme::INK_1,
    });
}

/// A tab of the navigation: lit when open, dim when a search found nothing in it.
fn nav_tab(kit: &mut Kit, parent: Entity, tab: Tab, lit: bool, empty: bool) {
    let button = kit
        .commands
        .spawn((
            TabButton(tab),
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            Node {
                height: px(NAV_TAB_HEIGHT),
                padding: UiRect::left(px(20)),
                align_items: AlignItems::Center,
                border: UiRect::left(px(6)),
                ..default()
            },
            BorderColor::all(if lit { theme::SELECT } else { Color::NONE }),
            Name::new(tab.title().to_lowercase()),
            ChildOf(parent),
        ))
        .id();
    if lit {
        kit.commands
            .entity(button)
            .insert(theme::rightward(theme::SELECT_FILL));
    }
    let role = Type {
        face: if lit { Face::Bold } else { Face::Semibold },
        size: 17.0,
        tracking: 2.0,
        figures: false,
    };
    let ink = if lit {
        theme::INK_1
    } else if empty {
        theme::INK_OFF
    } else {
        theme::NAV_INK
    };
    kit.typed(button, tab.title(), role, ink);
}

/// A section of the tab in view (or a card of a search) under its tab; `lit` while it is the one
/// in view, or while a search lists it.
fn nav_link(kit: &mut Kit, list: Entity, (at, jump): (usize, Jump), label: &str, lit: bool) {
    let link = kit
        .commands
        .spawn((
            NavLink { card: at, jump },
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            Node {
                height: px(NAV_LINK_HEIGHT),
                padding: UiRect::left(px(14)),
                align_items: AlignItems::Center,
                border: UiRect::left(px(2)),
                ..default()
            },
            BorderColor::all(if lit { theme::SELECT } else { theme::NAV_RULE }),
            Name::new(format!("{} link", label.to_lowercase())),
            ChildOf(list),
        ))
        .id();
    let role = Type {
        face: Face::Regular,
        size: 13.0,
        tracking: 0.5,
        figures: false,
    };
    let ink = if lit { theme::INK_1 } else { theme::NAV_INK };
    let text = kit.typed(link, label, role, ink);
    kit.commands.entity(text).insert(NavText(at));
}

/// The words of a navigation link.
#[derive(Component)]
pub struct NavText(pub usize);

/// The open view's title, the help circle of the hotkeys, and the window's close cross.
fn header(kit: &mut Kit, content: Entity, screen: &Screen, title: &str) {
    let bar = kit
        .commands
        .spawn((
            Node {
                height: px(HEADER_HEIGHT),
                flex_shrink: 0.0,
                padding: UiRect::new(px(40), px(16), px(0), px(0)),
                column_gap: px(10),
                align_items: AlignItems::Center,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme::DIVIDER),
            HeaderRoot,
            ChildOf(content),
        ))
        .id();
    header_contents(kit, bar, screen, title);
}

/// What the header holds: the open tab's title, the hotkeys' help and reset, the close cross.
fn header_contents(kit: &mut Kit, bar: Entity, screen: &Screen, title: &str) {
    kit.typed(bar, title, theme::TITLE, theme::INK_1);
    if screen.tab == Tab::Hotkeys && !screen.is_searching() {
        help_circle(kit, bar);
    }
    kit.node(
        bar,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    if screen.tab == Tab::Hotkeys && !screen.is_searching() {
        reset_all(kit, bar);
    }
    let close = kit
        .commands
        .spawn((
            CloseButton,
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            Node {
                width: px(theme::HIT),
                height: px(theme::HIT),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Name::new("close"),
            ChildOf(bar),
        ))
        .id();
    let cross = kit
        .icon(close, "menu.window_close", CLOSE_ICON, theme::INK_3)
        .unwrap_or_else(|| kit.typed(close, "×", theme::TITLE, theme::INK_3));
    kit.commands.entity(close).insert(crate::main_menu::Wash {
        target: cross,
        rest: theme::INK_3,
        hover: theme::INK_1,
    });
}

/// The circled question mark by HOTKEYS: under the cursor it says how a key is changed.
fn help_circle(kit: &mut Kit, bar: Entity) {
    crate::ui::help::draw(
        &mut kit.visual(),
        bar,
        "hotkey help",
        "Click a key, then press the new one. Backspace unbinds, Escape keeps it.",
        crate::ui::help::Style {
            diameter: 22.0,
            glyph: Type {
                face: Face::Bold,
                size: 13.0,
                tracking: 0.0,
                figures: false,
            },
            side: crate::ui::help::Side::Below,
        },
    );
}

/// The scroll bar's rail and thumb, which `screen::scroll_bar` places.
fn scroll_bar(kit: &mut Kit, area: Entity) {
    let rail = kit
        .commands
        .spawn((
            ScrollRail,
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                right: px(4),
                top: px(8),
                bottom: px(8),
                width: px(6),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(theme::SCROLL_RAIL),
            Pickable::IGNORE,
            ChildOf(area),
        ))
        .id();
    kit.commands.spawn((
        ScrollThumb,
        Node {
            position_type: PositionType::Absolute,
            width: px(6),
            border_radius: BorderRadius::all(px(3)),
            ..default()
        },
        BackgroundColor(theme::SCROLL_THUMB),
        Pickable::IGNORE,
        ChildOf(rail),
    ));
}

/// A card: its title over its rows.
fn card_view(kit: &mut Kit, body: Entity, at: usize, card: &Card, ctx: &RowCtx) {
    // As the original game's page has it: the section's title over a panel of its rows.
    let section = kit
        .commands
        .spawn((
            CardAnchor(at),
            CardTab(card.tab),
            Node {
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                max_width: px(CARD_WIDTH),
                row_gap: px(10),
                ..default()
            },
            Pickable::IGNORE,
            Name::new(format!("{} card", card.title.to_lowercase())),
            ChildOf(body),
        ))
        .id();
    let heading = Type {
        size: 20.0,
        face: Face::Regular,
        tracking: 1.0,
        ..theme::HEADING
    };
    kit.typed(section, card.title.as_str(), heading, theme::INK_2);
    let frame = kit
        .commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(3),
                padding: UiRect::all(px(8)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::CARD),
            BorderColor::all(theme::CARD_EDGE),
            Pickable::IGNORE,
            ChildOf(section),
        ))
        .id();
    for row in &card.section.rows {
        row_view(kit, frame, *row, ctx);
    }
}

/// One row: its label, the arrow that puts it back when it changed, and its control.
fn row_view(kit: &mut Kit, card: Entity, row: Row, ctx: &RowCtx) {
    let root = row_frame(kit, card, row, ctx);
    if row == Row::Census {
        census_line(kit, root);
        return;
    }
    if row == Row::Mixer {
        mixer_view(kit, root, row);
        return;
    }
    let role = Type {
        size: if ctx.dense { 14.0 } else { 15.0 },
        ..theme::LABEL
    };
    let label = kit.label(root, row, &ctx.screen.query, role);
    kit.commands.entity(label).insert(Node {
        flex_grow: 1.0,
        min_width: px(0),
        ..default()
    });
    reset_arrow(kit, root, row, false);
    match row {
        Row::Text(setting) => name_field(kit, root, setting),
        Row::Toggle(toggle) => tick_box(kit, root, toggle),
        Row::Slider(slider) => slider_view(kit, root, slider),
        Row::Choice(choice) => choice_view(kit, root, choice, ctx),
        Row::Key(at) => key_field(kit, root, at),
        Row::Census | Row::Mixer => {}
    }
}

/// A row's box in its card with the bar that marks it changed; a toggle's is a button, a click
/// anywhere on it turns it.
fn row_frame(kit: &mut Kit, card: Entity, row: Row, ctx: &RowCtx) -> Entity {
    let height = if ctx.dense {
        KEY_ROW_HEIGHT
    } else {
        ROW_HEIGHT
    };
    let vertical = match row {
        Row::Census => 14,
        Row::Mixer => 18,
        _ => 2,
    };
    let node = Node {
        position_type: PositionType::Relative,
        min_height: px(height),
        flex_shrink: 0.0,
        column_gap: px(10),
        padding: UiRect::axes(px(14), px(vertical)),
        align_items: AlignItems::Center,
        ..default()
    };
    let name = Name::new(row.label().to_lowercase());
    let root = match row {
        Row::Toggle(toggle) => kit
            .commands
            .spawn((ToggleRow(toggle), Button, name, ChildOf(card)))
            .id(),
        _ => kit
            .commands
            .spawn((name, Pickable::IGNORE, ChildOf(card)))
            .id(),
    };
    kit.commands
        .entity(root)
        .insert((node, BackgroundColor(theme::FIELD_SUNKEN)));
    kit.commands.spawn((
        RowBar(row),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(8),
            bottom: px(8),
            width: px(3),
            ..default()
        },
        BackgroundColor(theme::FOCUS),
        Pickable::IGNORE,
        ChildOf(root),
    ));
    root
}

/// The line telling where the art and the sound come from.
fn census_line(kit: &mut Kit, root: Entity) {
    let role = Type {
        size: 14.0,
        ..theme::BODY
    };
    let text = kit.typed(root, kit.manifest.census(), role, theme::INK_4);
    kit.commands.entity(text).insert(Node {
        flex_grow: 1.0,
        flex_basis: px(0),
        ..default()
    });
}

/// The arrow that puts a changed row back to its default; in the row's top corner when the row
/// has no label line to stand beside.
fn reset_arrow(kit: &mut Kit, row_entity: Entity, row: Row, corner: bool) {
    let place = if corner {
        Node {
            position_type: PositionType::Absolute,
            right: px(8),
            top: px(6),
            ..default()
        }
    } else {
        Node::default()
    };
    let button = kit
        .commands
        .spawn((
            RowReset(row),
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            Node {
                display: Display::None,
                width: px(28),
                height: px(28),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..place
            },
            Name::new(format!("reset {}", row.label().to_lowercase())),
            ChildOf(row_entity),
        ))
        .id();
    if let Some(icon) = kit.icon(button, "menu.reset", 16.0, theme::INK_5) {
        kit.commands.entity(button).insert(crate::main_menu::Wash {
            target: icon,
            rest: theme::INK_5,
            hover: theme::INK_1,
        });
    }
}

/// A typed line (the profile name): a field a click types into, Enter or Escape done.
fn name_field(kit: &mut Kit, root: Entity, setting: TextSetting) {
    let field = kit
        .commands
        .spawn((
            NameField(setting),
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            Name::new(format!("{} field", setting.label().to_lowercase())),
            Node {
                width: px(FIELD_WIDTH),
                height: px(FIELD_HEIGHT),
                flex_shrink: 0.0,
                padding: UiRect::horizontal(px(10)),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::FIELD_TYPED),
            BorderColor::all(theme::FIELD_EDGE),
            ChildOf(root),
        ))
        .id();
    let text = kit.typed(field, "", theme::BODY, theme::INK_1);
    kit.commands
        .entity(text)
        .insert((NameText(setting), TextLayout::no_wrap()));
    kit.commands.spawn((
        NameCaret(setting),
        Node {
            display: Display::None,
            width: px(2),
            height: px(18),
            margin: UiRect::left(px(1)),
            ..default()
        },
        BackgroundColor(theme::SELECT),
        Pickable::IGNORE,
        ChildOf(field),
    ));
    kit.node(
        field,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    let count = Type {
        size: 12.0,
        figures: true,
        ..theme::BODY
    };
    let counter = kit.typed(field, "", count, theme::INK_6);
    kit.commands.entity(counter).insert(NameCount(setting));
}

/// A checkbox: a click anywhere on the row turns it.
fn tick_box(kit: &mut Kit, root: Entity, toggle: Toggle) {
    let tick = kit
        .commands
        .spawn((
            ToggleBox(toggle),
            // Named, as the keyboard knows the control it stands on by its name.
            Name::new(format!("{} box", toggle.label().to_lowercase())),
            Outline::new(px(2), px(3), Color::NONE),
            Node {
                width: px(24),
                height: px(24),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                ..default()
            },
            BorderColor::all(theme::TICK_EDGE_OFF),
            BackgroundColor(Color::NONE),
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    if let Some(mark) = kit.icon(tick, "menu.check", 16.0, theme::INK_1) {
        kit.commands.entity(mark).insert(ToggleMark(toggle));
    }
}

/// A slider: its rail, filled to the value, the thumb on it, the value beside it.
fn slider_view(kit: &mut Kit, root: Entity, slider: Slider) {
    let track = kit
        .commands
        .spawn((
            SliderTrack(slider),
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            RelativeCursorPosition::default(),
            Node {
                width: px(SLIDER_WIDTH),
                height: px(24),
                flex_shrink: 0.0,
                ..default()
            },
            Name::new(format!("{} slider", slider.label().to_lowercase())),
            ChildOf(root),
        ))
        .id();
    slider_rail(kit, track, slider);
    kit.commands.spawn((
        SliderThumb(slider),
        Node {
            position_type: PositionType::Absolute,
            top: px(4),
            left: percent(0),
            margin: UiRect::left(px(-THUMB / 2.0)),
            width: px(THUMB),
            height: px(THUMB),
            border_radius: BorderRadius::MAX,
            border: UiRect::all(px(0)),
            ..default()
        },
        BackgroundColor(theme::INK_1),
        BorderColor::all(theme::FOCUS),
        Pickable::IGNORE,
        ChildOf(track),
    ));
    let value = Type {
        figures: true,
        ..theme::BODY
    };
    let shown = kit.typed(root, "", value, theme::INK_1);
    kit.commands.entity(shown).insert((
        SliderValue(slider),
        Node {
            width: px(46),
            justify_content: JustifyContent::FlexEnd,
            ..default()
        },
        TextLayout::justify(Justify::Right),
    ));
}

/// A slider's rail and the part of it the value fills.
fn slider_rail(kit: &mut Kit, track: Entity, slider: Slider) {
    let rail = |width: Val| Node {
        position_type: PositionType::Absolute,
        left: px(0),
        top: px(10),
        height: px(4),
        width,
        border_radius: BorderRadius::all(px(2)),
        ..default()
    };
    kit.commands.spawn((
        rail(percent(100)),
        BackgroundColor(theme::RAIL),
        Pickable::IGNORE,
        ChildOf(track),
    ));
    kit.commands.spawn((
        SliderFill(slider),
        rail(percent(0)),
        BackgroundColor(theme::FOCUS),
        Pickable::IGNORE,
        ChildOf(track),
    ));
}

/// The mixer: a strip for each channel of sound and, at the right, the master.
fn mixer_view(kit: &mut Kit, root: Entity, row: Row) {
    let strips = kit.node(
        root,
        Node {
            flex_grow: 1.0,
            justify_content: JustifyContent::SpaceAround,
            align_items: AlignItems::FlexStart,
            ..default()
        },
    );
    for (slider, mute) in model::strips() {
        if slider == Slider::Volume {
            let rule = kit.node(
                strips,
                Node {
                    width: px(1),
                    align_self: AlignSelf::Stretch,
                    ..default()
                },
            );
            kit.commands
                .entity(rule)
                .insert(BackgroundColor(theme::CARD_RULE));
        }
        strip(kit, strips, slider, mute);
    }
    reset_arrow(kit, root, row, true);
}

/// A strip of the mixer from top to foot: the level, the fader, the mute and the name.
fn strip(kit: &mut Kit, parent: Entity, slider: Slider, mute: Toggle) {
    let column = kit.node(
        parent,
        Node {
            width: px(STRIP_WIDTH),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(10),
            ..default()
        },
    );
    let figures = Type {
        figures: true,
        ..theme::BODY
    };
    let level = kit.typed(column, "", figures, theme::INK_1);
    kit.commands.entity(level).insert(SliderValue(slider));
    fader(kit, column, slider);
    mute_button(kit, column, mute);
    let name = match slider {
        Slider::Volume => "MASTER".to_owned(),
        other => other.label().to_uppercase(),
    };
    let caption = Type {
        size: 11.0,
        tracking: 1.5,
        ..theme::CAPTION
    };
    kit.typed(column, name, caption, theme::INK_3);
}

/// An upright slider: the rail with its scale, filled to the level, and the cap on it.
fn fader(kit: &mut Kit, parent: Entity, slider: Slider) {
    let track = kit
        .commands
        .spawn((
            SliderTrack(slider),
            Vertical,
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            RelativeCursorPosition::default(),
            Node {
                width: px(FADER_WIDTH),
                height: px(FADER_HEIGHT),
                flex_shrink: 0.0,
                ..default()
            },
            Name::new(format!("{} slider", slider.label().to_lowercase())),
            ChildOf(parent),
        ))
        .id();
    let rail = |height: Val| Node {
        position_type: PositionType::Absolute,
        left: px(FADER_WIDTH / 2.0 - 2.0),
        bottom: px(0),
        width: px(4),
        height,
        border_radius: BorderRadius::all(px(2)),
        ..default()
    };
    kit.commands.spawn((
        rail(percent(100)),
        BackgroundColor(theme::RAIL),
        Pickable::IGNORE,
        ChildOf(track),
    ));
    kit.commands.spawn((
        SliderFill(slider),
        Vertical,
        rail(percent(0)),
        BackgroundColor(theme::FOCUS),
        Pickable::IGNORE,
        ChildOf(track),
    ));
    for step in 0..=4 {
        kit.commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(FADER_WIDTH / 2.0 + 7.0),
                bottom: percent(step as f32 * 25.0),
                width: px(6),
                height: px(1),
                ..default()
            },
            BackgroundColor(theme::INK_6),
            Pickable::IGNORE,
            ChildOf(track),
        ));
    }
    kit.commands.spawn((
        SliderThumb(slider),
        Vertical,
        Node {
            position_type: PositionType::Absolute,
            left: px((FADER_WIDTH - FADER_CAP.0) / 2.0),
            bottom: percent(0),
            margin: UiRect::bottom(px(-FADER_CAP.1 / 2.0)),
            width: px(FADER_CAP.0),
            height: px(FADER_CAP.1),
            border_radius: BorderRadius::all(px(3)),
            border: UiRect::all(px(0)),
            ..default()
        },
        BackgroundColor(theme::INK_1),
        BorderColor::all(theme::FOCUS),
        Pickable::IGNORE,
        ChildOf(track),
    ));
}

/// A strip's mute: a button that lights red while its channel is silenced.
fn mute_button(kit: &mut Kit, parent: Entity, mute: Toggle) {
    let button = kit
        .commands
        .spawn((
            MuteBox(mute),
            ToggleRow(mute),
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            Node {
                width: px(MUTE_SIZE.0),
                height: px(MUTE_SIZE.1),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::FIELD),
            BorderColor::all(theme::FIELD_EDGE),
            Name::new(mute.label().to_lowercase()),
            ChildOf(parent),
        ))
        .id();
    let caption = Type {
        face: Face::Bold,
        size: 11.0,
        tracking: 1.5,
        figures: false,
    };
    kit.typed(button, "MUTE", caption, theme::INK_1);
}

/// A choice: its value on a button that opens the list under it.
fn choice_view(kit: &mut Kit, root: Entity, choice: Choice, ctx: &RowCtx) {
    let anchor = kit.node(
        root,
        Node {
            position_type: PositionType::Relative,
            flex_shrink: 0.0,
            ..default()
        },
    );
    let button = kit
        .commands
        .spawn((
            ChoiceButton(choice),
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            Node {
                width: px(FIELD_WIDTH),
                height: px(FIELD_HEIGHT),
                padding: UiRect::horizontal(px(12)),
                column_gap: px(8),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::FIELD),
            BorderColor::all(theme::FIELD_EDGE),
            Name::new(format!("{} choice", choice.label().to_lowercase())),
            ChildOf(anchor),
        ))
        .id();
    let value = kit.typed(button, "", theme::BODY, theme::INK_1);
    kit.commands.entity(value).insert(ChoiceValue(choice));
    kit.icon(button, "menu.dropdown", 12.0, theme::INK_3);
    if ctx.screen.dropdown == Some(choice) {
        options_list(kit, anchor, choice, ctx);
    }
}

/// The open list of a choice, over the rows below it.
fn options_list(kit: &mut Kit, anchor: Entity, choice: Choice, ctx: &RowCtx) {
    let list = kit
        .commands
        .spawn((
            OptionsList,
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                top: px(42),
                width: px(230),
                padding: UiRect::vertical(px(4)),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::LIST),
            BorderColor::all(theme::TOOLTIP_EDGE),
            BoxShadow::new(Color::BLACK, px(0), px(10), px(0), px(30)),
            GlobalZIndex(66),
            ChildOf(anchor),
        ))
        .id();
    let picked = choice.current(ctx.settings, ctx.displays);
    let options = choice.options(ctx.settings, ctx.displays);
    for (at, option) in options.into_iter().enumerate() {
        let enabled = choice.option_enabled(ctx.settings, ctx.displays, at);
        option_item(
            kit,
            list,
            ChoiceOption(choice, at),
            option,
            enabled,
            picked == Some(at),
        );
    }
}

/// An option of an open list: greyed when it cannot be picked, marked when it is the one picked.
fn option_item(
    kit: &mut Kit,
    list: Entity,
    which: ChoiceOption,
    words: String,
    enabled: bool,
    picked: bool,
) {
    let item = kit
        .commands
        .spawn((
            which,
            Button,
            Node {
                height: px(34),
                padding: UiRect::horizontal(px(12)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
            Name::new(words.to_lowercase()),
            ChildOf(list),
        ))
        .id();
    let ink = if enabled {
        theme::INK_1
    } else {
        theme::INK_OFF
    };
    kit.typed(item, words, theme::BODY, ink);
    if !picked {
        return;
    }
    kit.commands
        .entity(item)
        .insert(BackgroundColor(theme::LIST_PICKED));
    kit.commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            bottom: px(0),
            width: px(3),
            ..default()
        },
        BackgroundColor(theme::SELECT),
        Pickable::IGNORE,
        ChildOf(item),
    ));
    kit.icon(item, "menu.check", 14.0, theme::SELECT);
}

/// A binding's key field.
fn key_field(kit: &mut Kit, root: Entity, at: usize) {
    let field = kit
        .commands
        .spawn((
            KeyField(at),
            RowKey,
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            Node {
                width: px(130),
                height: px(32),
                flex_shrink: 0.0,
                padding: UiRect::horizontal(px(8)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::FIELD),
            BorderColor::all(theme::FIELD_EDGE),
            Name::new(format!("key {}", keys::BINDINGS[at].label.to_lowercase())),
            ChildOf(root),
        ))
        .id();
    let role = Type {
        face: Face::Bold,
        size: 13.0,
        tracking: 0.0,
        figures: false,
    };
    let text = kit.typed(field, "", role, theme::INK_1);
    kit.commands.entity(text).insert(KeyText(at));
}

/// A column of the slots' panel: a caption over what it holds.
fn slot_column(kit: &mut Kit, panel: Entity, caption: &str) -> Entity {
    let column = kit.node(
        panel,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            ..default()
        },
    );
    let role = Type {
        size: 12.0,
        tracking: 2.0,
        ..theme::CAPTION
    };
    kit.typed(column, caption, role, theme::INK_4);
    column
}

/// The HUD's slots at the top of the hotkeys: the ability bar, the six item slots, the town
/// portal and the neutral item's slot, each with its key in its corner; a click on one captures
/// its key as the row's field does.
fn hud_panel(kit: &mut Kit, body: Entity, icons: &[(keys::Action, String)]) {
    let panel = kit
        .commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                max_width: px(CARD_WIDTH),
                padding: UiRect::new(px(18), px(18), px(16), px(14)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::FlexEnd,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::CARD),
            BorderColor::all(theme::CARD_EDGE),
            Pickable::IGNORE,
            Name::new("slots"),
            ChildOf(body),
        ))
        .id();
    let art_of = |action: keys::Action| {
        icons
            .iter()
            .find(|(of, _)| *of == action)
            .map(|(_, path)| path.as_str())
    };
    ability_bar(kit, panel, &art_of);
    item_slots(kit, panel, &art_of);
}

/// What a slot is: the binding its key is (none for a slot no key reaches), the art in it, its
/// size, whether it is round, and the word under its badge.
struct Slot<'a> {
    at: Option<usize>,
    art: Option<&'a str>,
    size: (f32, f32),
    round: bool,
    tag: &'a str,
}

impl<'a> Slot<'a> {
    /// The slot of `action`, a square of `size`.
    fn of(action: keys::Action, art: Option<&'a str>, size: (f32, f32)) -> Slot<'a> {
        Slot {
            at: Some(keys::index_of(action).expect("a bound slot")),
            art,
            size,
            round: false,
            tag: "",
        }
    }
}

/// The ability bar: Q W E D F R.
fn ability_bar<'a>(
    kit: &mut Kit,
    panel: Entity,
    art_of: &impl Fn(keys::Action) -> Option<&'a str>,
) {
    let column = slot_column(kit, panel, "ABILITIES");
    let bar = kit.node(
        column,
        Node {
            column_gap: px(6),
            ..default()
        },
    );
    for action in [
        keys::Action::Ability(0),
        keys::Action::Ability(1),
        keys::Action::Ability(2),
        keys::Action::Ability(3),
        keys::Action::Ability(4),
        keys::Action::Ultimate,
    ] {
        slot(kit, bar, Slot::of(action, art_of(action), ABILITY_SLOT));
    }
}

/// The inventory in three by two, the town portal and the neutral item's round slot.
fn item_slots<'a>(kit: &mut Kit, panel: Entity, art_of: &impl Fn(keys::Action) -> Option<&'a str>) {
    let column = slot_column(kit, panel, "ITEMS");
    let line = kit.node(
        column,
        Node {
            column_gap: px(10),
            align_items: AlignItems::FlexEnd,
            ..default()
        },
    );
    let grid = kit.node(
        line,
        Node {
            display: Display::Grid,
            grid_template_columns: RepeatedGridTrack::px(3, ITEM_SLOT.0),
            column_gap: px(5),
            row_gap: px(5),
            ..default()
        },
    );
    for number in 0..6 {
        let action = keys::Action::Item(number);
        slot(kit, grid, Slot::of(action, art_of(action), ITEM_SLOT));
    }
    let side = kit.node(
        line,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6),
            ..default()
        },
    );
    let portal = keys::Action::TownPortal;
    let town_portal = Slot {
        tag: "TP",
        ..Slot::of(portal, art_of(portal), PORTAL_SLOT)
    };
    slot(kit, side, town_portal);
    // Neutral items have no binding yet: their slot shows unbound.
    let neutral = Slot {
        at: None,
        art: None,
        size: (NEUTRAL_SLOT, NEUTRAL_SLOT),
        round: true,
        tag: "",
    };
    slot(kit, side, neutral);
}

/// One slot: its square with the art in it, its key badge and its word.
fn slot(kit: &mut Kit, parent: Entity, slot: Slot) {
    let (width, height) = slot.size;
    let mut square = kit.commands.spawn((
        Node {
            width: px(width),
            height: px(height),
            flex_shrink: 0.0,
            border: UiRect::all(px(1)),
            border_radius: if slot.round {
                BorderRadius::MAX
            } else {
                BorderRadius::all(px(2))
            },
            ..default()
        },
        BackgroundColor(theme::FIELD_TYPED),
        BorderColor::all(theme::TOOLTIP_EDGE),
        ChildOf(parent),
    ));
    let label = slot
        .at
        .map_or("neutral item", |at| keys::BINDINGS[at].label);
    square.insert(Name::new(format!("slot {}", label.to_lowercase())));
    if let Some(at) = slot.at {
        square.insert((
            KeyField(at),
            Button,
            Outline::new(px(2), px(2), Color::NONE),
        ));
    } else {
        square.insert(Pickable::IGNORE);
    }
    let square = square.id();
    if let Some(path) = slot.art {
        kit.commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                ..default()
            },
            ImageNode::new(kit.server.load(path.to_owned())),
            Pickable::IGNORE,
            ChildOf(square),
        ));
    }
    key_badge(kit, square, slot.at, slot.round);
    if !slot.tag.is_empty() {
        let caption = Type {
            face: Face::Bold,
            size: 9.0,
            tracking: 1.0,
            figures: false,
        };
        let words = kit.typed(square, slot.tag, caption, theme::INK_5);
        kit.commands.entity(words).insert(Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            bottom: px(2),
            justify_content: JustifyContent::Center,
            ..default()
        });
    }
}

/// The key in a slot's corner (a round slot's, in its middle).
fn key_badge(kit: &mut Kit, square: Entity, at: Option<usize>, round: bool) {
    let place = if round {
        Node {
            left: percent(50),
            top: percent(50),
            margin: UiRect::new(px(-9), px(0), px(-14), px(0)),
            ..default()
        }
    } else {
        Node {
            left: px(1),
            top: px(1),
            ..default()
        }
    };
    let badge = kit
        .commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                min_width: px(14),
                height: px(16),
                padding: UiRect::horizontal(px(4)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..place
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            Pickable::IGNORE,
            ChildOf(square),
        ))
        .id();
    let role = Type {
        face: Face::Bold,
        size: 11.0,
        tracking: 0.0,
        figures: false,
    };
    let key = kit.typed(
        badge,
        if at.is_some() { "" } else { "—" },
        role,
        theme::INK_6,
    );
    if let Some(at) = at {
        kit.commands.entity(badge).insert(SlotBadge(at));
        kit.commands.entity(key).insert(SlotKey(at));
    }
}

/// RESET TO DEFAULTS at the right of the header while the hotkeys are in view.
fn reset_all(kit: &mut Kit, bar: Entity) {
    let button = kit
        .commands
        .spawn((
            ResetButton,
            Button,
            Outline::new(px(2), px(2), Color::NONE),
            Node {
                height: px(36),
                padding: UiRect::horizontal(px(20)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::FIELD),
            BorderColor::all(theme::FIELD_EDGE),
            Name::new("reset to defaults"),
            ChildOf(bar),
        ))
        .id();
    kit.typed(
        button,
        "RESET TO DEFAULTS",
        theme::button(36.0),
        theme::INK_2,
    );
}

/// The bar under the hotkeys that says which bindings share a key; it shows only while some do.
fn clash_bar(kit: &mut Kit, content: Entity) {
    let bar = kit
        .commands
        .spawn((
            ClashBar,
            Node {
                display: Display::None,
                height: px(CLASH_BAR_HEIGHT),
                flex_shrink: 0.0,
                padding: UiRect::horizontal(px(40)),
                column_gap: px(10),
                align_items: AlignItems::Center,
                border: UiRect::top(px(1)),
                ..default()
            },
            BackgroundColor(theme::KEY_CLASH.0),
            BorderColor::all(theme::KEY_CLASH.1),
            Pickable::IGNORE,
            ChildOf(content),
        ))
        .id();
    let sign = kit.node(
        bar,
        Node {
            width: px(16),
            height: px(16),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(1.5)),
            ..default()
        },
    );
    kit.commands
        .entity(sign)
        .insert(BorderColor::all(theme::ERROR_TEXT));
    let mark = Type {
        face: Face::Bold,
        size: 11.0,
        tracking: 0.0,
        figures: false,
    };
    kit.typed(sign, "!", mark, theme::ERROR_TEXT);
    let role = Type {
        size: 14.0,
        ..theme::BODY
    };
    let words = kit.typed(bar, "", role, theme::ERROR_TEXT);
    kit.commands.entity(words).insert(ClashText);
}
