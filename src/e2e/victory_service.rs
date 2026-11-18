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

/// A service whose practice matches cheat, as its Demo Hero's do, so that the test fells the
/// Ancient instead of playing the match.
fn service_with_cheating_practice() -> crate::main_menu::tests::Service {
    crate::main_menu::tests::start_service(true, |config| {
        let practice = config
            .find("id = \"practice\"")
            .expect("the practice ruleset");
        let args = practice
            + config[practice..]
                .find("args = [\"--map\", \"0\",")
                .expect("its arguments");
        let mut changed = config.clone();
        changed.insert_str(args + "args = [".len(), "\"--cheats\", ");
        changed
    })
}

fn menu(client: &Client) -> &MenuCore {
    client.world().non_send::<MenuCore>()
}

#[test]
fn a_service_match_ends_on_its_result_page_by_itself_with_the_game_left_and_the_account_free() {
    let service = service_with_cheating_practice();
    let mut client = signed_up(&service, "alice");
    act(&mut client, Act::Go(Page::Lobbies));
    act(&mut client, Act::LobbyUi(LobbyAction::Create));
    client.until("the lobby made", |c| {
        menu(c).page() == Page::Lobby && menu(c).view().lobby_ui.editor.is_none()
    });
    act(&mut client, Act::LobbyUi(LobbyAction::Edit));
    act(
        &mut client,
        Act::LobbyUi(LobbyAction::EditMode(bota_radovid::proto::RulesetId(
            "practice".to_owned(),
        ))),
    );
    act(&mut client, Act::LobbyUi(LobbyAction::Save));
    client.until("the practice lobby saved", |c| {
        menu(c).view().lobby_ui.editor.is_none()
    });
    act(&mut client, Act::ReadyLobby(true));
    client.until("the hero on the map", |c| {
        let core = c.core_mut();
        if core.view().is_none() && core.my_slot.is_some() && !core.ready {
            core.act(Intent::PickHero(HeroId::SHADOW_FIEND.0));
            core.act(Intent::ToggleReady);
        }
        core.ears().is_some()
    });
    client.cheat(Cheat::StartGame);
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
