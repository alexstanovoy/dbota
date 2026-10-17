//! Debug: with `DBOTA_DRILL` set, a live session plays a fixed drill through
//! the same intents the mouse and keys give (pick, ready, learn, buy, move,
//! attack-move, cast, stop), logs what each did to the hero, and exits. It
//! proves the wire end to end where nobody is at the keys.

use bevy::prelude::*;
use bota_client_core::{ClientSession, Intent, Slot};

use crate::session::Session;

/// Where the drill is: seconds since the hero was first seen, and the step
/// next due.
#[derive(Default)]
struct Drill {
    started: Option<f32>,
    step: usize,
    readied: bool,
}

pub fn plugin(app: &mut App) {
    if std::env::var_os("DBOTA_DRILL").is_some() {
        app.add_systems(Update, drill);
    }
}

fn drill(
    mut session: NonSendMut<Session>,
    time: Res<Time>,
    mut state: Local<Drill>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
) {
    let fps = diagnostics
        .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let core = &mut session.0;
    if !state.readied && core.my_slot.is_some() && core.view().is_none() {
        state.readied = true;
        pick(core);
    }
    let Some(hero) = core
        .my_hero()
        .and_then(|id| core.known(id))
        .map(|(u, _)| u.clone())
    else {
        return;
    };
    let now = time.elapsed_secs();
    let started = *state.started.get_or_insert(now);
    let at = now - started;
    let (x, y) = (hero.pos.x.to_f32(), hero.pos.y.to_f32());
    let gold = core
        .view()
        .and_then(|v| v.players.iter().find(|p| Some(p.slot) == core.my_slot))
        .and_then(|p| p.gold);
    let report = |what: &str| {
        info!(
            "drill {at:5.1}s {what}: hero at ({x:.0}, {y:.0}) hp {} mana {} gold {gold:?} ping {:?} ms, refusal {:?}, {fps:.0} fps, abilities {:?}",
            hero.hp,
            hero.mana,
            core.session().and_then(ClientSession::order_delay_ms),
            core.reject.as_ref().map(|r| r.0.clone()),
            hero.abilities
                .iter()
                .map(|a| (a.id.0, a.level, a.cooldown_left))
                .collect::<Vec<_>>()
        );
    };
    let steps = steps(&std::env::var("DBOTA_DRILL").unwrap_or_default());
    let Some(&(due, what)) = steps.get(state.step) else {
        return;
    };
    if at < due {
        return;
    }
    state.step += 1;
    report(what);
    let intent = match what {
        "gold" => {
            core.send_order(bota_proto::Order::Cheat {
                cheat: bota_proto::Cheat::Gold { amount: 3000 },
            });
            return;
        }
        "buy grenade" => Intent::Buy(53),
        "buy lace" => Intent::Buy(52),
        "walk to mid" => Intent::MoveTo((7600.0, 7600.0)),
        "photograph shop" | "photograph debuff" | "photograph look" => {
            photograph(
                &mut commands,
                format!("temp/shots/drill-{}.png", &what[11..]),
            );
            return;
        }
        "throw" | "select" => {
            let Some(view) = core.view() else {
                return;
            };
            let Some(at) = creep_within_throw(view, &hero) else {
                // Wait for the creeps to come within a throw, the clock
                // with the drill.
                state.step -= 1;
                state.started = Some(started + time.delta_secs());
                return;
            };
            if what == "select" {
                Intent::SelectAt(at)
            } else {
                let Some(slot) = hero
                    .items
                    .iter()
                    .position(|i| i.is_some_and(|i| i.id.0 == 53))
                else {
                    info!("drill: no grenade carried");
                    return;
                };
                core.act(Intent::Press {
                    slot: Slot::Item(slot as u8),
                    ctrl: false,
                });
                Intent::Aimed(at)
            }
        }
        "levels" => {
            core.send_order(bota_proto::Order::Cheat {
                cheat: bota_proto::Cheat::Levels { count: 14 },
            });
            return;
        }
        "learn talent 1" | "learn talent 2" => {
            core.learn_talent(if what.ends_with('1') { 1 } else { 2 });
            return;
        }
        "photograph" => {
            photograph(&mut commands, "temp/shots/drill-talents.png".into());
            return;
        }
        "learn Q" => Intent::Press {
            slot: Slot::Ability(0),
            ctrl: true,
        },
        "buy Tango" => match bota_client_core::first_answer("Tango") {
            Some(id) => Intent::Buy(id),
            None => return,
        },
        "move" => Intent::MoveTo((x + 600.0, y + 600.0)),
        "attack-move to mid" => {
            Intent::AttackMoveTo((crate::map::DOTA_ORIGIN, crate::map::DOTA_ORIGIN))
        }
        "cast Q" => Intent::Press {
            slot: Slot::Ability(0),
            ctrl: false,
        },
        "stop" => Intent::Stop,
        "exit" => {
            exit.write(AppExit::Success);
            return;
        }
        _ => return,
    };
    core.act(intent);
}

/// The steps of a drill, `DBOTA_DRILL`'s value: seconds after the hero is
/// first seen, and what is done then.
fn steps(kind: &str) -> &'static [(f32, &'static str)] {
    match kind {
        // Two minutes in the lane, for frame times (`--bench` ends it sooner).
        "bench" => &[(1.0, "attack-move to mid"), (120.0, "exit")],
        // The hero walked out of the fountain and photographed, to see it drawn.
        "look" => &[
            (1.0, "walk to mid"),
            (6.0, "photograph look"),
            (7.0, "exit"),
        ],
        // (Cheats, with --open-shop.) Two grenades and a lace bought, the
        // shop photographed, a grenade thrown at the enemy creeps at mid and
        // one of them photographed with its debuff.
        "items" => &[
            (0.5, "gold"),
            (1.5, "buy grenade"),
            (2.0, "buy grenade"),
            (2.5, "buy lace"),
            (4.0, "photograph shop"),
            (5.0, "walk to mid"),
            (20.0, "throw"),
            (21.0, "select"),
            (22.5, "photograph debuff"),
            (24.0, "exit"),
        ],
        // (Cheats.) To level 15, two talents learned, the tree pinned open
        // and photographed.
        "talents" => &[
            (0.5, "levels"),
            (2.0, "learn talent 1"),
            (3.0, "learn talent 2"),
            (5.0, "photograph"),
            (7.0, "exit"),
        ],
        _ => &[
            (0.5, "learn Q"),
            (1.5, "buy Tango"),
            (3.0, "move"),
            (10.0, "attack-move to mid"),
            (24.0, "cast Q"),
            (26.0, "stop"),
            (30.0, "report"),
            (31.0, "exit"),
        ],
    }
}

/// Saves the screen to a file.
fn photograph(commands: &mut Commands, path: String) {
    commands
        .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
        .observe(bevy::render::view::screenshot::save_to_disk(path));
}

/// Picks Shadow Fiend, whose first slot is a raze, or the hero named by
/// `DBOTA_DRILL_HERO`, and readies.
fn pick(core: &mut bota_client_core::App) {
    let name = std::env::var("DBOTA_DRILL_HERO").unwrap_or_else(|_| "Shadow Fiend".into());
    let hero = bota_core::HEROES
        .iter()
        .position(|h| h.name == name)
        .map_or(0, |id| id as u16);
    core.act(Intent::PickHero(hero));
    core.act(Intent::ToggleReady);
    info!("drill: picked {name}, ready");
}

/// The nearest enemy creep within a grenade's throw of the hero.
fn creep_within_throw(
    view: &bota_proto::WorldView,
    hero: &bota_proto::UnitView,
) -> Option<(f32, f32)> {
    let (x, y) = (hero.pos.x.to_f32(), hero.pos.y.to_f32());
    view.units
        .iter()
        .filter(|u| u.team != hero.team && bota_core::kind_def(u.kind).creep)
        .map(|u| (u.pos.x.to_f32(), u.pos.y.to_f32()))
        .filter(|(cx, cy)| (cx - x).hypot(cy - y) < 850.0)
        .min_by_key(|(cx, cy)| ((cx - x).hypot(cy - y)) as i64)
}
