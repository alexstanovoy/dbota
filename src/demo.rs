//! Dota's Demo Hero panel (the install's `hud_hero_demo`: a 280-pixel
//! column at the left, its categories SPAWN, UNITS and GLOBAL, Dota's
//! colours and its own words): every button sends one cheat, so the panel
//! does nothing a typed `-command` could not. It shows only in a Demo Hero
//! game.
//!
//! What lands at a spot (the creeps there, a teleport) waits for a click on
//! the ground; a right click lets it go.

use bevy::prelude::*;
use bota_client_core::{App as Core, Intent, world_vec};
use bota_proto::{Cheat, HeroId, Vec2};

use crate::dota_text::Texts;
use crate::hud::Blocker;
use crate::main_menu::{Act, MenuCore};
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// Gold one click of the panel gives, and its bigger button.
const GOLD_STEPS: [i32; 2] = [1_000, 10_000];
/// How far in front of one's hero a spawned hero stands, in world units:
/// clear of the fountain's reach when the hero stands at it.
const SPAWN_AHEAD: i32 = 1_600;
/// Items the search shows at most.
const FOUND_ITEMS: usize = 12;
/// The panel's width: Dota's `.ControlPanelContainer`.
const WIDTH: f32 = 280.0;

/// What waits for a click on the ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placing {
    /// Enemy creeps at the spot.
    Creeps,
    /// One's hero to the spot.
    Teleport,
}

/// The panel's state: its toggles as last sent, the hero to spawn, what
/// waits for a click, and the item search.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct DemoPanel {
    /// Shown, or folded away (Dota's slide thumb).
    pub open: bool,
    pub spawn_hero: u16,
    pub placing: Option<Placing>,
    pub free_spells: bool,
    /// Lane creeps spawn on their clock: off as Demo Hero starts.
    pub creeps: bool,
    pub all_vision: bool,
    pub invulnerable: bool,
    /// The item search as typed, and whether the keyboard is in it.
    pub search: String,
    pub typing: bool,
}

impl Default for DemoPanel {
    fn default() -> DemoPanel {
        DemoPanel {
            open: true,
            spawn_hero: HeroId::PUDGE.0,
            placing: None,
            free_spells: false,
            creeps: false,
            all_vision: false,
            invulnerable: false,
            search: String::new(),
            typing: false,
        }
    }
}

/// A press of the panel.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
enum Press {
    Fold,
    SpawnHero(u16),
    SpawnAlly,
    SpawnEnemy,
    LevelUp,
    LevelMax,
    Invulnerable,
    Reset,
    LevelEnemies,
    Respawn,
    Teleport,
    Gold(i32),
    FreeSpells,
    Creeps,
    Night,
    AllVision,
    CreepWave,
    CreepsAt,
    Neutrals,
    Refresh,
    Search,
    Grant(u16),
    Quit,
}

impl Press {
    /// The name a script clicks it by: `demo levelmax`, `demo gold(1000)`.
    fn name(self) -> String {
        format!("demo {self:?}").to_lowercase()
    }
}

#[derive(Component)]
struct PanelRoot;

pub fn plugin(app: &mut App) {
    app.init_resource::<DemoPanel>()
        .add_systems(Startup, spawn_root)
        .add_systems(PreUpdate, place.after(bevy::input::InputSystems))
        .add_systems(Update, (presses, draw).chain());
}

fn spawn_root(mut commands: Commands) {
    commands.spawn((
        PanelRoot,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(96),
            bottom: px(340),
            ..default()
        },
        Pickable::IGNORE,
        GlobalZIndex(40),
    ));
}

/// The hero of the Demo Hero game on screen, if one is.
fn demo_on_screen(menu: Option<&MenuCore>) -> bool {
    menu.is_some_and(|menu| menu.demo_hero().is_some() && menu.shows_game())
}

/// Sends one cheat.
fn cheat(core: &mut Core, cheat: Cheat) {
    core.act(Intent::Cheat(cheat));
}

/// The spot `SPAWN_AHEAD` in front of one's hero, towards the enemy's
/// fountain: where Dota's demo stands a spawned hero.
fn ahead_of_hero(core: &Core) -> Option<Vec2> {
    let hero = core
        .my_hero()
        .and_then(|id| core.known(id))
        .map(|(u, _)| u.pos)?;
    let session = core.session()?;
    let toward = bota_core::fountain_pos(session.map(), session.enemy());
    let (dx, dy) = ((toward.x - hero.x).to_f32(), (toward.y - hero.y).to_f32());
    let length = (dx * dx + dy * dy).sqrt().max(1.0);
    let ahead = SPAWN_AHEAD as f32 / length;
    Some(world_vec((
        hero.x.to_f32() + dx * ahead,
        hero.y.to_f32() + dy * ahead,
    )))
}

/// Whether it is night in the view now.
fn night(core: &Core) -> bool {
    core.view()
        .is_some_and(|view| bota_core::time_of_day(view.day_tick()).night)
}

/// The panel's buttons, pressed.
fn presses(
    mut session: NonSendMut<Session>,
    mut menu: Option<NonSendMut<MenuCore>>,
    mut panel: ResMut<DemoPanel>,
    buttons: Query<(&Interaction, &Press), Changed<Interaction>>,
    mut typing: ResMut<crate::input::Typing>,
) {
    if !demo_on_screen(menu.as_deref()) {
        return;
    }
    let core = &mut session.0;
    let pressed: Vec<Press> = buttons
        .iter()
        .filter(|(i, _)| **i == Interaction::Pressed)
        .map(|(_, p)| *p)
        .collect();
    for press in pressed {
        match press {
            Press::Fold => panel.open = !panel.open,
            Press::SpawnHero(hero) => panel.spawn_hero = hero,
            Press::SpawnAlly | Press::SpawnEnemy => {
                if let Some(at) = ahead_of_hero(core) {
                    let hero = HeroId(panel.spawn_hero);
                    let enemy = press == Press::SpawnEnemy;
                    cheat(core, Cheat::CreateHero { hero, enemy, at });
                }
            }
            Press::LevelUp => cheat(core, Cheat::Levels { count: 1 }),
            Press::LevelMax => cheat(
                core,
                Cheat::Levels {
                    count: bota_core::rules::HERO_MAX_LEVEL,
                },
            ),
            Press::Invulnerable => {
                panel.invulnerable = !panel.invulnerable;
                let on = panel.invulnerable;
                cheat(core, Cheat::Invulnerable { on });
            }
            Press::Reset => cheat(core, Cheat::ResetHero),
            Press::LevelEnemies => cheat(core, Cheat::LevelBots { count: 1 }),
            Press::Respawn => cheat(core, Cheat::Respawn),
            Press::Teleport => panel.placing = Some(Placing::Teleport),
            Press::Gold(amount) => cheat(core, Cheat::Gold { amount }),
            Press::FreeSpells => {
                panel.free_spells = !panel.free_spells;
                let on = panel.free_spells;
                cheat(core, Cheat::Wtf { on });
            }
            Press::Creeps => {
                panel.creeps = !panel.creeps;
                let on = panel.creeps;
                cheat(core, Cheat::CreepSpawning { on });
            }
            Press::Night => {
                let night = !night(core);
                cheat(core, Cheat::TimeOfDay { night });
            }
            Press::AllVision => {
                panel.all_vision = !panel.all_vision;
                let on = panel.all_vision;
                cheat(core, Cheat::AllVision { on });
            }
            Press::CreepWave => cheat(core, Cheat::SpawnCreeps),
            Press::CreepsAt => panel.placing = Some(Placing::Creeps),
            Press::Neutrals => cheat(core, Cheat::SpawnNeutrals),
            Press::Refresh => cheat(core, Cheat::Refresh),
            Press::Search => {
                panel.typing = true;
                *typing = crate::input::Typing::DemoSearch;
            }
            Press::Grant(item) => cheat(
                core,
                Cheat::Item {
                    item: bota_proto::ItemId(item),
                },
            ),
            Press::Quit => {
                if let Some(menu) = menu.as_mut() {
                    menu.act(Act::Disconnect);
                }
            }
        }
    }
}

/// A click on the ground for what waits for one; a right click lets it go.
/// The click is the panel's: the world never sees it.
fn place(
    mut session: NonSendMut<Session>,
    menu: Option<NonSend<MenuCore>>,
    mut panel: ResMut<DemoPanel>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    cursor: Res<crate::camera::CursorGround>,
    over: Res<crate::input::OverHud>,
) {
    let Some(placing) = panel.placing else {
        return;
    };
    if !demo_on_screen(menu.as_deref()) {
        panel.placing = None;
        return;
    }
    if mouse.just_pressed(MouseButton::Right) {
        mouse.clear_just_pressed(MouseButton::Right);
        panel.placing = None;
        return;
    }
    if over.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(at) = cursor.0.map(|p| world_vec((p.x, -p.z))) else {
        return;
    };
    mouse.clear_just_pressed(MouseButton::Left);
    panel.placing = None;
    let core = &mut session.0;
    match placing {
        Placing::Creeps => cheat(core, Cheat::CreateCreeps { enemy: true, at }),
        Placing::Teleport => cheat(core, Cheat::Teleport { at }),
    }
}

/// What the panel draws, to tell when it must be built again.
#[derive(Clone, PartialEq)]
struct Shown {
    panel: DemoPanel,
    night: bool,
    found: Vec<u16>,
}

/// Builds the panel again when what it shows changed; hides it outside a
/// Demo Hero game.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    session: NonSend<Session>,
    menu: Option<NonSend<MenuCore>>,
    panel: Res<DemoPanel>,
    texts: Res<Texts>,
    manifest: Res<Manifest>,
    catalog: Res<crate::shop::Catalog>,
    fonts: Res<Fonts>,
    server: Res<AssetServer>,
    root: Single<Entity, With<PanelRoot>>,
    mut drawn: Local<Option<Shown>>,
) {
    let core = &session.0;
    if !demo_on_screen(menu.as_deref()) {
        if drawn.take().is_some() {
            commands.entity(*root).despawn_children();
        }
        return;
    }
    let look = crate::tip_content::Lookup {
        texts: &texts,
        manifest: &manifest,
    };
    let found: Vec<u16> = catalog
        .search(&look, &panel.search)
        .into_iter()
        .filter(|id| bota_core::item_def(bota_proto::ItemId(*id)).is_some())
        .take(FOUND_ITEMS)
        .collect();
    let shown = Shown {
        panel: panel.clone(),
        night: night(core),
        found,
    };
    if drawn.as_ref() == Some(&shown) {
        return;
    }
    commands.entity(*root).despawn_children();
    let mut kit = Kit {
        commands: &mut commands,
        fonts: &fonts,
        texts: &texts,
        manifest: &manifest,
        server: &server,
    };
    build(&mut kit, *root, &shown);
    *drawn = Some(shown);
}

/// What the builders need.
struct Kit<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    fonts: &'a Fonts,
    texts: &'a Texts,
    manifest: &'a Manifest,
    server: &'a AssetServer,
}

/// The panel's colours: Dota's `hud_hero_demo` and `DemoButton`.
const PANEL_FROM: Color = Color::srgb(
    0x20 as f32 / 255.0,
    0x25 as f32 / 255.0,
    0x27 as f32 / 255.0,
);
const PANEL_TO: Color = Color::srgb(
    0x20 as f32 / 255.0,
    0x22 as f32 / 255.0,
    0x24 as f32 / 255.0,
);
const BUTTON: Color = Color::srgb(
    0x3e as f32 / 255.0,
    0x41 as f32 / 255.0,
    0x44 as f32 / 255.0,
);
const BUTTON_EDGE: Color = Color::srgb(
    0x4e as f32 / 255.0,
    0x51 as f32 / 255.0,
    0x55 as f32 / 255.0,
);
const BUTTON_INK: Color = Color::srgb(
    0xd8 as f32 / 255.0,
    0xda as f32 / 255.0,
    0xde as f32 / 255.0,
);
const RULE: Color = Color::srgba(
    0x4e as f32 / 255.0,
    0x51 as f32 / 255.0,
    0x55 as f32 / 255.0,
    0.6,
);
const LIT: Color = Color::srgb(
    0x7e as f32 / 255.0,
    0x94 as f32 / 255.0,
    0xb1 as f32 / 255.0,
);
const ALLY: Color = Color::srgb(0.16, 0.42, 0.18);
const ENEMY: Color = Color::srgb(0.48, 0.13, 0.1);

impl Kit<'_, '_, '_> {
    /// One of the demo's words from the install (`addon_english`), else this
    /// client's own.
    fn word(&self, key: &str, own: &str) -> String {
        self.texts.label(key, &[]).unwrap_or_else(|| own.to_owned())
    }

    fn node(&mut self, parent: Entity, node: Node) -> Entity {
        self.commands
            .spawn((node, Pickable::IGNORE, ChildOf(parent)))
            .id()
    }

    fn text(&mut self, parent: Entity, text: impl Into<String>, size: f32, color: Color) {
        self.commands.spawn((
            ui::text(text, &self.fonts.bold, size, color),
            Pickable::IGNORE,
            ChildOf(parent),
        ));
    }

    /// A category's header (`.CategoryHeader`).
    fn header(&mut self, parent: Entity, text: String) {
        let line = self.node(
            parent,
            Node {
                margin: UiRect::bottom(px(6)),
                ..default()
            },
        );
        self.text(
            line,
            text.to_uppercase(),
            15.0,
            Color::srgb(0.96, 0.96, 0.96),
        );
    }

    /// A row of buttons sharing its width (`.Row`).
    fn row(&mut self, parent: Entity) -> Entity {
        self.node(
            parent,
            Node {
                width: percent(100),
                column_gap: px(3),
                margin: UiRect::vertical(px(3)),
                ..default()
            },
        )
    }

    /// One of the demo's buttons (`.DemoButton`), lit while what it toggles
    /// is on, in a side's colour for spawning.
    fn button(&mut self, parent: Entity, label: String, press: Press, fill: Color) -> Entity {
        let button = self
            .commands
            .spawn((
                press,
                Button,
                Blocker,
                Name::new(press.name()),
                Node {
                    height: px(30),
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(fill),
                BorderColor::all(BUTTON_EDGE),
                ChildOf(parent),
            ))
            .id();
        self.commands.spawn((
            ui::text(label.to_uppercase(), &self.fonts.regular, 14.0, BUTTON_INK),
            TextLayout::justify(Justify::Center),
            Pickable::IGNORE,
            ChildOf(button),
        ));
        button
    }

    /// A toggle (`.DemoToggleButton`): lit while on.
    fn toggle(&mut self, parent: Entity, label: String, press: Press, on: bool) {
        self.button(parent, label, press, if on { LIT } else { BUTTON });
    }

    fn rule(&mut self, parent: Entity) {
        self.commands.spawn((
            Node {
                width: percent(100),
                height: px(1),
                margin: UiRect::vertical(px(6)),
                ..default()
            },
            BackgroundColor(RULE),
            Pickable::IGNORE,
            ChildOf(parent),
        ));
    }
}

/// The whole panel.
fn build(kit: &mut Kit, root: Entity, shown: &Shown) {
    let panel = &shown.panel;
    let column = kit
        .commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                height: percent(100),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    if panel.open {
        let body = kit
            .commands
            .spawn((
                Blocker,
                Interaction::default(),
                Node {
                    width: px(WIDTH),
                    height: percent(100),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(12), px(8)),
                    border: UiRect::all(px(1)),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ScrollPosition::default(),
                BackgroundGradient::from(LinearGradient::to_left(vec![
                    PANEL_FROM.into(),
                    PANEL_TO.into(),
                ])),
                BorderColor::all(Color::BLACK),
                ChildOf(column),
            ))
            .id();
        contents(kit, body, shown);
    }
    // Dota's slide thumb folds the panel away and back.
    let thumb = kit
        .commands
        .spawn((
            Press::Fold,
            Button,
            Blocker,
            Name::new("demo fold"),
            Node {
                width: px(25),
                height: px(120),
                align_self: AlignSelf::Center,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::right(px(10)),
                ..default()
            },
            BackgroundGradient::from(LinearGradient::to_right(vec![
                Color::srgb(0.07, 0.07, 0.07).into(),
                Color::srgb(0.23, 0.23, 0.23).into(),
            ])),
            ChildOf(column),
        ))
        .id();
    kit.text(thumb, if panel.open { "‹" } else { "›" }, 20.0, BUTTON_INK);
}

fn contents(kit: &mut Kit, body: Entity, shown: &Shown) {
    let panel = &shown.panel;
    let name = kit.word("addon_game_name", "Hero Demo");
    kit.text(body, name.to_uppercase(), 18.0, Color::WHITE);
    kit.rule(body);

    // SPAWN: the hero to spawn, and on which side.
    let spawn = kit.word("SpawnUnits_ButtonCategory", "Spawn");
    kit.header(body, spawn);
    let picker = kit
        .commands
        .spawn((
            Node {
                column_gap: px(6),
                padding: UiRect::all(px(4)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            BorderColor::all(RULE),
            Pickable::IGNORE,
            ChildOf(body),
        ))
        .id();
    for (id, hero) in bota_core::HEROES.iter().enumerate() {
        let Ok(id) = u16::try_from(id) else {
            continue;
        };
        let chosen = panel.spawn_hero == id;
        let card = kit
            .commands
            .spawn((
                Press::SpawnHero(id),
                Button,
                Blocker,
                Name::new(format!("demo hero {}", hero.name.to_lowercase())),
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    flex_grow: 1.0,
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BorderColor::all(if chosen { Color::WHITE } else { Color::NONE }),
                BackgroundGradient::from(LinearGradient::to_bottom(vec![
                    Color::srgb_u8(0x37, 0x3f, 0x43).into(),
                    Color::srgb_u8(0x21, 0x27, 0x2a).into(),
                ])),
                ChildOf(picker),
            ))
            .id();
        if let Some(path) = kit.manifest.portrait(id) {
            kit.commands.spawn((
                Node {
                    width: px(116),
                    height: px(64),
                    ..default()
                },
                ImageNode::new(kit.server.load(path)),
                Pickable::IGNORE,
                ChildOf(card),
            ));
        }
        kit.text(card, hero.name.to_uppercase(), 12.0, BUTTON_INK);
    }
    let row = kit.row(body);
    let ally = kit.word("SpawnAllyHero_Button", "Ally");
    kit.button(row, ally, Press::SpawnAlly, ALLY);
    let enemy = kit.word("SpawnEnemyHero_Button", "Enemy");
    kit.button(row, enemy, Press::SpawnEnemy, ENEMY);
    let row = kit.row(body);
    let wave = kit.word("SpawnCreepWave_Button", "+ Creep wave");
    kit.button(row, wave, Press::CreepWave, BUTTON);
    let at = if panel.placing == Some(Placing::Creeps) {
        "Click the ground…".to_owned()
    } else {
        "Creeps here".to_owned()
    };
    kit.button(row, at, Press::CreepsAt, ENEMY);
    let row = kit.row(body);
    let neutrals = kit.word("Neutrals_ButtonCategory", "Neutrals");
    kit.button(row, neutrals, Press::Neutrals, BUTTON);
    kit.rule(body);

    // UNITS: one's own hero, and the enemies'.
    let units = kit.word("EditUnits_ButtonCategory", "Units");
    kit.header(body, units);
    let row = kit.row(body);
    let up = kit.word("LevelUpSelectedHeroes_Button", "Level up");
    kit.button(row, up, Press::LevelUp, BUTTON);
    let max = kit.word("MaxLevelUpSelectedHeroes_Button", "Level max");
    kit.button(row, max, Press::LevelMax, BUTTON);
    let row = kit.row(body);
    let invuln = kit.word("Invuln_Button", "Invulnerable");
    kit.toggle(row, invuln, Press::Invulnerable, panel.invulnerable);
    let reset = kit.word("Reset_Button", "Reset");
    kit.button(row, reset, Press::Reset, BUTTON);
    let row = kit.row(body);
    kit.button(row, "Respawn".to_owned(), Press::Respawn, BUTTON);
    let teleport = if panel.placing == Some(Placing::Teleport) {
        "Click the ground…".to_owned()
    } else {
        "Teleport".to_owned()
    };
    kit.button(row, teleport, Press::Teleport, BUTTON);
    let row = kit.row(body);
    let enemies = kit.word("LevelUpEnemy_Button", "Level up enemy");
    kit.button(row, enemies, Press::LevelEnemies, BUTTON);
    let row = kit.row(body);
    for amount in GOLD_STEPS {
        kit.button(row, format!("+{amount} gold"), Press::Gold(amount), BUTTON);
    }
    // Items: typed into, the matches shown to grant.
    let search = kit
        .commands
        .spawn((
            Press::Search,
            Button,
            Blocker,
            Name::new("demo items"),
            Node {
                height: px(28),
                margin: UiRect::vertical(px(3)),
                padding: UiRect::horizontal(px(8)),
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            BorderColor::all(if panel.typing { LIT } else { BUTTON_EDGE }),
            ChildOf(body),
        ))
        .id();
    let typed = match (panel.search.is_empty(), panel.typing) {
        (true, false) => "Items: type to search".to_owned(),
        (_, true) => format!("{}|", panel.search),
        (false, false) => panel.search.clone(),
    };
    kit.text(search, typed, 14.0, BUTTON_INK);
    let grid = kit.node(
        body,
        Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(3),
            row_gap: px(3),
            ..default()
        },
    );
    for &item in &shown.found {
        let icon = kit.manifest.item_icon(item);
        let mut cell = kit.commands.spawn((
            Press::Grant(item),
            Button,
            Blocker,
            Name::new(format!("demo item {item}")),
            Node {
                width: px(40),
                height: px(30),
                border: UiRect::all(px(1)),
                ..default()
            },
            BorderColor::all(BUTTON_EDGE),
            ChildOf(grid),
        ));
        match icon {
            Some(path) => {
                cell.insert(ImageNode::new(kit.server.load(path)));
            }
            None => {
                cell.insert(BackgroundColor(BUTTON));
            }
        }
    }
    kit.rule(body);

    // GLOBAL: the match's own switches.
    let global = kit.word("Global_ButtonCategory", "Global");
    kit.header(body, global);
    let row = kit.row(body);
    let free = kit.word("FreeSpells_Button", "Free spells");
    kit.toggle(row, free, Press::FreeSpells, panel.free_spells);
    let creeps = kit.word("SpawnCreeps_Button", "Creeps");
    kit.toggle(row, creeps, Press::Creeps, panel.creeps);
    let row = kit.row(body);
    let night = kit.word("NightEnable_Button", "Night");
    kit.toggle(row, night, Press::Night, shown.night);
    let vision = kit.word("AllVision_Button", "All vision");
    kit.toggle(row, vision, Press::AllVision, panel.all_vision);
    let row = kit.row(body);
    let refresh = kit.word("Refresh_Button", "Refresh");
    kit.button(row, refresh, Press::Refresh, BUTTON);
    kit.rule(body);
    let row = kit.row(body);
    let quit = kit.word("QuitDemo_Button", "Quit");
    kit.button(row, quit, Press::Quit, ENEMY);
}

/// The item search's keys while it has the keyboard: typing, Backspace,
/// Enter to grant the first match, Escape to let go.
pub fn search_key(
    core: &mut Core,
    panel: &mut DemoPanel,
    key: &bevy::input::keyboard::Key,
    first: Option<u16>,
) -> bool {
    use bevy::input::keyboard::Key;
    match key {
        Key::Enter => {
            if let Some(item) = first {
                cheat(
                    core,
                    Cheat::Item {
                        item: bota_proto::ItemId(item),
                    },
                );
            }
            panel.typing = false;
            true
        }
        Key::Escape => {
            panel.typing = false;
            true
        }
        Key::Backspace => {
            panel.search.pop();
            false
        }
        Key::Space => {
            panel.search.push(' ');
            false
        }
        Key::Character(typed) if panel.search.len() < 24 => {
            panel.search.push_str(typed);
            false
        }
        _ => false,
    }
}
