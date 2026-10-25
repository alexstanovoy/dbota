//! The first start: with no asset cache made from the local Dota 2 install,
//! the menu says so at its foot and imports it with a click, the work
//! `dbota import` does, its progress shown; then a restart picks the new
//! cache up (models, the map and sounds are read as the client starts).
//!
//! A cache another dbota made (its stamp is not of this build's asset
//! fingerprint) may lack what this one draws: a screen over everything holds
//! the menu (no game starts, not even one the command line names) until it
//! is imported again, with the button right there. Unattended runs (captures,
//! checks) are only warned.

use std::sync::{
    Mutex,
    mpsc::{Receiver, channel},
};

use bevy::prelude::*;

use crate::import::Need;
use crate::main_menu::{MenuCore, Page};
use crate::ui::{self, Fonts};

/// Where the import is.
#[derive(Clone, Debug, PartialEq)]
enum State {
    /// Nothing to say: the cache is this build's.
    Fine,
    Needed(Need),
    /// The share done and the asset at hand.
    Importing(f32, String),
    Done,
    Failed(String),
}

/// What the import thread tells.
enum Told {
    Progress(f32, String),
    Finished(Result<bool, String>),
}

#[derive(Resource)]
struct Notice {
    state: State,
    told: Option<Mutex<Receiver<Told>>>,
    /// The cache is out of date for this build: the screen holds everything until a restart
    /// reads the new one.
    holds: bool,
}

/// The line at the menu's foot.
#[derive(Component)]
struct Root;

/// What its button does.
#[derive(Component, Clone, Copy)]
enum Press {
    Import,
    Restart,
    Quit,
}

pub fn plugin(app: &mut App) {
    let args = app.world().resource::<crate::Args>();
    let need = crate::import::need(&args.cache_dir);
    let holds = holds(need, args);
    let state = need.map_or(State::Fine, State::Needed);
    app.insert_resource(Notice {
        state,
        told: None,
        holds,
    })
    .insert_resource(crate::main_menu::Held(holds))
    .add_systems(Update, (listen, presses, draw).chain());
}

/// Whether the cache at hand keeps this run from playing: one made by another dbota, in a run a
/// person plays (an unattended one is only warned).
pub fn holds(need: Option<Need>, args: &crate::Args) -> bool {
    need == Some(Need::Reimport) && !args.unattended()
}

/// Takes in what the import thread said since the last frame.
fn listen(mut notice: ResMut<Notice>) {
    let Some(told) = &notice.told else {
        return;
    };
    let mut last = None;
    if let Ok(told) = told.lock() {
        while let Ok(t) = told.try_recv() {
            last = Some(t);
        }
    }
    match last {
        Some(Told::Progress(done, item)) => notice.state = State::Importing(done, item),
        Some(Told::Finished(result)) => {
            notice.told = None;
            notice.state = match result {
                Ok(true) => State::Done,
                Ok(false) => State::Failed(
                    "imported, but some assets did not convert (they are drawn as stand-ins; \
                     the terminal names them)"
                        .to_owned(),
                ),
                Err(why) => State::Failed(why),
            };
        }
        None => {}
    }
}

fn presses(
    mut notice: ResMut<Notice>,
    buttons: Query<(&Interaction, &Press), Changed<Interaction>>,
    args: Res<crate::Args>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, press) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match press {
            Press::Import if notice.told.is_none() => {
                let (tell, told) = channel();
                let cache = args.cache_dir.clone();
                std::thread::spawn(move || {
                    let progress = tell.clone();
                    let result = crate::import::run_with(None, &cache, true, &mut |done, item| {
                        let _ = progress.send(Told::Progress(done, item.to_owned()));
                    });
                    let _ = tell.send(Told::Finished(result));
                });
                notice.told = Some(Mutex::new(told));
                notice.state = State::Importing(0.0, String::new());
            }
            Press::Import => {}
            Press::Quit => {
                exit.write(AppExit::Success);
            }
            Press::Restart => {
                let again = std::env::current_exe().and_then(|exe| {
                    std::process::Command::new(exe)
                        .args(std::env::args_os().skip(1))
                        .spawn()
                });
                match again {
                    Ok(_) => {
                        exit.write(AppExit::Success);
                    }
                    Err(why) => {
                        notice.state = State::Failed(format!("cannot start dbota again: {why}"))
                    }
                }
            }
        }
    }
}

/// What the line says, and its button.
fn words(state: &State) -> Option<(String, Option<(&'static str, Press)>)> {
    Some(match state {
        State::Fine => return None,
        State::Needed(Need::Import) => (
            "No Dota 2 assets yet: heroes, the map and sounds are drawn as stand-ins. \
             dbota converts them from your Dota 2 install, on this machine (about a minute)."
                .to_owned(),
            Some(("IMPORT FROM DOTA 2", Press::Import)),
        ),
        State::Needed(Need::Reimport) => (
            "The Dota 2 assets were imported by another dbota version: some may be missing."
                .to_owned(),
            Some(("IMPORT AGAIN", Press::Import)),
        ),
        State::Importing(done, item) => (
            format!("Importing from Dota 2… {:.0} %  {item}", done * 100.0),
            None,
        ),
        State::Done => (
            "Dota 2 assets imported. dbota reads them as it starts.".to_owned(),
            Some(("RESTART DBOTA", Press::Restart)),
        ),
        State::Failed(why) => (
            format!("Import failed: {why}"),
            Some(("TRY AGAIN", Press::Import)),
        ),
    })
}

/// The screen holding a run whose cache is out of date: its title, what it says and its buttons.
fn held_words(state: &State) -> (&'static str, String, Vec<(&'static str, Press)>) {
    let quit = ("QUIT", Press::Quit);
    let fingerprint = dbota_import::import::fingerprint();
    match state {
        State::Importing(done, item) => (
            "Importing from Dota 2",
            format!("{:.0} %  {item}", done * 100.0),
            vec![],
        ),
        State::Done => (
            "Dota 2 assets imported",
            "dbota reads them as it starts.".to_owned(),
            vec![("RESTART DBOTA", Press::Restart), quit],
        ),
        State::Failed(why) => (
            "The import failed",
            why.clone(),
            vec![("TRY AGAIN", Press::Import), quit],
        ),
        _ => (
            "The assets are out of date for this version",
            format!(
                "They were imported by another dbota and may lack what this one draws \
                 (it needs assets {fingerprint}). Re-import them from your Dota 2 folder: \
                 about a minute, on this machine."
            ),
            vec![("RE-IMPORT FROM DOTA 2", Press::Import), quit],
        ),
    }
}

/// A button of the notice, named after its label for scripts (`click:quit dbota` for QUIT, which
/// the menu's own `quit` must not be taken for).
fn button(commands: &mut Commands, fonts: &Fonts, parent: Entity, label: &str, press: Press) {
    let name = match press {
        Press::Quit => "quit dbota".to_owned(),
        _ => label.to_lowercase(),
    };
    let button = commands
        .spawn((
            press,
            Button,
            Node {
                height: px(40),
                padding: UiRect::axes(px(18), px(0)),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(match press {
                Press::Quit => ui::PANEL_DEEP,
                _ => Color::srgb(0.12, 0.3, 0.14),
            }),
            Name::new(name),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        ui::text(label, &fonts.bold, 16.0, ui::INK),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

/// Draws the screen that holds the run: over everything, the menu, a game and the settings.
fn draw_held(commands: &mut Commands, fonts: &Fonts, state: &State) {
    let (title, text, buttons) = held_words(state);
    let screen = commands
        .spawn((
            Root,
            crate::hud::Blocker,
            Interaction::default(),
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(18),
                ..ui::fill()
            },
            // Opaque: nothing of what it holds shows through.
            BackgroundColor(Color::srgb(0.02, 0.025, 0.035)),
            GlobalZIndex(90),
        ))
        .id();
    let title = commands
        .spawn((
            ui::text(title, &fonts.bold, 28.0, ui::GOLDEN),
            ChildOf(screen),
        ))
        .id();
    commands.entity(title).insert(Pickable::IGNORE);
    commands.spawn((
        ui::text(text, &fonts.regular, 17.0, ui::INK),
        Node {
            max_width: px(760),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(screen),
    ));
    let row = commands
        .spawn((
            Node {
                column_gap: px(14),
                ..default()
            },
            ChildOf(screen),
        ))
        .id();
    for (label, press) in buttons {
        button(commands, fonts, row, label, press);
    }
}

/// Draws the line anew when the state changes; shown while the menu covers
/// the screen (not over a game, not under the settings).
fn draw(
    mut commands: Commands,
    notice: Res<Notice>,
    fonts: Res<Fonts>,
    menu: Option<NonSend<MenuCore>>,
    root: Query<(Entity, &mut Visibility), With<Root>>,
    mut drawn: Local<Option<State>>,
) {
    let shown = notice.holds || menu.is_some_and(|m| !m.shows_game() && m.page() != Page::Settings);
    let mut root = root;
    if drawn.as_ref() == Some(&notice.state) {
        for (_, mut visibility) in &mut root {
            visibility.set_if_neq(ui::visible(shown));
        }
        return;
    }
    *drawn = Some(notice.state.clone());
    for (entity, _) in &root {
        commands.entity(entity).despawn();
    }
    if notice.holds {
        draw_held(&mut commands, &fonts, &notice.state);
        return;
    }
    let Some((text, button)) = words(&notice.state) else {
        return;
    };
    let failed = matches!(notice.state, State::Failed(_));
    let line = commands
        .spawn((
            Root,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(32),
                left: percent(50),
                width: px(820),
                margin: UiRect::left(px(-410)),
                padding: UiRect::axes(px(20), px(14)),
                column_gap: px(18),
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(ui::PANEL),
            BorderColor::all(if failed { ERROR } else { ui::GOLDEN }),
            GlobalZIndex(62),
            ui::visible(shown),
        ))
        .id();
    commands.spawn((
        ui::text(text, &fonts.regular, 16.0, ui::INK),
        Node {
            flex_grow: 1.0,
            flex_shrink: 1.0,
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(line),
    ));
    if let Some((label, press)) = button {
        self::button(&mut commands, &fonts, line, label, press);
    }
}

/// A failed import's edge.
const ERROR: Color = Color::srgb(0.9, 0.35, 0.3);
