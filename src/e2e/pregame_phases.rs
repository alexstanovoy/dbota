//! The pick and the strategy screens through the phases the server tells, with the server's
//! messages and the clock under the test's control: the countdown against the time told and the
//! frames' time (selecting, late, tactical), a seat given its hero when the time ran out, what a
//! click does once the picks are final, what is and is not sent for a choice and for LOCK IN, and
//! the order of the six figures of the hero panel.
//!
//! A real server brings the client to its pick screen; then the client's link is replaced by one
//! of the test's own, which the test feeds `LobbyState` messages and which records what the client
//! sends. The server's own timing (the clock, the penalty, the hero it draws) is the server's tests'
//! (`bota_server::pick_clock`): here the client is shown what the server says it does.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use bevy::prelude::*;
use bota_proto::{ClientMsg, HeroId, PickPhase, ServerMsg};

use super::harness::{Client, Server, quiet_seat, subtree};

const PATIENCE: Duration = Duration::from_secs(30);
/// One frame of the controlled clock.
const FRAME: Duration = Duration::from_millis(16);

/// The test's end of the client's link: what the server "says" and what the client said.
struct Link {
    inbox: Rc<RefCell<VecDeque<ServerMsg>>>,
    sent: Rc<RefCell<Vec<ClientMsg>>>,
}

impl bota_client_core::LiveLink for Link {
    fn poll(&mut self) -> Vec<ServerMsg> {
        self.inbox.borrow_mut().drain(..).collect()
    }
    fn send(&mut self, message: &ClientMsg) -> std::io::Result<()> {
        self.sent.borrow_mut().push(message.clone());
        Ok(())
    }
    fn is_closed(&self) -> bool {
        false
    }
}

/// The test's hold on the client's link, and the client.
struct Scripted {
    client: Client,
    inbox: Rc<RefCell<VecDeque<ServerMsg>>>,
    sent: Rc<RefCell<Vec<ClientMsg>>>,
    _server: Server,
}

/// A lobby of two seats whose first a bot holds, locked in with Pudge, the client on the second
/// at its pick screen, then its link replaced by the test's and its clock by a controlled one
/// (a frame is 16 ms).
fn scripted() -> Scripted {
    let server = Server::start(2, &["--pick-secs", "600", "--tactical-secs", "600"]);
    quiet_seat(&server, HeroId::PUDGE);
    let mut client = Client::run(&["--addr", &server.address]);
    client.until("the selection clock running", |c| {
        c.core().lobby.pick != PickPhase::Waiting
    });
    client.within(PATIENCE, "the loading screen lifted", |client| {
        let world = client.world();
        world.resource::<crate::loading::Loading>().done()
            && world.resource::<crate::trees::SceneryReady>().0
            && world.resource::<crate::minimap::PhotoTaken>().0
            && world
                .resource::<crate::loading::Parts>()
                .0
                .values()
                .all(|(done, total)| done == total)
    });
    let inbox = Rc::new(RefCell::new(VecDeque::new()));
    let sent = Rc::new(RefCell::new(Vec::new()));
    client.core_mut().source = bota_client_core::Source::Live(Box::new(Link {
        inbox: inbox.clone(),
        sent: sent.clone(),
    }));
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(FRAME));
    Scripted {
        client,
        inbox,
        sent,
        _server: server,
    }
}

impl Scripted {
    /// The server tells the lobby as it is, in `pick`, with the client's seat holding `hero`
    /// (readied when `locked`).
    fn tell(&mut self, pick: PickPhase, hero: Option<HeroId>, locked: bool) {
        let core = self.client.core();
        let mine = core.my_slot;
        let mut slots = core.lobby.slots.clone();
        for seat in &mut slots {
            if Some(seat.slot) == mine {
                seat.hero = hero;
                seat.ready = locked;
            }
        }
        self.inbox.borrow_mut().push_back(ServerMsg::LobbyState {
            slots,
            same_heroes: core.lobby.same_heroes,
            pick,
            heroes: core.lobby.heroes.clone(),
            catalogue: bota_core::Catalogue::FULL.prefix(),
        });
    }

    /// Whether a text shows on the screen.
    fn shows(&mut self, text: &str) -> bool {
        self.client.shown_texts().iter().any(|t| t == text)
    }

    /// The texts of the frame that hold a clock (`m:ss`).
    fn clocks(&mut self) -> Vec<String> {
        self.client
            .shown_texts()
            .into_iter()
            .filter(|t| {
                t.split_once(':').is_some_and(|(m, s)| {
                    m.len() <= 2
                        && s.len() == 2
                        && m.bytes().all(|b| b.is_ascii_digit())
                        && s.bytes().all(|b| b.is_ascii_digit())
                })
            })
            .collect()
    }
}

/// The big clock is the time the server told, minus the frames' time since: selecting, then the
/// late phase with its warning and the time left to the hero it will be given, then the tactical
/// phase of the seat that was given one, whose strategy screen counts its own time down.
#[test]
fn the_clock_follows_the_time_the_server_told_through_selecting_late_and_tactical() {
    let mut script = scripted();
    script.tell(PickPhase::Selecting { left_ms: 75_000 }, None, false);
    script.client.frames(2);
    assert_eq!(script.clocks(), ["1:15"], "75 s told, none gone");
    // 130 frames of 16 ms are 2.08 s: 72.9 s are left, which a clock rounds up to 1:13.
    script.client.frames(130);
    assert_eq!(script.clocks(), ["1:13"], "two seconds on");

    script.tell(PickPhase::Late { left_ms: 20_000 }, None, false);
    script.client.frames(2);
    assert_eq!(script.clocks(), ["0:20"], "the penalty's 20 s");
    assert!(script.shows("PICK NOW"), "the late phase's heading");
    assert!(
        script.shows("Losing 2 gold every second: a hero is given in 0:20"),
        "the warning with the time left"
    );

    // The time is out: the server has given the seat Shadow Fiend and begins the tactical phase.
    script.tell(
        PickPhase::Tactical { left_ms: 30_000 },
        Some(HeroId::SHADOW_FIEND),
        true,
    );
    script.client.frames(3);
    assert!(
        script.client.core().in_tactical_phase(),
        "the strategy phase"
    );
    assert_eq!(script.clocks(), ["0:30"], "the tactical phase's 30 s");
    assert!(script.shows("SHADOW FIEND"), "the hero it was given");
    script.client.frames(130);
    assert_eq!(script.clocks(), ["0:28"], "two seconds on");
}

/// A click that was on its way when the picks became final sends nothing: the selection clock
/// can finalize a hero that was picked and not readied, so readiness is no fence.
#[test]
fn a_click_that_lands_once_the_picks_are_final_sends_nothing() {
    let mut script = scripted();
    script.client.hover_ui_with(PATIENCE, |c| {
        let world = c.world_mut();
        world
            .query::<(Entity, &Name)>()
            .iter(world)
            .find(|(_, n)| n.as_str() == "pick random")
            .map(|(e, _)| e)
    });
    // The seat picked a hero, did not ready it, and the time ran out: the tactical phase began
    // in the frame the click is made.
    let mine = script.client.core().my_slot;
    let core = script.client.core_mut();
    for seat in &mut core.lobby.slots {
        if Some(seat.slot) == mine {
            seat.hero = Some(HeroId::SHADOW_FIEND);
            seat.ready = false;
        }
    }
    core.ready = false;
    core.lobby.pick = PickPhase::Tactical { left_ms: 30_000 };
    script.client.click(MouseButton::Left);
    assert!(
        script.sent.borrow().is_empty(),
        "nothing sent after the picks are final: {:?}",
        script.sent.borrow()
    );
}

/// What goes upstream: a choice nothing (and no other seat sees it), LOCK IN exactly the pick and
/// then the readiness, a later click on a card, RANDOM or LOCK IN nothing.
#[test]
fn a_choice_sends_nothing_and_lock_in_sends_the_pick_and_the_readiness_once() {
    let mut script = scripted();
    script.client.click_named("pick shadow fiend");
    assert!(script.sent.borrow().is_empty(), "no message for a choice");
    script.client.click_named("pick ready");
    assert_eq!(
        script.sent.borrow().as_slice(),
        [
            ClientMsg::PickHero {
                hero: HeroId::SHADOW_FIEND
            },
            ClientMsg::SetReady(true)
        ],
        "the pick, then the readiness"
    );
    script.sent.borrow_mut().clear();
    script.client.click_named("pick random");
    script.client.click_named("pick pudge");
    script.client.click_named("pick ready");
    assert!(
        script.sent.borrow().is_empty(),
        "nothing more once locked in: {:?}",
        script.sent.borrow()
    );
}

/// The six figures of the hero panel are the original's, in its order and with its icons: down
/// the left column damage, armor and move speed, down the right the attack rate, the attack speed
/// and the attack range. Read off what is drawn, by position, for Shadow Fiend whose figures the
/// install's hero data gives (damage 44 is the average of 16..22 and the 25 agility; 125 is the
/// 100 base attack speed and the 25 agility; 1.6 is the base attack time).
#[test]
fn the_panels_six_figures_are_drawn_in_the_originals_order_with_its_icons() {
    let mut script = scripted();
    script.client.click_named("pick shadow fiend");
    script.client.within(PATIENCE, "the panel's figures", |c| {
        c.shown_texts().iter().any(|t| t == "125")
    });
    let client = &mut script.client;
    let block = {
        let world = client.world_mut();
        world
            .query::<(Entity, &Name)>()
            .iter(world)
            .find(|(_, name)| name.as_str() == "pick panel stats")
            .map(|(entity, _)| entity)
            .expect("the figures' block")
    };
    let world = client.world();
    let manifest = world.resource::<crate::manifest::Manifest>();
    let server = world.resource::<AssetServer>();
    // (middle across, middle down, what it is): an icon by its image's path, a figure by its text.
    let mut icons = Vec::new();
    let mut figures = Vec::new();
    for entity in subtree(world, block) {
        let Some(at) = world.get::<bevy::ui::UiGlobalTransform>(entity) else {
            continue;
        };
        let at = at.translation;
        if let Some(image) = world.get::<ImageNode>(entity)
            && let Some(path) = server.get_path(image.image.id())
        {
            icons.push((at, path.to_string()));
        }
        if let Some(text) = world.get::<Text>(entity) {
            figures.push((at, text.0.clone()));
        }
    }
    let middle = world
        .get::<bevy::ui::UiGlobalTransform>(block)
        .expect("laid out")
        .translation
        .x;
    let reading = |left: bool| {
        let mut column: Vec<_> = figures
            .iter()
            .filter(|(at, text)| (at.x < middle) == left && !text.is_empty() && text != "RANGED")
            .collect();
        column.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
        column
            .into_iter()
            .map(|(at, text)| {
                // Its icon: the one on the same row, nearest to the left of the figure.
                let icon = icons
                    .iter()
                    .filter(|(icon, _)| (icon.y - at.y).abs() < 4.0 && icon.x < at.x)
                    .max_by(|a, b| a.0.x.total_cmp(&b.0.x))
                    .map(|(_, path)| path.clone());
                (icon, text.clone())
            })
            .collect::<Vec<_>>()
    };
    let icon = |logical: &str| manifest.file(logical);
    assert_eq!(
        reading(true),
        [
            (icon("hud.damage"), "44".to_owned()),
            (icon("hud.armor"), "4.2".to_owned()),
            (icon("hud.speed"), "305".to_owned()),
        ],
        "the left column"
    );
    assert_eq!(
        reading(false),
        [
            (icon("hud.attack_speed2"), "1.6".to_owned()),
            (icon("hud.attack_speed3"), "125".to_owned()),
            (icon("hud.attack_range"), "525".to_owned()),
        ],
        "the right column"
    );
}
