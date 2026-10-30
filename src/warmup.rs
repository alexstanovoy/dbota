//! Pipelines compiled while the game loads, not in the frame a thing first
//! appears. A render pipeline is compiled on the render thread the first
//! frame a mesh of its kind is drawn (Bevy compiles synchronously here, see
//! `main.rs`, and always does in a browser), which held that frame for up to a
//! tenth of a second: the first missile of a match, the first hover ring.
//! Each module that draws art later in a match makes one piece of each kind
//! at the start, marked [`Warmup`]: far below the map, seen by no view the
//! player sees (without frustum culling, so its pipelines are specialised; the
//! GPU culls it), shown while the game loads and again for a few frames
//! whenever the settings change (anti-aliasing and shadows specialise
//! pipelines anew), and hidden the rest of the time.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bevy::{
    camera::visibility::NoFrustumCulling,
    prelude::*,
    render::{Render, RenderApp, RenderSystems, render_resource::PipelineCache},
};

/// Where warm-up pieces stand: far below the map, out of every view.
const SPOT: Vec3 = Vec3::new(0.0, -100_000.0, 0.0);

/// Frames the pieces are shown at least: the pipelines of a new mesh are
/// queued the frame after it is first extracted, and settings applied in the
/// first frames (anti-aliasing) specialise them again.
const FRAMES: u8 = 8;

/// A piece of art shown while loading so its pipelines are compiled then.
#[derive(Component, Default)]
#[require(Transform = Transform::from_translation(SPOT), Visibility, NoFrustumCulling)]
pub struct Warmup;

/// Whether the render world had no pipeline still waiting to compile last
/// frame.
#[derive(Resource, Clone, Default)]
pub struct PipelinesReady(Arc<AtomicBool>);

impl PipelinesReady {
    pub fn get(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Frames since the warm-up last began (at the start, at a new piece, at a
/// change of the settings); it is over at [`FRAMES`] with no pipeline
/// waiting.
#[derive(Resource, Default)]
struct Warming {
    frames: u8,
    over: bool,
}

pub fn plugin(app: &mut App) {
    let ready = PipelinesReady::default();
    app.insert_resource(ready.clone())
        .init_resource::<Warming>()
        .add_systems(Last, (warm, gizmos).chain());
    app.add_systems(PostUpdate, report.in_set(crate::loading::Report));
    match app.get_sub_app_mut(RenderApp) {
        Some(render) => {
            render
                .insert_resource(ready)
                .add_systems(Render, pipelines_ready.in_set(RenderSystems::Cleanup));
        }
        // Nothing compiles without a renderer (the headless tests).
        None => ready.0.store(true, Ordering::Relaxed),
    }
}

fn pipelines_ready(cache: Res<PipelineCache>, ready: Res<PipelinesReady>) {
    ready.0.store(
        cache.waiting_pipelines().next().is_none(),
        Ordering::Relaxed,
    );
}

/// Shows the pieces while the warm-up lasts and hides them after.
fn warm(
    ready: Res<PipelinesReady>,
    settings: Res<crate::config::Settings>,
    added: Query<(), Added<Warmup>>,
    mut pieces: Query<&mut Visibility, With<Warmup>>,
    mut warming: ResMut<Warming>,
) {
    if settings.is_changed() || !added.is_empty() {
        *warming = Warming::default();
    }
    if warming.over {
        return;
    }
    warming.frames = warming.frames.saturating_add(1);
    warming.over = warming.frames >= FRAMES && ready.get();
    let shown = if warming.over {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut visibility in &mut pieces {
        visibility.set_if_neq(shown);
    }
}

/// The lines the hover rings and the range overlay draw, one of each kind,
/// while the warm-up lasts.
fn gizmos(warming: Res<Warming>, mut gizmos: Gizmos) {
    if warming.over {
        return;
    }
    gizmos.line(SPOT, SPOT + Vec3::X, Color::WHITE);
    gizmos.linestrip([SPOT, SPOT + Vec3::X, SPOT + Vec3::Z], Color::WHITE);
    gizmos.circle(
        Isometry3d::new(SPOT, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        1.0,
        Color::WHITE,
    );
}

/// The pipelines' part of the loading: compiled, the warm-up over.
fn report(warming: Res<Warming>, mut parts: ResMut<crate::loading::Parts>) {
    parts.set("pipelines", usize::from(warming.over), 1);
}
