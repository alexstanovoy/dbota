//! The end of a match of the service, as its player lives it in the real client: once the end has
//! been shown, the client goes by itself to the match's result page in the menu, its connection
//! to the game server closed by itself (no lost connection, nothing left to rejoin), and the
//! account may look for its next match at once.

use std::time::Duration;

use bevy::time::TimeUpdateStrategy;
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, Target, Team, UnitKind};

use super::harness::Client;
use super::menu::{act, signed_up};
use crate::finale::{Finale, LONGEST_SECS};
use crate::main_menu::{Act, LobbyAction, MenuCore, Page};

const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 30);

/// Enables the same setup cheats as Demo Hero for the selected mode in this test service.
fn service_with_cheats(mode: &str, save_replays: bool) -> crate::main_menu::tests::Service {
    crate::main_menu::tests::start_service(save_replays, |mut config| {
        let ruleset = config
            .find(&format!("id = \"{mode}\""))
            .expect("the ruleset");
        let args = ruleset + config[ruleset..].find("args = [").expect("its arguments");
        // The local fixture for the two-player result needs cheats with two reserved seats.
        let cheats = if mode == "solo_mid" {
            "\"--lobby-cheats\", "
        } else {
            "\"--cheats\", "
        };
        config.insert_str(args + "args = [".len(), cheats);
        config
    })
}

fn menu(client: &Client) -> &MenuCore {
    client.world().non_send::<MenuCore>()
}

fn launch_lobby(client: &mut Client, mode: &str) {
    act(client, Act::Go(Page::Lobbies));
    act(client, Act::LobbyUi(LobbyAction::Create));
    client.until("the lobby made", |c| {
        menu(c).page() == Page::Lobby && menu(c).view().lobby_ui.editor.is_none()
    });
    act(client, Act::LobbyUi(LobbyAction::Edit));
    act(
        client,
        Act::LobbyUi(LobbyAction::EditMode(bota_radovid::proto::RulesetId(
            mode.to_owned(),
        ))),
    );
    act(client, Act::LobbyUi(LobbyAction::Save));
    client.until("the lobby mode saved", |c| {
        menu(c).view().lobby_ui.editor.is_none()
    });
    if mode == "full" {
        let crate::main_menu::PageView::Lobby(lobby) = menu(client).view().body else {
            panic!("the full lobby room is visible");
        };
        assert_eq!(lobby.settings.mode, "Full rules");
        assert_eq!(
            lobby
                .teams
                .iter()
                .map(|team| team.slots.len())
                .collect::<Vec<_>>(),
            [5, 5]
        );
    }
    act(client, Act::ReadyLobby(true));
    client.until("the hero on the map", |c| {
        let core = c.core_mut();
        if core.view().is_none() && core.my_slot.is_some() && !core.ready {
            core.act(Intent::PickHero(HeroId::SHADOW_FIEND.0));
            core.act(Intent::ToggleReady);
        }
        core.ears().is_some()
    });
    client.cheat(Cheat::StartGame);
}

#[test]
fn leaving_a_service_match_shows_a_readable_result_reason_without_a_finale() {
    use bevy::prelude::*;

    let service = service_with_cheats("practice", true);
    let mut client = signed_up(&service, "leaver");
    launch_lobby(&mut client, "practice");
    client.until("the match may be left", |c| menu(c).view().can_leave_game);
    act(&mut client, Act::LeaveGame);
    act(&mut client, Act::Answer(true));
    client.until("the menu shows the reason for the defeat", |c| {
        c.shown_texts()
            .iter()
            .any(|text| text.contains("a player left"))
    });
    assert_eq!(menu(&client).page(), Page::Result);
    assert!(client.world().resource::<Finale>().run.is_none());
    let world = client.world_mut();
    let mut details = world.query::<(&Name, &Text, &TextColor, &ChildOf)>();
    let (_, text, color, parent) = details
        .iter(world)
        .find(|(name, ..)| name.as_str() == "match result detail")
        .expect("match reason is drawn in the result header");
    assert!(text.0.contains("a player left"));
    assert_eq!(color.0, crate::theme::INK_2);
    assert_eq!(
        world
            .get::<BackgroundColor>(parent.parent())
            .map(|background| background.0),
        Some(crate::theme::RESULT_PANEL),
        "the reason has a solid dark background over the menu artwork"
    );
}

#[test]
fn a_service_match_ends_on_its_result_page_by_itself_with_the_game_left_and_the_account_free() {
    let service = service_with_cheats("full", true);
    let mut client = signed_up(&service, "alice");
    launch_lobby(&mut client, "full");
    client.cheat(Cheat::AllVision { on: true });
    let ancient = |c: &Client| {
        c.view()
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Ancient && u.team == Team::Dire)
            .map(|u| u.id)
    };
    client.until("the Dire Ancient in sight", |c| ancient(c).is_some());
    let target = ancient(&client).expect("in sight");
    client.core_mut().act(Intent::Cheat(Cheat::Destroy {
        target: Target::Unit(target),
    }));
    client.until("the end shown", |c| {
        c.world().resource::<Finale>().run.is_some()
    });
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    let mut shown = 0.0f32;
    while menu(&client).page() == Page::Game {
        assert!(
            shown < LONGEST_SECS + 2.0,
            "the client is still in the game {shown} s after the decision"
        );
        assert!(!client.core().lost, "the connection was never lost");
        client.step_frames(1);
        shown += FRAME.as_secs_f32();
    }
    assert!(!menu(&client).in_game(), "the game is left");
    assert!(
        client.core().session().is_none() && client.core().over.is_none(),
        "the client let go of the game server: the menu's idle session is on"
    );
    client.until("the result page", |c| menu(c).page() == Page::Result);
    assert_eq!(
        crate::error_msg::shown(client.world_mut()),
        None,
        "no error told"
    );
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "VICTORY" || text == "RADIANT VICTORY"),
        "the result says who won: {:?}",
        client.shown_texts()
    );
    assert!(!menu(&client).view().reconnect, "nothing is left to rejoin");
    act(
        &mut client,
        Act::Find(bota_radovid::proto::RulesetId("solo_mid".to_owned())),
    );
    client.until("the account looks for its next match", |c| {
        menu(c).view().searches.len() == 1
    });
}

/// Runs a frame of each client, on the frame clock both are given, until `done` holds for both.
fn both(
    clients: &mut [&mut Client; 2],
    what: &str,
    mut done: impl FnMut(usize, &mut Client) -> bool,
) {
    let mut frames = 0u32;
    loop {
        let mut all = true;
        for (seat, client) in clients.iter_mut().enumerate() {
            all &= done(seat, client);
        }
        if all {
            return;
        }
        frames += 1;
        assert!(frames < 30 * 600, "{what}: not reached");
        for client in clients.iter_mut() {
            client.step_frames(1);
        }
    }
}

/// Two players of the service play a rated 1x1 to its end: both are shown the end, both land on
/// the result page by themselves with their rating change on its first frame (the service rates
/// the match when the game says its result, not when the game server has let them go), and both
/// may look for their next match at once.
#[test]
fn both_players_of_a_rated_match_see_the_end_and_land_on_their_rating_change() {
    let service = service_with_cheats("solo_mid", false);
    let mut alice = signed_up(&service, "alice");
    let mut bob = signed_up(&service, "bob");
    let solo = || bota_radovid::proto::RulesetId("solo_mid".to_owned());
    act(&mut alice, Act::Find(solo()));
    act(&mut bob, Act::Find(solo()));
    let mut players = [&mut alice, &mut bob];
    both(&mut players, "the offer", |_, c| {
        menu(c).view().offer.is_some()
    });
    for client in players.iter_mut() {
        let proposal = menu(client).view().offer.expect("an offer").proposal;
        act(client, Act::Accept(proposal));
    }
    both(&mut players, "both heroes on the map", |_, c| {
        let core = c.core_mut();
        if core.view().is_none() && core.my_slot.is_some() && !core.ready {
            core.act(Intent::PickHero(HeroId::SHADOW_FIEND.0));
            core.act(Intent::ToggleReady);
        }
        core.ears().is_some()
    });
    let radiant = usize::from(players[0].core().my_team() != Some(Team::Radiant));
    // In lockstep both players must keep sending acknowledgements during setup.
    for cheat in [Cheat::StartGame, Cheat::AllVision { on: true }] {
        let tick = players[radiant].tick();
        players[radiant].core_mut().act(Intent::Cheat(cheat));
        both(&mut players, "both players saw the setup tick", |_, c| {
            c.tick() > tick + 1
        });
    }
    let tower = |c: &Client| {
        c.view()
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Tower && u.team == Team::Dire)
            .map(|u| u.id)
    };
    both(&mut players, "a Dire tower in sight", |_, c| {
        c.core().my_team() != Some(Team::Radiant) || tower(c).is_some()
    });
    let target = tower(players[radiant]).expect("in sight");
    players[radiant]
        .core_mut()
        .act(Intent::Cheat(Cheat::Destroy {
            target: Target::Unit(target),
        }));
    both(&mut players, "the end shown to both", |_, c| {
        c.world().resource::<Finale>().run.is_some()
    });
    for client in players.iter_mut() {
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    }
    let mut checked = [false; 2];
    both(&mut players, "the result pages", |seat, c| {
        if checked[seat] {
            return true;
        }
        if menu(c).page() != Page::Result {
            assert!(
                c.world().resource::<Finale>().run.is_some() || menu(c).page() == Page::Game,
                "nothing between the end and the result page"
            );
            return false;
        }
        // The first frame drawn on this player's page.
        c.step_frames(1);
        let crate::main_menu::PageView::Result(result) = menu(c).view().body else {
            panic!("player {seat} has a drawn result page");
        };
        let mine = result
            .seats
            .iter()
            .find(|row| row.mine)
            .expect("my result row");
        let expected = if seat == radiant { "(+" } else { "(-" };
        assert!(
            mine.rating.contains(expected) && c.shown_texts().contains(&mine.rating),
            "player {seat} expected a visible {expected} rating change, saw {:?}",
            mine.rating
        );
        checked[seat] = true;
        true
    });
    for client in players.iter_mut() {
        assert_eq!(
            crate::error_msg::shown(client.world_mut()),
            None,
            "no error told"
        );
        act(client, Act::Find(solo()));
    }
    both(&mut players, "both look for their next match", |_, c| {
        let view = menu(c).view();
        view.searches.len() == 1 || view.offer.is_some()
    });
}
