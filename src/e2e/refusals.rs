//! Refusals as Dota tells them: an order turned down shows Dota's line from
//! the install on its error plate; what Dota lets go by (an order of a dead hero) says nothing.

use std::time::Duration;

use bota_client_core::{Intent, Refused, Slot};
use bota_proto::{Cheat, HeroId, ItemId, RejectReason, Team};

use super::harness::{Client, Server};

/// Dota's line of an error key, as the install words it.
fn line(client: &Client, key: &str) -> String {
    let texts = client.world().resource::<crate::dota_text::Texts>();
    texts.label(key, &[]).expect("the install's line")
}

/// Whether the error plate shows these words.
fn shows(client: &mut Client, words: &str) -> bool {
    crate::error_msg::shown(client.world_mut()).is_some_and(|t| t.eq_ignore_ascii_case(words))
}

/// An unlearned ability pressed and an item bought without the gold show Dota's own lines,
/// "Ability Not Learned" and "Not Enough Gold" in the install's words; an order of a hero lying
/// dead shows nothing (it showed the simulator's reason).
#[test]
fn a_refusal_shows_dotas_line_and_a_dead_heros_order_none() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let patience = Duration::from_secs(10);
    let not_learned = line(&client, "dota_hud_error_ability_not_learned");
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(0),
        ctrl: false,
    });
    client.within(patience, "Ability Not Learned", |c| shows(c, &not_learned));
    let no_gold = line(&client, "dota_hud_error_not_enough_gold");
    client.within(patience, "the last refusal gone", |c| {
        c.core().reject.is_none()
    });
    client.core_mut().act(Intent::Buy(ItemId::OGRE_AXE.0));
    client.within(patience, "Not Enough Gold", |c| shows(c, &no_gold));
    // Gone before the hero falls: waiting for it after, the match (which runs a tick a frame)
    // goes on to the hero's respawn when frames are quick.
    client.within(patience, "the last refusal gone", |c| {
        c.core().reject.is_none()
    });
    let map = bota_core::map_of(bota_core::MAP2_ID);
    client.cheat(Cheat::Teleport {
        at: bota_core::fountain_pos(map, Team::Dire),
    });
    client.until("the hero fallen", |c| c.core().my_standing_hero().is_none());
    client.core_mut().act(Intent::MoveTo((1000.0, 1000.0)));
    client.within(patience, "the dead hero's order turned down", |c| {
        let reason = c.core().reject.as_ref().map(|(r, _)| r.clone());
        reason == Some(Refused::Rule(RejectReason::HeroDead))
    });
    client.frames(2);
    let told = crate::error_msg::shown(client.world_mut());
    assert_eq!(told, None, "nothing told for a dead hero's order");
}
