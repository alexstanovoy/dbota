//! A picked HUD press may outlive the match state that made its button available.

use std::time::Duration;

use bevy::{
    picking::{
        backend::HitData,
        events::{Pointer, PointerPress, PointerRelease},
        pointer::{Location, PointerButton, PointerId},
    },
    prelude::*,
    ui_widgets::Button,
    window::PrimaryWindow,
};
use bota_client_core::Intent;
use bota_proto::HeroId;

use super::harness::{Client, Server};

/// Dispatches the native event of a press already picked on this real widget.
fn picked_press(client: &mut Client, entity: Entity) {
    let position = client.centre_of(entity).expect("the drawn button");
    let world = client.world_mut();
    let camera = world
        .get::<ComputedUiTargetCamera>(entity)
        .and_then(|c| c.get())
        .expect("the UI camera");
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .expect("the primary window");
    let target = bevy::window::WindowRef::Entity(window)
        .normalize(None)
        .expect("the window target");
    let press = PointerPress {
        entity,
        pointer: Pointer::new(
            PointerId::Mouse,
            Location {
                target: bevy::camera::NormalizedRenderTarget::Window(target),
                position,
            },
        ),
        button: PointerButton::Primary,
        hit: HitData::new(camera, 0.0, None, None),
        count: 1,
    };
    world.trigger(press.clone());
    world.trigger(PointerRelease {
        entity,
        pointer: press.pointer,
        button: press.button,
        hit: press.hit,
    });
    world.flush();
}

#[test]
fn a_picked_talent_press_waits_for_a_known_match_or_the_tactical_phase() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let world = client.world_mut();
    let button = world
        .query_filtered::<Entity, With<crate::talents::TalentButton>>()
        .single(world)
        .expect("the talent button");
    client.hover_ui(button, Duration::from_secs(30));
    client
        .world_mut()
        .resource_mut::<crate::input::Panels>()
        .talents = false;
    client
        .world_mut()
        .resource_mut::<crate::session::MatchKnown>()
        .0 = false;
    client
        .world_mut()
        .resource_mut::<crate::select::TacticalPhase>()
        .0 = false;
    picked_press(&mut client, button);
    assert!(
        !client.world().resource::<crate::input::Panels>().talents,
        "a stale press cannot pin talents outside a match"
    );

    client
        .world_mut()
        .resource_mut::<crate::session::MatchKnown>()
        .0 = true;
    picked_press(&mut client, button);
    assert!(
        client.world().resource::<crate::input::Panels>().talents,
        "the same real button works in a known match"
    );
    client
        .world_mut()
        .resource_mut::<crate::input::Panels>()
        .talents = false;
    client
        .world_mut()
        .resource_mut::<crate::session::MatchKnown>()
        .0 = false;
    client
        .world_mut()
        .resource_mut::<crate::select::TacticalPhase>()
        .0 = true;
    picked_press(&mut client, button);
    assert!(
        client.world().resource::<crate::input::Panels>().talents,
        "the tactical phase also permits the talent button"
    );
}

fn courier_button(client: &mut Client) -> Entity {
    let world = client.world_mut();
    let path = world
        .resource::<crate::manifest::Manifest>()
        .file("hud.courier")
        .expect("the courier icon");
    let assets = world.resource::<AssetServer>().clone();
    let mut entity = world
        .query::<(Entity, &ImageNode, &InheritedVisibility)>()
        .iter(world)
        .find(|(_, image, visible)| {
            visible.get()
                && assets
                    .get_path(image.image.id())
                    .is_some_and(|p| p.to_string() == path)
        })
        .map(|(entity, ..)| entity)
        .expect("the drawn courier icon");
    for _ in 0..32 {
        if world.get::<Button>(entity).is_some() {
            return entity;
        }
        entity = world
            .get::<ChildOf>(entity)
            .expect("the icon belongs to a button")
            .parent();
    }
    panic!("the courier button is within the HUD hierarchy");
}

#[test]
fn a_picked_courier_press_does_nothing_until_the_match_is_known() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.core_mut().act(Intent::SelectHero);
    let courier = client.core().my_courier().expect("the player's courier");
    let button = courier_button(&mut client);
    client.hover_ui(button, Duration::from_secs(30));
    let before = client.core().selected;
    assert_ne!(before, Some(courier));
    client
        .world_mut()
        .resource_mut::<crate::session::MatchKnown>()
        .0 = false;
    picked_press(&mut client, button);
    assert_eq!(
        client.core().selected,
        before,
        "the queued press cannot change selection after the match becomes unknown"
    );

    client
        .world_mut()
        .resource_mut::<crate::session::MatchKnown>()
        .0 = true;
    picked_press(&mut client, button);
    assert_eq!(
        client.core().selected,
        Some(courier),
        "the same real courier button selects it during the match"
    );
}

#[test]
fn the_tactical_phase_is_current_in_the_frame_that_picking_observers_run() {
    #[derive(Resource, Default)]
    struct Seen(Option<(bool, bool)>);

    fn observe(
        session: NonSend<crate::session::Session>,
        phase: Res<crate::select::TacticalPhase>,
        mut seen: ResMut<Seen>,
    ) {
        seen.0 = Some((session.0.in_tactical_phase(), phase.0));
    }

    let server = Server::start(1, &["--tactical-secs", "1000"]);
    let mut client = Client::pick(&server, HeroId::SHADOW_FIEND, &[]);
    assert!(client.core().in_tactical_phase());
    client.app.init_resource::<Seen>().add_systems(
        PreUpdate,
        observe.in_set(bevy::picking::PickingSystems::Hover),
    );
    // The phase cached for the last frame was false when the server entered strategy.
    client
        .world_mut()
        .resource_mut::<crate::select::TacticalPhase>()
        .0 = false;
    client.frames(1);
    assert_eq!(
        client.world().resource::<Seen>().0,
        Some((true, true)),
        "picking sees the phase pumped for this frame"
    );
}
