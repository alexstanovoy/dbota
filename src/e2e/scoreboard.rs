//! The scoreboard's rows as Dota draws them: each row's hero picture is Dota's own for the
//! scoreboard, `dota_hud_scoreboard_player.vxml`'s `DOTAHeroImage` (no `heroimagestyle`: the
//! landscape picture, `panorama/images/heroes/npc_dota_hero_<name>_png`), in an 88 x 54 frame it
//! covers.

use std::time::Duration;

use bota_proto::{HeroId, Team};

use super::harness::{Client, Server, quiet_seat};

const SOON: Duration = Duration::from_secs(30);

/// The landscape picture of every seat's hero, by side and place on it, in seat order.
fn wanted(client: &Client) -> Vec<(Team, u8, Option<String>)> {
    let manifest = client.world().resource::<crate::manifest::Manifest>();
    let view = client.view();
    let mut out = Vec::new();
    for team in [Team::Radiant, Team::Dire] {
        let seats = view.players.iter().filter(|p| p.team == team);
        for (rank, p) in seats.enumerate() {
            let rank = u8::try_from(rank).expect("a side's seats fit a byte");
            out.push((team, rank, manifest.hero_icon(p.hero.0)));
        }
    }
    out
}

#[test]
fn every_scoreboard_row_shows_its_heros_landscape_picture() {
    let server = Server::start(4, &[]);
    for hero in [HeroId::PUDGE, HeroId::SHADOW_FIEND, HeroId::PUDGE] {
        quiet_seat(&server, hero);
    }
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.within(SOON, "four seats in the match", |c| {
        c.view().players.len() == 4
    });
    let want = wanted(&client);
    assert!(
        want.iter().all(|(.., path)| path.is_some()),
        "every hero has a landscape picture in the cache: {want:?}"
    );
    let heroes: std::collections::HashSet<_> = want.iter().map(|(.., p)| p.clone()).collect();
    assert_eq!(heroes.len(), 2, "both heroes' pictures differ: {want:?}");
    client
        .world_mut()
        .resource_mut::<crate::input::Panels>()
        .scoreboard = true;
    client.within(SOON, "every row's hero picture drawn", |c| {
        crate::scoreboard::drawn_heroes(c.world_mut()).len() == 4
    });
    client.frames(3);
    assert_eq!(crate::scoreboard::drawn_heroes(client.world_mut()), want);
}
