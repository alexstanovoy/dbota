//! A match's steady frames create no texture: through the production renderer (offscreen), with
//! one's hero in the bottom panel's portrait and frames coming 240 times a second, two seconds of
//! frames create no texture and few buffers. The portrait's
//! textures used to be freed between its frames and made anew on each, which held those frames
//! up to 150 ms.

use std::{sync::atomic::Ordering, time::Duration};

use bevy::{
    camera::{ImageRenderTarget, RenderTarget},
    prelude::*,
    render::render_resource::{TextureFormat, TextureUsages},
    time::TimeUpdateStrategy,
};
use bota_client_core::Intent;
use bota_proto::HeroId;

use super::harness::{Client, Server};
use crate::kept_targets::{BUFFERS_CREATED, TEXTURES_CREATED};

/// Frames of a 240 Hz display: two seconds of them, as many again to settle first.
const FRAMES: usize = 480;
/// Buffers the renderer creates per frame today: its light clusters' storage and the gizmos'
/// lines are made anew every frame. More is a new leak of a buffer per frame.
const BUFFERS_PER_FRAME: u32 = 9;

fn prepare(app: &mut App) {
    app.add_systems(PostStartup, surface);
}

fn surface(
    mut images: ResMut<Assets<Image>>,
    mut camera: Single<&mut RenderTarget, With<crate::camera::RtsCamera>>,
) {
    let mut image = Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    **camera = RenderTarget::Image(ImageRenderTarget {
        handle: images.add(image),
        scale_factor: 1.0,
    });
}

fn created() -> (u32, u32) {
    (
        TEXTURES_CREATED.load(Ordering::Relaxed),
        BUFFERS_CREATED.load(Ordering::Relaxed),
    )
}

#[test]
fn the_steady_frames_of_a_match_with_an_animated_portrait_create_no_texture() {
    let server = Server::start(1, &[]);
    let mut client = Client::run_rendered(
        &["--addr", &server.address, "--mute", "--window", "1280x720"],
        prepare,
    );
    client.until("the hero picked", |c| {
        let core = c.core_mut();
        if core.view().is_none() && core.my_slot.is_some() && !core.ready {
            core.act(Intent::PickHero(HeroId::SHADOW_FIEND.0));
            core.act(Intent::ToggleReady);
        }
        core.ears().is_some()
    });
    client.until("the loading over", |c| {
        c.world().resource::<crate::loading::Loading>().done()
    });
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 240.0,
        )));
    client.core_mut().act(Intent::SelectHero);
    client.until("one's hero drawn in the portrait", |c| {
        let stage = c.world().resource::<crate::portrait::Stage>();
        stage.actor.as_ref().is_some_and(|a| a.posed()) && stage.drawing()
    });
    client.step_frames(FRAMES);
    let (textures, buffers) = created();
    let mut drawn = 0;
    for _ in 0..FRAMES {
        client.app.update();
        drawn += usize::from(
            client
                .world()
                .resource::<crate::portrait::Stage>()
                .drawing(),
        );
    }
    let (textures, buffers) = (created().0 - textures, created().1 - buffers);
    eprintln!(
        "{FRAMES} frames, the portrait drawn on {drawn}: {textures} textures, {buffers} buffers"
    );
    assert!(
        drawn >= FRAMES / 16,
        "the portrait drawn on {drawn} of {FRAMES} frames"
    );
    assert_eq!(textures, 0, "textures created in {FRAMES} steady frames");
    assert!(
        buffers <= BUFFERS_PER_FRAME * FRAMES as u32,
        "{buffers} buffers created in {FRAMES} steady frames"
    );
}
