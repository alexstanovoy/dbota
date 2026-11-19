//! Reports requested cache images once the match UI prepares textured draw batches for them.

use std::sync::{Arc, Mutex};

use bevy::{
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems, render_asset::RenderAssets,
        texture::GpuImage,
    },
    ui_render::UiBatch,
};

#[derive(Resource, Clone)]
pub(crate) struct Trace(Arc<Mutex<Status>>);

struct Status {
    paths: Vec<String>,
    drawn: bool,
}

impl Trace {
    #[cfg(test)]
    pub(crate) fn drawn(&self) -> bool {
        self.0.lock().expect("UI art trace").drawn
    }
}

#[derive(Resource, Default)]
struct Images(Vec<AssetId<Image>>);

pub(super) fn install(app: &mut App, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let trace = Trace(Arc::new(Mutex::new(Status {
        paths,
        drawn: false,
    })));
    app.insert_resource(trace.clone());
    app.add_plugins(RenderTrace(trace));
}

struct RenderTrace(Trace);

impl Plugin for RenderTrace {
    fn build(&self, _: &mut App) {}

    fn finish(&self, app: &mut App) {
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .insert_resource(self.0.clone())
                .init_resource::<Images>()
                .add_systems(ExtractSchedule, extract)
                .add_systems(
                    Render,
                    report
                        .after(RenderSystems::Render)
                        .before(RenderSystems::Cleanup),
                );
        }
    }
}

fn extract(
    trace: Res<Trace>,
    mut images: ResMut<Images>,
    hud: Extract<Res<crate::ui::HudShown>>,
    server: Extract<Res<AssetServer>>,
) {
    images.0.clear();
    let status = trace.0.lock().expect("UI art trace");
    if !hud.0 || status.drawn {
        return;
    }
    for path in &status.paths {
        let Some(handle) = server.get_handle::<Image>(crate::manifest::cache_path(path)) else {
            continue;
        };
        if server.is_loaded_with_dependencies(handle.id()) {
            images.0.push(handle.id());
        }
    }
}

fn report(
    trace: Res<Trace>,
    images: Res<Images>,
    prepared: Res<RenderAssets<GpuImage>>,
    batches: Query<&UiBatch>,
) {
    if images.0.is_empty() {
        return;
    }
    let mut status = trace.0.lock().expect("UI art trace");
    if !status.drawn
        && images.0.len() == status.paths.len()
        && images.0.iter().all(|id| {
            prepared.get(*id).is_some()
                && batches
                    .iter()
                    .any(|batch| batch.image == *id && !batch.range.is_empty())
        })
    {
        status.drawn = true;
        info!("ui-art: drew {}", status.paths.join(", "));
    }
}
