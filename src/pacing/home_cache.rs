//! Keep the two static layers on either side of the animated menu hero.

use std::sync::atomic::{AtomicBool, Ordering};

use bevy::{
    core_pipeline::{
        Core2d, Core2dSystems, Core3d, Core3dSystems,
        blit::{BlitPipeline, BlitPipelineKey},
    },
    ecs::entity::EntityHashMap,
    prelude::*,
    render::{
        GpuResourceAppExt, Render, RenderApp, RenderSystems,
        camera::ExtractedCamera,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_phase::ViewSortedRenderPhases,
        render_resource::{
            BindGroup, BlendState, CachedRenderPipelineId, Extent3d, LoadOp, Operations,
            PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
            SpecializedRenderPipelines, StoreOp, TextureDescriptor, TextureDimension,
            TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::{ExtractedView, ViewTarget},
    },
    text::{EmSize, RemSize},
    ui::{
        ComputedUiRenderTargetInfo, ContentSize, FixedNode, IgnoreScroll, LayoutConfig, UiSystems,
    },
    ui_render::{TransparentUi, UiCameraView, prepare_uinodes, ui_pass},
};

#[derive(Clone, Default, ExtractResource, PartialEq, Eq, Resource)]
#[extract_app(RenderApp)]
pub(super) struct Request {
    pub(super) hero: Option<Entity>,
    pub(super) refresh: bool,
}

struct Layer {
    view: TextureView,
    bind: BindGroup,
}

#[derive(Resource, Default)]
struct Cache {
    layers: Option<Layers>,
}

struct Layers {
    view: Entity,
    hero: Entity,
    size: UVec2,
    format: TextureFormat,
    pipeline: CachedRenderPipelineId,
    behind: Layer,
    over: Layer,
    valid: AtomicBool,
    composited: AtomicBool,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Request>()
        .add_plugins(ExtractResourcePlugin::<Request>::default())
        .configure_sets(PostUpdate, UiSystems::Layout.run_if(layout_due));
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .init_gpu_resource::<Cache>()
        .add_systems(
            Render,
            (prepare, keep_only_hero)
                .chain()
                .before(prepare_uinodes)
                .in_set(RenderSystems::PrepareBindGroups),
        )
        .add_systems(
            Core2d,
            (cached_ui, skip_stock_pass)
                .chain()
                .after(Core2dSystems::PostProcess)
                .before(ui_pass),
        )
        .add_systems(
            Core3d,
            (cached_ui, skip_stock_pass)
                .chain()
                .after(Core3dSystems::PostProcess)
                .before(ui_pass),
        );
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn layout_due(
    last: Res<Request>,
    rem: Res<RemSize>,
    changes: Query<
        (),
        (
            With<Node>,
            Or<(
                Changed<Children>,
                Changed<ChildOf>,
                Changed<ContentSize>,
                Changed<ComputedUiRenderTargetInfo>,
                Changed<EmSize>,
                Changed<UiTransform>,
                Changed<LayoutConfig>,
                Changed<Outline>,
                Changed<ScrollPosition>,
                Added<FixedNode>,
                Changed<IgnoreScroll>,
            )>,
        ),
    >,
    mut nodes: RemovedComponents<Node>,
    mut children: RemovedComponents<Children>,
    mut parent: RemovedComponents<ChildOf>,
    mut fixed: RemovedComponents<FixedNode>,
    mut ignored_scroll: RemovedComponents<IgnoreScroll>,
    mut layout: RemovedComponents<LayoutConfig>,
    mut outlines: RemovedComponents<Outline>,
    mut scroll: RemovedComponents<ScrollPosition>,
    styles: Query<(Entity, &Node), Changed<Node>>,
    mut previous: Local<EntityHashMap<Node>>,
) -> bool {
    if last.hero.is_none() {
        if !previous.is_empty() {
            previous.clear();
        }
        return true;
    }
    let mut removed_node = false;
    for id in nodes.read() {
        removed_node = true;
        previous.remove(&id);
    }
    let removed = children.read().count()
        + parent.read().count()
        + fixed.read().count()
        + ignored_scroll.read().count()
        + layout.read().count()
        + outlines.read().count()
        + scroll.read().count();
    // A hidden widget may write the same style every update. Actual style changes still
    // affect layout even for invisible nodes that reserve space in a visible container.
    let mut style_changed = false;
    for (entity, style) in &styles {
        if previous.get(&entity) != Some(style) {
            style_changed = true;
            previous.insert(entity, style.clone());
        }
    }
    style_changed || rem.is_changed() || !changes.is_empty() || removed_node || removed != 0
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    request: Res<Request>,
    mut cache: ResMut<Cache>,
    cameras: Query<(Entity, &UiCameraView, &ExtractedCamera, &ViewTarget)>,
    phases: Res<ViewSortedRenderPhases<TransparentUi>>,
    views: Query<&ExtractedView>,
    device: Res<RenderDevice>,
    blit: Res<BlitPipeline>,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<BlitPipeline>>,
) {
    let Some(hero) = request.hero else {
        if cache.layers.is_some() {
            cache.layers = None;
        }
        return;
    };
    let Some((view, camera, target)) = cameras.iter().find_map(|(id, ui, camera, target)| {
        let view = views.get(ui.0).ok()?;
        let phase = phases.get(&view.retained_view_entity)?;
        phase
            .items
            .values()
            .any(|item| item.entity.1.id() == hero)
            .then_some((id, camera, target))
    }) else {
        return;
    };
    let Some(size) = camera
        .physical_target_size
        .filter(|size| size.x > 0 && size.y > 0)
    else {
        return;
    };
    // Do not keep two unbounded backing images for virtual displays or unusual windows.
    if camera.viewport.is_some() || u64::from(size.x) * u64::from(size.y) > 4096 * 2160 {
        cache.layers = None;
        return;
    }
    let format = target.main_texture_format();
    if let Some(layers) = cache.layers.as_ref()
        && (layers.view, layers.hero, layers.size, layers.format) == (view, hero, size, format)
    {
        if request.refresh {
            layers.valid.store(false, Ordering::Relaxed);
        }
        return;
    }
    let make_layer = |label| {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        let bind = blit.create_bind_group(&device, &view, &pipeline_cache);
        Layer { view, bind }
    };
    let pipeline = pipelines.specialize(
        &pipeline_cache,
        &blit,
        BlitPipelineKey {
            target_format: format,
            blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            samples: 1,
            source_space: None,
        },
    );
    cache.layers = Some(Layers {
        view,
        hero,
        size,
        format,
        pipeline,
        behind: make_layer("menu behind hero"),
        over: make_layer("menu over hero"),
        valid: AtomicBool::new(false),
        composited: AtomicBool::new(false),
    });
}

fn keep_only_hero(
    cache: Res<Cache>,
    cameras: Query<&UiCameraView>,
    views: Query<&ExtractedView>,
    mut phases: ResMut<ViewSortedRenderPhases<TransparentUi>>,
    pipeline_cache: Res<PipelineCache>,
) {
    let Some(layers) = cache
        .layers
        .as_ref()
        .filter(|layers| layers.valid.load(Ordering::Relaxed))
    else {
        return;
    };
    if pipeline_cache
        .get_render_pipeline(layers.pipeline)
        .is_none()
    {
        return;
    }
    let Some(key) = cameras
        .get(layers.view)
        .ok()
        .and_then(|ui| views.get(ui.0).ok())
        .map(|view| view.retained_view_entity)
    else {
        return;
    };
    if let Some(phase) = phases.get_mut(&key) {
        phase
            .items
            .retain(|_, item| item.entity.1.id() == layers.hero);
    }
}

#[allow(clippy::too_many_arguments)]
fn cached_ui(
    world: &World,
    view: ViewQuery<(&ViewTarget, &UiCameraView)>,
    cache: Res<Cache>,
    phases: Res<ViewSortedRenderPhases<TransparentUi>>,
    views: Query<&ExtractedView>,
    pipeline_cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let Some(layers) = cache
        .layers
        .as_ref()
        .filter(|layers| layers.view == view.entity())
    else {
        return;
    };
    let (target, ui) = view.into_inner();
    let Ok(ui_view) = views.get(ui.0) else { return };
    let key = ui_view.retained_view_entity;
    let Some(phase) = phases.get(&key) else {
        return;
    };
    let Some(at) = phase
        .items
        .values()
        .position(|item| item.entity.1.id() == layers.hero)
    else {
        return;
    };
    // The stage has its own material, so its quad is a complete batch.
    if phase.items[at].batch_range.len() != 1 {
        return;
    }
    let Some(pipeline) = pipeline_cache.get_render_pipeline(layers.pipeline) else {
        return;
    };
    if !layers.valid.load(Ordering::Relaxed) {
        if phase
            .items
            .values()
            .any(|item| pipeline_cache.get_render_pipeline(item.pipeline).is_none())
        {
            return;
        }
        for (range, layer) in [
            (0..at, &layers.behind),
            (at + 1..phase.items.len(), &layers.over),
        ] {
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("cache static menu layer"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &layer.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Default::default()),
                        store: StoreOp::Store,
                    },
                })],
                ..default()
            });
            if phase.render_range(&mut pass, world, ui.0, range).is_err() {
                return;
            }
        }
        layers.valid.store(true, Ordering::Relaxed);
    }
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("composite menu hero and cached layers"),
        color_attachments: &[Some(target.get_unsampled_color_attachment())],
        ..default()
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &layers.behind.bind, &[]);
    pass.draw(0..3, 0..1);
    if phase
        .render_range(&mut pass, world, ui.0, at..at + 1)
        .is_err()
    {
        return;
    }
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &layers.over.bind, &[]);
    pass.draw(0..3, 0..1);
    layers.composited.store(true, Ordering::Relaxed);
}

fn skip_stock_pass(
    view: ViewQuery<&UiCameraView>,
    cache: Res<Cache>,
    views: Query<&ExtractedView>,
    mut phases: ResMut<ViewSortedRenderPhases<TransparentUi>>,
) {
    let Some(layers) = cache
        .layers
        .as_ref()
        .filter(|layers| layers.view == view.entity())
    else {
        return;
    };
    if !layers.composited.swap(false, Ordering::Relaxed) {
        return;
    }
    if let Ok(view) = views.get(view.into_inner().0)
        && let Some(phase) = phases.get_mut(&view.retained_view_entity)
    {
        phase.items.clear();
    }
}
