//! Debug: with `DBOTA_DRILL` set, a live session plays a fixed drill through
//! the same intents the mouse and keys give (pick, ready, learn, buy, move,
//! attack-move, cast, stop), logs what each did to the hero, and exits. It
//! proves the wire end to end where nobody is at the keys.

use bevy::prelude::*;
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
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
) {
    let fps = diagnostics
        .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let core = &mut session.0;
    if !state.readied && core.my_slot.is_some() && core.view.is_none() {
        state.readied = true;
        // Shadow Fiend, whose first slot is a raze.
        core.act(Intent::PickHero(2));
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
        .view
        .as_ref()
        .and_then(|v| v.players.iter().find(|p| Some(p.slot) == core.my_slot))
        .and_then(|p| p.gold);
    let report = |what: &str| {
        info!(
            "drill {at:5.1}s {what}: hero at ({x:.0}, {y:.0}) hp {} mana {} gold {gold:?} ping {:?} ms, refusal {:?}, {fps:.0} fps, abilities {:?}",
            hero.hp,
            hero.mana,
            core.order_delay_ms(),
            core.reject.as_ref().map(|r| r.0.clone()),
            hero.abilities
                .iter()
                .map(|a| (a.id.0, a.level, a.cooldown_left))
                .collect::<Vec<_>>()
        );
    };
    let steps: [(f32, &str); 8] = [
        (0.5, "learn Q"),
        (1.5, "buy Tango"),
        (3.0, "move"),
        (10.0, "attack-move to mid"),
        (24.0, "cast Q"),
        (26.0, "stop"),
        (30.0, "report"),
        (31.0, "exit"),
    ];
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
        "buy Tango" => match bota_client_core::first_answer("Tango") {
            Some(id) => Intent::Buy(id),
            None => return,
        },
        "move" => Intent::MoveTo((x + 600.0, y + 600.0)),
        "attack-move to mid" => Intent::AttackMoveTo((9216.0, 9216.0)),
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
