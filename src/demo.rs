//! The in-match cheat panel. Hero actions address the selected hero; world actions are
//! sent from the player's seat. Placement waits for a world click.

use bevy::input::keyboard::Key;
use bevy::picking::{events::PointerPress, hover::Hovered, pointer::PointerButton};
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use bota_client_core::{App as Core, Source, world_vec};
use bota_proto::{Cheat, EntityId, HeroId, ItemId, Order, Vec2};

use crate::dota_text::Texts;
use crate::hud::Blocker;
use crate::input::Typing;
use crate::main_menu::{Act, MenuCore};
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts, dota};

const WIDTH: f32 = 280.0;
const THUMB: f32 = 24.0;
const TOP: f32 = 129.5;
const ROW_HEIGHT: f32 = 26.0;
const GAP: f32 = 4.0;
const FOUND_ITEMS: usize = 6;
const SEARCH_BYTES: usize = 32;

const BODY: Color = Color::srgb_u8(32, 37, 40);
const BUTTON: Color = Color::srgb_u8(61, 66, 70);
const EDGE: Color = Color::srgb_u8(80, 86, 90);
const ACCENT: Color = Color::srgb_u8(65, 86, 109);
const ALLY: Color = Color::srgb_u8(51, 86, 64);
const ENEMY: Color = Color::srgb_u8(108, 53, 51);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    Hero { hero: HeroId, enemy: bool },
    Creeps,
    Teleport(EntityId),
}

impl Placement {
    fn order(self, at: Vec2) -> (Option<EntityId>, Cheat) {
        match self {
            Self::Hero { hero, enemy } => (None, Cheat::CreateHero { hero, enemy, at }),
            Self::Creeps => (None, Cheat::CreateCreeps { enemy: true, at }),
            Self::Teleport(hero) => (Some(hero), Cheat::Teleport { at }),
        }
    }
}

#[derive(Resource, Debug, Default)]
pub struct DemoPanel {
    open: Option<bool>,
    spawn_hero: Option<HeroId>,
    placing: Option<Placement>,
    pub search: String,
    typing: bool,
    scroll: f32,
}

impl DemoPanel {
    /// Called by world input before it interprets a click as a selection or order.
    pub fn world_click(
        &mut self,
        core: &mut Core,
        buttons: &ButtonInput<MouseButton>,
        ground: (f32, f32),
    ) -> bool {
        let Some(placement) = self.placing else {
            return false;
        };
        if buttons.just_pressed(MouseButton::Right) {
            self.placing = None;
            return true;
        }
        if buttons.just_pressed(MouseButton::Left) {
            self.placing = None;
            let (hero, cheat) = placement.order(world_vec(ground));
            send(core, hero, cheat);
            return true;
        }
        false
    }
}

#[derive(Component, Clone, Copy)]
enum Press {
    Fold,
    SpawnKind(HeroId),
    Spawn(bool),
    CreepsHere,
    Move,
    World(Cheat),
    Hero(Cheat),
    Search,
    Grant(ItemId),
    Quit,
}

#[derive(Component)]
struct PanelRoot;

#[derive(Component)]
struct PanelBody;

pub fn plugin(app: &mut App) {
    app.init_resource::<DemoPanel>()
        .add_systems(Startup, spawn_root)
        .add_systems(
            First,
            crate::loading::reset::<DemoPanel>.in_set(crate::loading::Begin),
        )
        .add_observer(presses)
        .add_systems(Update, draw.in_set(ui::WidgetBuild))
        .add_systems(Update, (paint, scroll).in_set(ui::WidgetPaint));
}

fn spawn_root(mut commands: Commands) {
    commands.spawn((
        PanelRoot,
        ui::HudPanel,
        Name::new("demo controls"),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(TOP),
            bottom: px(crate::minimap::SIDE + ui::BAR_MARGIN),
            width: dota(WIDTH + THUMB),
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
        GlobalZIndex(40),
    ));
}

fn allowed(core: &Core) -> bool {
    matches!(core.source, Source::Live(_))
        && core.my_slot.is_some()
        && core.view().is_some()
        && core.over.is_none()
        && !core.lost
        && core
            .statics()
            .is_some_and(|statics| statics.metadata().cheats)
}

fn is_demo(core: &Core, menu: Option<&MenuCore>) -> bool {
    core.statics()
        .is_some_and(|statics| statics.metadata().map == bota_core::HERO_DEMO_ID)
        || menu.is_some_and(|menu| menu.demo_hero().is_some())
}

fn spawn_kind(panel: &DemoPanel, core: &Core) -> HeroId {
    panel
        .spawn_hero
        .or_else(|| selected_hero(core).map(|(_, hero, _)| hero))
        .unwrap_or(HeroId::PUDGE)
}

fn selected_hero(core: &Core) -> Option<(EntityId, HeroId, bool)> {
    let selected = core.commanded()?;
    let seat = core
        .view()?
        .players
        .iter()
        .find(|seat| seat.unit == Some(selected))?;
    let created = core.created_seat(seat.slot);
    (Some(seat.slot) == core.my_slot || created).then_some((selected, seat.hero, created))
}

fn send(core: &mut Core, hero: Option<EntityId>, cheat: Cheat) {
    if allowed(core) {
        core.send_order_to(hero, Order::Cheat { cheat });
    }
}

fn presses(
    event: On<PointerPress>,
    mut session: NonSendMut<Session>,
    mut menu: Option<NonSendMut<MenuCore>>,
    mut panel: ResMut<DemoPanel>,
    buttons: Query<&Press>,
    mut typing: ResMut<Typing>,
    shown: Res<ui::HudShown>,
) {
    let core = &mut session.0;
    if event.button != PointerButton::Primary || !shown.0 || !allowed(core) {
        return;
    }
    let Ok(&press) = buttons.get(event.entity) else {
        return;
    };
    panel.typing = matches!(press, Press::Search);
    if panel.typing {
        *typing = Typing::DemoSearch;
    } else if *typing == Typing::DemoSearch {
        *typing = Typing::No;
    }
    match press {
        Press::Fold => {
            panel.open = Some(!panel.open.unwrap_or(is_demo(core, menu.as_deref())));
            panel.placing = None;
        }
        Press::SpawnKind(hero) => panel.spawn_hero = Some(hero),
        Press::Spawn(enemy) => {
            panel.placing = Some(Placement::Hero {
                hero: spawn_kind(&panel, core),
                enemy,
            })
        }
        Press::CreepsHere => panel.placing = Some(Placement::Creeps),
        Press::Move => {
            panel.placing = selected_hero(core).map(|(id, _, _)| Placement::Teleport(id));
        }
        Press::World(order) => send(core, None, order),
        Press::Hero(order) => send_to_selected(core, order),
        Press::Grant(item) => send_to_selected(core, Cheat::Item { item }),
        Press::Search => {}
        Press::Quit => {
            if let Some(menu) = &mut menu {
                menu.act(Act::Disconnect);
            }
        }
    }
}

fn send_to_selected(core: &mut Core, cheat: Cheat) {
    if let Some((id, _, created)) = selected_hero(core)
        && (cheat != Cheat::RemoveHero || created)
    {
        send(core, Some(id), cheat);
    }
}

pub fn item_results(
    catalog: &crate::shop::Catalog,
    look: &crate::tip_content::Lookup,
    search: &str,
) -> impl Iterator<Item = ItemId> + use<> {
    catalog
        .search(look, search)
        .into_iter()
        .map(ItemId)
        .filter(|&id| bota_core::item_def(id).is_some())
        .take(FOUND_ITEMS)
}

pub fn search_key(
    core: &mut Core,
    panel: &mut DemoPanel,
    key: &Key,
    first: Option<ItemId>,
) -> bool {
    match key {
        Key::Enter | Key::Escape => {
            if *key == Key::Enter
                && let Some(item) = first
            {
                send_to_selected(core, Cheat::Item { item });
            }
            panel.typing = false;
            true
        }
        Key::Backspace => {
            panel.search.pop();
            false
        }
        Key::Character(typed) if panel.search.len() + typed.len() <= SEARCH_BYTES => {
            panel.search.push_str(typed);
            false
        }
        Key::Space if panel.search.len() < SEARCH_BYTES => {
            panel.search.push(' ');
            false
        }
        _ => false,
    }
}

#[derive(PartialEq, Eq)]
struct Shown {
    open: bool,
    quit: bool,
    spawn: HeroId,
    placing: Option<Placement>,
    selected: Option<(EntityId, HeroId, bool)>,
    search: String,
    typing: bool,
}

#[derive(bevy::ecs::system::SystemParam)]
struct Art<'w> {
    texts: Res<'w, Texts>,
    manifest: Res<'w, Manifest>,
    catalog: Res<'w, crate::shop::Catalog>,
    fonts: Res<'w, Fonts>,
    server: Res<'w, AssetServer>,
}

fn draw(
    mut commands: Commands,
    session: NonSend<Session>,
    menu: Option<NonSend<MenuCore>>,
    panel: Res<DemoPanel>,
    hud: Res<ui::HudShown>,
    art: Art,
    root: Single<Entity, With<PanelRoot>>,
    mut drawn: Local<Option<Shown>>,
) {
    let core = &session.0;
    if !hud.0 || !allowed(core) {
        if drawn.take().is_some() {
            commands.entity(*root).despawn_children();
        }
        return;
    }
    let shown = Shown {
        open: panel.open.unwrap_or(is_demo(core, menu.as_deref())),
        quit: menu.is_some_and(|menu| menu.demo_hero().is_some()),
        spawn: spawn_kind(&panel, core),
        placing: panel.placing,
        selected: selected_hero(core),
        search: panel.search.clone(),
        typing: panel.typing,
    };
    if drawn.as_ref() != Some(&shown) {
        commands.entity(*root).despawn_children();
        let mut kit = Kit {
            commands: &mut commands,
            art: &art,
        };
        kit.build(*root, &shown, panel.scroll);
        *drawn = Some(shown);
    }
}

#[derive(Component)]
struct Fill(Color);

fn paint(mut buttons: Query<(&Hovered, &Fill, &mut BackgroundColor), Changed<Hovered>>) {
    for (hovered, fill, mut background) in &mut buttons {
        background.0 = if hovered.get() {
            fill.0.lighter(0.07)
        } else {
            fill.0
        };
    }
}

fn scroll(
    wheel: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    mut bodies: Query<(&Hovered, &mut ScrollPosition), With<PanelBody>>,
    mut panel: ResMut<DemoPanel>,
) {
    let Ok((hovered, mut position)) = bodies.single_mut() else {
        return;
    };
    if !hovered.get() || wheel.delta.y == 0.0 {
        return;
    }
    let step = match wheel.unit {
        bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y * ROW_HEIGHT,
        bevy::input::mouse::MouseScrollUnit::Pixel => wheel.delta.y,
    };
    panel.scroll = (position.y - step).max(0.0);
    position.y = panel.scroll;
}

struct Kit<'a, 'w, 's, 'r> {
    commands: &'a mut Commands<'w, 's>,
    art: &'a Art<'r>,
}

#[derive(Clone, Copy)]
struct Cell<'a> {
    name: &'a str,
    label: &'a str,
    press: Press,
    fill: Color,
    icon: Option<&'a str>,
}

impl<'a> Cell<'a> {
    const fn new(name: &'a str, label: &'a str, press: Press) -> Self {
        Self {
            name,
            label,
            press,
            fill: BUTTON,
            icon: None,
        }
    }

    fn icon(mut self, path: Option<&'a str>) -> Self {
        self.icon = path;
        self
    }

    const fn fill(mut self, fill: Color) -> Self {
        self.fill = fill;
        self
    }
}

impl Kit<'_, '_, '_, '_> {
    fn text(&mut self, parent: Entity, label: &str, size: f32, ink: Color) {
        self.commands.spawn((
            ui::text(label, &self.art.fonts.bold, size * ui::DOTA, ink),
            TextLayout::no_wrap(),
            Pickable::IGNORE,
            ChildOf(parent),
        ));
    }

    fn button(&mut self, parent: Entity, cell: Cell, node: Node) -> Entity {
        let button = self
            .commands
            .spawn((
                Name::new(format!("demo {}", cell.name.to_lowercase())),
                cell.press,
                Button,
                Blocker,
                Hovered::default(),
                Fill(cell.fill),
                BackgroundColor(cell.fill),
                BorderColor::all(EDGE),
                node,
                ChildOf(parent),
            ))
            .id();
        if let Some(icon) = cell.icon {
            self.commands.spawn((
                Node {
                    width: dota(25.0),
                    height: dota(16.0),
                    margin: UiRect::right(dota(GAP)),
                    flex_shrink: 0.0,
                    ..default()
                },
                ImageNode::new(self.art.server.load(icon.to_owned())),
                Pickable::IGNORE,
                ChildOf(button),
            ));
        }
        self.text(button, cell.label, 12.0, ui::INK);
        button
    }

    fn row(&mut self, parent: Entity, cells: &[Cell]) {
        let row = self
            .commands
            .spawn((
                Node {
                    display: Display::Grid,
                    grid_template_columns: RepeatedGridTrack::flex(cells.len() as u16, 1.0),
                    grid_template_rows: vec![GridTrack::px(ROW_HEIGHT * ui::DOTA)],
                    column_gap: dota(GAP),
                    flex_shrink: 0.0,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id();
        for &cell in cells {
            self.button(
                row,
                cell,
                Node {
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1)),
                    overflow: Overflow::clip(),
                    ..default()
                },
            );
        }
    }

    fn label(&mut self, body: Entity, text: &str, ink: Color) {
        let row = self
            .commands
            .spawn((
                Node {
                    height: dota(20.0),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    overflow: Overflow::clip(),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(body),
            ))
            .id();
        self.text(row, text, 13.0, ink);
    }

    fn build(&mut self, root: Entity, shown: &Shown, scroll: f32) {
        if shown.open {
            let body = self
                .commands
                .spawn((
                    Name::new("hud demo panel"),
                    PanelBody,
                    Blocker,
                    Hovered::default(),
                    Node {
                        align_self: AlignSelf::FlexStart,
                        width: dota(WIDTH),
                        max_height: percent(100),
                        flex_direction: FlexDirection::Column,
                        flex_shrink: 0.0,
                        padding: UiRect::all(dota(9.0)),
                        row_gap: dota(GAP),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition(bevy::math::Vec2::new(0.0, scroll)),
                    BackgroundGradient::from(LinearGradient::to_right(vec![
                        BODY.into(),
                        Color::srgb_u8(27, 30, 32).into(),
                    ])),
                    ChildOf(root),
                ))
                .id();
            self.spawn_controls(body, shown);
            self.hero_controls(body, shown);
            self.world_controls(body, shown);
        }
        self.button(
            root,
            Cell::new("fold", if shown.open { "‹" } else { "›" }, Press::Fold),
            Node {
                width: dota(THUMB),
                height: dota(88.0),
                max_height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::right(dota(6.0)),
                ..default()
            },
        );
    }

    fn spawn_controls(&mut self, body: Entity, shown: &Shown) {
        self.label(body, "SPAWN", ui::INK);
        let heroes: Vec<_> = bota_core::HEROES
            .iter()
            .enumerate()
            .map(|(index, hero)| {
                let id = HeroId(index as u16);
                (
                    id,
                    self.art.texts.name(hero.key).to_uppercase(),
                    self.art.manifest.portrait(id.0),
                )
            })
            .collect();
        let cells: Vec<_> = heroes
            .iter()
            .map(|(id, name, icon)| {
                Cell::new(name, name, Press::SpawnKind(*id))
                    .icon(icon.as_deref())
                    .fill(if *id == shown.spawn { ACCENT } else { BUTTON })
            })
            .collect();
        self.row(body, &cells);
        self.row(
            body,
            &[
                Cell::new("spawnally", "ALLY", Press::Spawn(false)).fill(ALLY),
                Cell::new("spawnenemy", "ENEMY", Press::Spawn(true)).fill(ENEMY),
            ],
        );
        self.row(
            body,
            &[
                Cell::new("creepwave", "CREEP WAVE", Press::World(Cheat::SpawnCreeps)),
                Cell::new("creepsat", "CREEPS HERE", Press::CreepsHere),
                Cell::new("neutrals", "NEUTRALS", Press::World(Cheat::SpawnNeutrals)),
            ],
        );
        let instruction = if shown.placing.is_some() {
            "CLICK GROUND · RIGHT CLICK TO CANCEL"
        } else {
            "UNITS"
        };
        self.label(body, instruction, ui::INK);
    }

    fn hero_controls(&mut self, body: Entity, shown: &Shown) {
        use Press::Hero;
        let selected = shown
            .selected
            .and_then(|(_, id, _)| bota_core::hero_def(id));
        let name = selected.map_or_else(
            || "Select a hero".to_owned(),
            |hero| self.art.texts.name(hero.key).to_owned(),
        );
        self.label(body, &name, ui::INK_DIM);
        self.row(
            body,
            &[
                Cell::new("levelup", "LEVEL UP", Hero(Cheat::Levels { count: 1 })),
                Cell::new(
                    "levelmax",
                    "LEVEL MAX",
                    Hero(Cheat::Levels {
                        count: bota_core::rules::HERO_MAX_LEVEL,
                    }),
                ),
                Cell::new("reset", "RESET HERO", Hero(Cheat::ResetHero)),
            ],
        );
        self.row(
            body,
            &[
                Cell::new("learn max", "LEARN MAX", Hero(Cheat::MaxAbilities)),
                Cell::new("unlearn", "UNLEARN", Hero(Cheat::UnlearnAbilities)),
            ],
        );
        self.row(
            body,
            &[
                Cell::new("teleport", "MOVE HERE", Press::Move),
                Cell::new("respawn", "RESPAWN", Hero(Cheat::Respawn)),
                Cell::new("remove", "REMOVE", Hero(Cheat::RemoveHero)),
            ],
        );
        self.row(
            body,
            &[
                Cell::new(
                    "invulnerable on",
                    "INVULNERABLE",
                    Hero(Cheat::Invulnerable { on: true }),
                ),
                Cell::new(
                    "invulnerable off",
                    "VULNERABLE",
                    Hero(Cheat::Invulnerable { on: false }),
                ),
            ],
        );
        self.row(
            body,
            &[
                Cell::new(
                    "gold(1000)",
                    "+1,000 GOLD",
                    Hero(Cheat::Gold { amount: 1000 }),
                ),
                Cell::new(
                    "gold(10000)",
                    "+10,000 GOLD",
                    Hero(Cheat::Gold { amount: 10_000 }),
                ),
            ],
        );
        let query = if shown.typing {
            format!("{}|", shown.search)
        } else {
            shown.search.clone()
        };
        let label = if query.is_empty() {
            "FREE ITEMS · SEARCH"
        } else {
            &query
        };
        self.row(
            body,
            &[
                Cell::new("items", label, Press::Search).fill(if shown.typing {
                    ACCENT
                } else {
                    BUTTON
                }),
            ],
        );
        let look = crate::tip_content::Lookup {
            texts: &self.art.texts,
            manifest: &self.art.manifest,
            rates: Default::default(),
        };
        let results: Vec<_> = item_results(&self.art.catalog, &look, &shown.search).collect();
        for items in results.chunks(2) {
            let names: Vec<_> = items
                .iter()
                .map(|item| {
                    (
                        format!("item {}", item.0),
                        look.item_name(item.0).unwrap_or_default(),
                        self.art.manifest.item_icon(item.0),
                    )
                })
                .collect();
            let cells: Vec<_> = items
                .iter()
                .zip(&names)
                .map(|(&item, (name, label, icon))| {
                    Cell::new(name, label, Press::Grant(item)).icon(icon.as_deref())
                })
                .collect();
            self.row(body, &cells);
        }
    }

    fn world_controls(&mut self, body: Entity, shown: &Shown) {
        use Press::World;
        self.label(body, "GLOBAL", ui::INK);
        self.row(
            body,
            &[
                Cell::new(
                    "all vision",
                    "ALL VISION",
                    World(Cheat::AllVision { on: true }),
                ),
                Cell::new(
                    "normal vision",
                    "NORMAL VISION",
                    World(Cheat::AllVision { on: false }),
                ),
            ],
        );
        self.row(
            body,
            &[
                Cell::new(
                    "creeps on",
                    "CREEPS ON",
                    World(Cheat::CreepSpawning { on: true }),
                ),
                Cell::new(
                    "creeps off",
                    "CREEPS OFF",
                    World(Cheat::CreepSpawning { on: false }),
                ),
            ],
        );
        self.row(
            body,
            &[
                Cell::new("free spells", "FREE SPELLS", World(Cheat::Wtf { on: true })),
                Cell::new(
                    "normal spells",
                    "NORMAL SPELLS",
                    World(Cheat::Wtf { on: false }),
                ),
            ],
        );
        self.row(
            body,
            &[
                Cell::new("day", "DAY", World(Cheat::TimeOfDay { night: false })),
                Cell::new("night", "NIGHT", World(Cheat::TimeOfDay { night: true })),
                Cell::new("refresh", "REFRESH ALL", World(Cheat::Refresh)),
            ],
        );
        if shown.quit {
            self.row(
                body,
                &[Cell::new("quit", "QUIT DEMO", Press::Quit).fill(ENEMY)],
            );
        }
    }
}
