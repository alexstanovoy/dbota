//! Home uses the game's hero, wearables and idle clip, and releases them when play resumes.

use bevy::prelude::*;
use bota_proto::HeroId;
use crate::main_menu::hero_stage::{HeroStage, LastPlayedHero};
use super::harness::Client;

#[test]
fn home_shows_the_last_played_model_with_its_idle_and_wearables_and_releases_it_in_game() {
    let mut client = Client::run(&["--menu-preview", "home", "--window", "1440x900"]);
    client.frames(10);
    assert!(client.world().resource::<HeroStage>().actor.is_none(), "fresh profile has no actor");
    for (hero, logical) in [(HeroId::SHADOW_FIEND, "hero.nevermore.model"), (HeroId::PUDGE, "hero.pudge.model")] {
        client.world_mut().resource_mut::<LastPlayedHero>().0 = Some(hero);
        client.until("the last played hero on Home", |c| c.world().resource::<HeroStage>().actor.as_ref().is_some_and(|a| a.posed));
        let world = client.world_mut();
        let actor = world.resource::<HeroStage>().actor.as_ref().expect("posed actor");
        let pivot = actor.pivot;
        let model = actor.model;
        let bank = world.resource::<crate::units::ModelBank>();
        assert_eq!(bank.0[model].model.logical, logical);
        let wearables = bank.0[model].scenes.as_ref().expect("scenes").wearables.len();
        let idle = bank.0[model].clips.iter().find(|c| c.0 == "idle").expect("idle").1;
        let parts = super::harness::subtree(world, pivot);
        assert_eq!(parts.iter().filter(|e| world.get::<crate::units::Wearable>(**e).is_some_and(|w| w.bound())).count(), wearables);
        assert!(parts.iter().any(|e| world.get::<AnimationPlayer>(*e).is_some_and(|p| p.is_playing_animation(idle))));
    }
    client.world_mut().remove_resource::<crate::main_menu::Preview>();
    client.frames(5);
    assert!(client.world().resource::<HeroStage>().actor.is_none(), "leaving Home removes the stage");
}
