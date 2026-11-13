//! Rank uncertainty comes from actual public per-mode profile replies.
use super::{harness::Client,menu::{act,signed_up}};
use crate::main_menu::{Act,MenuCore,Page};
use bevy::prelude::*;

fn widget(client:&mut Client)->Option<Entity> {
    let world=client.world_mut();
    world.query::<(Entity,&Name)>().iter(world).find(|(_,name)|name.as_str()=="rank confidence solo_mid").map(|(entity,_)|entity)
}
fn widget_words(client:&mut Client,entity:Entity)->Vec<String> {
    let world=client.world_mut();
    super::harness::subtree(world,entity).into_iter().filter_map(|entity|world.get::<Text>(entity).map(|text|text.0.clone())).collect()
}
#[test]
fn a_fresh_account_has_an_uncalibrated_zero_confidence_mode() {
    let service=crate::main_menu::tests::service();
    let mut client=signed_up(&service,"confidence_fresh");
    act(&mut client,Act::Go(Page::Profile));
    client.until("fresh account reply",|client|crate::main_menu::tests::own_cached_profile_id(client.world().non_send::<MenuCore>()).is_some());
    let entity=widget(&mut client).expect("the fresh ranked mode has a confidence widget");
    let words=widget_words(&mut client,entity);
    assert!(words.iter().any(|word|word=="Uncalibrated"));
    assert!(words.iter().any(|word|word=="Rank confidence"));
    assert!(words.iter().any(|word|word=="0%"));
    let world=client.world_mut();
    let fill=world.query::<(Entity,&Name)>().iter(world).find(|(_,name)|name.as_str()=="rank confidence fill solo_mid").expect("empty bar").0;
    assert_eq!(world.get::<ComputedNode>(fill).expect("bar layout").size().x,0.0);
    act(&mut client,Act::Go(Page::MatchHistory));
    let history_widget=widget(&mut client).expect("the match-history header shares the confidence widget");
    assert!(widget_words(&mut client,history_widget).iter().any(|word|word=="0%"));
}
#[test]
fn one_real_rated_game_moves_another_players_confidence_by_the_declared_formula() {
    let service=crate::main_menu::tests::service();
    let played=crate::main_menu::tests::served_postmatch(&service);
    let account=played.other;
    let deviation=crate::main_menu::tests::probe_public_deviation(&service,account);
    assert!((290.0..290.6).contains(&deviation),"the actual first-match Glicko deviation: {deviation}");
    let mut client=signed_up(&service,"confidence_viewer");
    client.world_mut().non_send_mut::<MenuCore>().inspect_player(crate::main_menu::tests::profile_target(&service,account)).expect("configured service");
    client.until("public per-mode rating loaded",|client|crate::main_menu::tests::visitor_public_matches(client.world().non_send::<MenuCore>()).is_some_and(|rows|rows.len()==1));
    let entity=widget(&mut client).expect("published deviation drives the other-player widget");
    let words=widget_words(&mut client,entity);
    // First game: RD≈290.32; (350−RD)/(350−60.2751118) rounds to21%.
    assert!(words.iter().any(|word|word=="21%"),"independent first-game percentage: {words:?}");
    let world=client.world_mut();
    let fill=world.query::<(Entity,&Name,&ChildOf)>().iter(world).find(|(_,name,_)|name.as_str()=="rank confidence fill solo_mid").expect("bar fill");
    let width=world.get::<ComputedNode>(fill.0).expect("fill layout").unrounded_size().x;
    let total=world.get::<ComputedNode>(fill.2.parent()).expect("rail layout").unrounded_size().x;
    assert!((width/total-0.21).abs()<0.005,"the actual bar advances to21percent");
    assert!(!words.iter().any(|word|word=="Uncalibrated"),"one played rated game meets the existing leaderboard eligibility rule");
}
#[test]
fn hidden_and_banned_profiles_publish_no_confidence_widget() {
    for banned in [false,true] {
        let service=crate::main_menu::tests::service();
        let played=crate::main_menu::tests::served_postmatch(&service);
        let account=played.other;
        crate::main_menu::tests::withhold_confidence_profile(&service,account,banned);
        let mut client=signed_up(&service,"confidence_privacy");
        client.world_mut().non_send_mut::<MenuCore>().inspect_player(crate::main_menu::tests::profile_target(&service,account)).expect("configured service");
        client.until("unavailable public profile",|client|client.shown_texts().iter().any(|text|text=="This player's profile is unavailable"));
        assert!(widget(&mut client).is_none());
        assert!(!client.shown_texts().iter().any(|text|text=="Rank confidence"));
    }
}
