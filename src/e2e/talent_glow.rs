//! The talent tree button lights as Dota's does: lit (`CanLevelStats`) while a talent could be
//! taken, and only with the Attribute Bonus's own glow (`SkillUpgradable`) while a skill point
//! could go into the bonus and no talent could be taken.

use bota_proto::{Cheat, HeroId, TalentSlot};

use super::harness::{Client, Server};
use crate::talents::{TreeShown, tree_shown};

/// One's own hero's talents, as the view tells them.
fn my_talents(client: &Client) -> Option<bota_proto::TalentView> {
    let core = client.core();
    core.talents_of(core.my_slot?)
}

#[test]
fn the_tree_lights_for_a_talent_and_glows_apart_while_only_the_attribute_bonus_could_take_a_point()
{
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Invulnerable { on: true });
    // Level two, two skill points to spend: an ability or the Attribute Bonus could take one,
    // and no talent's tier is open.
    client.cheat(Cheat::Levels { count: 1 });
    client.until("the Attribute Bonus could take a point", |c| {
        my_talents(c).is_some_and(|t| t.may_learn_attribute_bonus == Some(true))
    });
    assert_eq!(
        my_talents(&client).and_then(|t| t.may_learn),
        Some(Default::default())
    );
    client.frames(3);
    assert_eq!(
        tree_shown(client.world_mut()),
        (Some(TreeShown::Attributes), false),
        "no talent to take: the tree shows the Attribute Bonus's glow alone, its rim unlit"
    );
    // Level ten: the first tier is open.
    client.cheat(Cheat::Levels { count: 8 });
    client.until("a talent could be taken", |c| {
        my_talents(c)
            .and_then(|t| t.may_learn)
            .is_some_and(|m| m.has(TalentSlot(0)))
    });
    client.frames(3);
    assert_eq!(
        tree_shown(client.world_mut()),
        (Some(TreeShown::Talent), true),
        "a talent to take lights the tree and its rim"
    );
    client.core_mut().learn_talent(0);
    client.until("the talent taken", |c| {
        my_talents(c).is_some_and(|t| t.learned.has(TalentSlot(0)))
    });
    client.frames(3);
    assert_eq!(
        tree_shown(client.world_mut()),
        (Some(TreeShown::Attributes), false),
        "the tier taken, the skill points left show the Attribute Bonus's glow alone"
    );
}
