//! Debug: with `DBOTA_DRILL` set, a live session plays a fixed drill through
//! the same intents the mouse and keys give (pick, ready, learn, buy, move,
//! attack-move, cast, stop, walks queued with Shift), logs what each did to
//! the hero, and exits. It proves the wire end to end where nobody is at the
//! keys.

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use bota_client_core::{Intent, Slot};

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
        // Shadow Fiend: his first slot is a raze.
        core.act(Intent::PickHero(bota_proto::HeroId::SHADOW_FIEND.0));
        core.act(Intent::ToggleReady);
        info!("drill: picked Shadow Fiend, ready");
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
        .and_then(|v| v.player(core.my_slot?))
        .and_then(|p| p.gold);
    let report = |what: &str| {
        info!(
            "drill {at:5.1}s {what}: tick {:?} {:?} hero at ({x:.0}, {y:.0}) hp {} mana {} queued {} gold {gold:?} ping {:?} ms, refusal {:?}, {fps:.0} fps, abilities {:?}",
            core.view().map(|v| v.tick),
            hero.id,
            hero.hp,
            hero.mana,
            hero.queue.len(),
            core.source.ping().map(|ping| ping.as_millis()),
            core.reject.as_ref().map(|r| crate::error_msg::plain(&r.0)),
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
        "learn Q" => Intent::Press {
            slot: Slot::Ability(0),
            ctrl: true,
        },
        "buy Tango" => Intent::Buy(bota_proto::ItemId::TANGO.0),
        "move" => Intent::MoveTo((x + 600.0, y + 600.0)),
        // Shift held: each walk waits for the one before it.
        "queue a walk" | "queue another walk" => {
            let to = if what == "queue a walk" {
                (x + 600.0, y + 1200.0)
            } else {
                (x, y + 1200.0)
            };
            core.queueing = true;
            core.act(Intent::MoveTo(to));
            core.queueing = false;
            return;
        }
        "attack-move to mid" => {
            Intent::AttackMoveTo((crate::map::DOTA_ORIGIN, crate::map::DOTA_ORIGIN))
        }
        "cast Q" => Intent::Press {
            slot: Slot::Ability(0),
            ctrl: false,
        },
        "stop" => Intent::Stop,
        "shot" => {
            shoot(&mut commands, at);
            return;
        }
        "exit" => {
            exit.write(AppExit::Success);
            return;
        }
        _ => return,
    };
    core.act(intent);
}

/// Saves the screen, as `DBOTA_DRILL_SHOTS`'s folder, `link-` and the seconds
/// into the drill name it.
fn shoot(commands: &mut Commands, at: f32) {
    let Some(dir) = std::env::var_os("DBOTA_DRILL_SHOTS") else {
        return;
    };
    let path = std::path::Path::new(&dir).join(format!("link-{:05.1}.png", at));
    if let Err(err) = std::fs::create_dir_all(&dir) {
        error!("screenshot folder: {err}");
    }
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

/// The steps of a drill, `DBOTA_DRILL`'s value: seconds after the hero is
/// first seen, and what is done then.
fn steps(kind: &str) -> Vec<(f32, &'static str)> {
    match kind {
        // A long walk with a report and a screenshot every second and a
        // half, to watch the link break and mend meanwhile.
        "link" => {
            let mut steps = vec![(1.0, "attack-move to mid")];
            steps.extend((1..=80).flat_map(|n| {
                let at = 1.0 + n as f32 * 1.5;
                [(at, "report"), (at, "shot")]
            }));
            steps.push((125.0, "exit"));
            steps
        }
        // A walk with two more queued behind it (Shift), shot while the flags
        // stand and once the queue is walked.
        "queue" => vec![
            (0.5, "move"),
            (0.6, "queue a walk"),
            (0.7, "queue another walk"),
            (1.5, "report"),
            (1.5, "shot"),
            (12.0, "report"),
            (12.0, "shot"),
            (13.0, "exit"),
        ],
        // Two minutes in the lane, for frame times (`--bench` ends it sooner).
        "bench" => vec![(1.0, "attack-move to mid"), (120.0, "exit")],
        _ => vec![
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
