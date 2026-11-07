//! Real-match render and UI state checks for fixed readability; these do not compare pixels.
use super::harness::{Client, MID, Server, ring_of};
use bevy::{light::NotShadowReceiver, prelude::*};
use bota_proto::{Cheat, HeroId, Vec2 as Spot};

#[test]
fn full_visual_clarity_render_state_is_enabled_by_default() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    assert_full_clarity(&mut client);
    for preset in crate::config::PRESETS {
        client
            .world_mut()
            .resource_mut::<crate::config::Settings>()
            .set("quality", preset)
            .expect("a supported quality preset");
        client.frames(2);
        assert_contact_disc(&mut client);
    }
}

#[test]
fn retired_visual_clarity_is_ignored_and_absent_from_the_video_menu() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(
        &server,
        HeroId::SHADOW_FIEND,
        &["--set", "visual_clarity=off", "--settings", "Video"],
    );
    client.frames(3);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "Quality preset"),
        "the video options are visible before checking for the retired row"
    );
    assert!(
        !client
            .shown_texts()
            .iter()
            .any(|text| text.contains("Visual clarity")),
        "the video menu has no retired setting"
    );
    assert!(
        !client
            .world()
            .resource::<crate::config::Settings>()
            .render()
            .contains("visual_clarity"),
        "saving settings omits the retired key"
    );
    client
        .world_mut()
        .resource_mut::<crate::settings::Screen>()
        .open = false;
    assert_full_clarity(&mut client);
}

fn assert_full_clarity(client: &mut Client) {
    client.stand_at(MID);
    assert_contact_disc(client);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::CreateHero {
        hero: HeroId::SHADOW_FIEND,
        enemy: true,
        at: Spot::from_ints(MID.0 + 450, MID.1),
    });
    let mine = client.me().team;
    let enemy = client
        .view()
        .units
        .iter()
        .find(|u| u.hero == Some(HeroId::SHADOW_FIEND) && u.team != mine)
        .expect("the enemy hero")
        .clone();
    let color = crate::overlay::team_color(enemy.team)
        .with_alpha(0.5)
        .to_linear();
    client.until("the unselected hero's readability ring", |c| {
        c.line_strips()
            .iter()
            .filter(|(points, tint)| points.len() > 8 && *tint == color)
            .any(|(points, _)| {
                ring_of(points)
                    .0
                    .distance(Vec2::new(enemy.pos.x.to_f32(), enemy.pos.y.to_f32()))
                    < 1.0
            })
    });
}

fn assert_contact_disc(client: &mut Client) {
    let hero = client.me().id;
    client.until("the hero's disc drawn", |c| disc(c, hero).is_some());
    let (alpha, visible) = disc(client, hero).expect("the hero's disc exists");
    assert!(visible, "the contact disc is visible by default");
    assert!(
        (alpha - 0.6).abs() < 1e-6,
        "the contact disc has full opacity: {alpha}"
    );
}

fn disc(client: &mut Client, id: bota_proto::EntityId) -> Option<(f32, bool)> {
    let body = client.body(id)?;
    let world = client.world_mut();
    let (material, visible) = world
        .query_filtered::<(
            &ChildOf,
            &MeshMaterial3d<StandardMaterial>,
            &InheritedVisibility,
        ), With<NotShadowReceiver>>()
        .iter(world)
        .find(|(parent, _, _)| parent.parent() == body)
        .map(|(_, material, visible)| (material.0.clone(), visible.get()))?;
    Some((
        world
            .resource::<Assets<StandardMaterial>>()
            .get(&material)?
            .base_color
            .alpha(),
        visible,
    ))
}
