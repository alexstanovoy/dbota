//! Drawing the particles: every card of a frame is one instance of a quad in a single vertex
//! buffer written once a frame, drawn in one call per look (a texture and its blend) with that
//! look's pipeline and bind group, both made once. No material, no mesh, no per-card entity.

use std::ops::Range;

use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d, TransparentSortingInfo3d},
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::SRes},
    },
    mesh::VertexBufferLayout,
    pbr::{MeshPipeline, MeshPipelineKey, MeshPipelineSystems, SetMeshViewBindGroup, ViewKeyCache},
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
        render_asset::RenderAssets,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        texture::GpuImage,
        view::ExtractedView,
    },
};

use super::draw::Quad;
use super::library::{Library, MAX_LOOKS};

/// A frame's cards, grouped by look, as the main world made them.
#[derive(Resource, Default)]
pub struct ParticleFrame {
    pub quads: Vec<Quad>,
    pub batches: Vec<Batch>,
}

/// One look's cards: their range in the frame's buffer and their centre, by which the batch
/// is sorted among the other transparent things.
#[derive(Clone, Debug)]
pub struct Batch {
    pub look: u16,
    pub range: Range<u32>,
    pub center: Vec3,
}

/// The looks in the render world: each texture's image and blend.
#[derive(Resource, Default)]
struct Looks {
    /// Each look's first and second layer's images, whether it modulates and depth tests.
    list: Vec<(AssetId<Image>, AssetId<Image>, bool, bool)>,
    /// A look's bind group, with the texture views it was made of (made again when an image
    /// changes).
    bind_groups: Vec<Option<([TextureViewId; 2], BindGroup)>>,
}

/// The render world's copy of the frame and the buffer it is written to.
#[derive(Resource)]
struct Buffers {
    instances: RawBufferVec<Quad>,
    batches: Vec<Batch>,
}

/// One render entity per batch slot, so a phase item can name its batch.
#[derive(Resource)]
struct Slots(Vec<Entity>);

#[derive(Component)]
struct Slot(usize);

#[derive(Resource)]
struct ParticlePipeline {
    variants: Variants<RenderPipeline, Specializer>,
    texture_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
}

struct Specializer {
    mesh_pipeline: MeshPipeline,
}

#[derive(Clone, PartialEq, Eq, Hash, SpecializerKey)]
struct Key {
    view: MeshPipelineKey,
    /// `MOD2X`: twice the colour times what is behind.
    modulate: bool,
    depth_test: bool,
}

pub fn plugin(app: &mut App) {
    embedded_asset!(app, "particles.wgsl");
    app.init_resource::<ParticleFrame>();
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .init_resource::<Looks>()
        .add_render_command::<Transparent3d, DrawParticles>()
        .add_systems(RenderStartup, init.after(MeshPipelineSystems))
        .add_systems(ExtractSchedule, extract)
        .add_systems(
            Render,
            (
                prepare.in_set(RenderSystems::PrepareResources),
                prepare_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                queue.in_set(RenderSystems::Queue),
            ),
        );
}

fn init(
    mut commands: Commands,
    mesh_pipeline: Res<MeshPipeline>,
    server: Res<AssetServer>,
    device: Res<RenderDevice>,
) {
    let shader = load_embedded_asset!(server.as_ref(), "particles.wgsl");
    let texture_layout = BindGroupLayoutDescriptor::new(
        "particle look",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                binding_types::texture_2d(TextureSampleType::Float { filterable: true }),
                binding_types::sampler(SamplerBindingType::Filtering),
                // The second texture layer's (the first's again for a look of one).
                binding_types::texture_2d(TextureSampleType::Float { filterable: true }),
            ),
        ),
    );
    let descriptor = RenderPipelineDescriptor {
        label: Some("particles".into()),
        vertex: VertexState {
            shader: shader.clone(),
            buffers: vec![instance_layout()],
            ..default()
        },
        fragment: Some(FragmentState {
            shader,
            ..default()
        }),
        layout: vec![Default::default(), texture_layout.clone()],
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..default()
        },
        ..default()
    };
    let sampler = device.create_sampler(&SamplerDescriptor {
        label: Some("particle textures"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_filter: MipmapFilterMode::Linear,
        ..default()
    });
    commands.insert_resource(ParticlePipeline {
        variants: Variants::new(
            Specializer {
                mesh_pipeline: mesh_pipeline.clone(),
            },
            descriptor,
        ),
        texture_layout,
        sampler,
    });
    commands.insert_resource(Buffers {
        instances: RawBufferVec::new(BufferUsages::VERTEX),
        batches: Vec::new(),
    });
    let slots = (0..MAX_LOOKS)
        .map(|i| commands.spawn(Slot(i)).id())
        .collect();
    commands.insert_resource(Slots(slots));
}

/// The quad's attributes, one set per instance: the shader makes the corners.
fn instance_layout() -> VertexBufferLayout {
    let formats = [
        VertexFormat::Float32x3,
        VertexFormat::Float32x3,
        VertexFormat::Float32x3,
        VertexFormat::Float32x3,
        VertexFormat::Float32x4,
        VertexFormat::Float32x4,
        VertexFormat::Float32x4,
        VertexFormat::Float32x4,
        VertexFormat::Float32x4,
        VertexFormat::Float32x4,
        VertexFormat::Float32x4,
        VertexFormat::Uint32x4,
    ];
    let mut offset = 0;
    let attributes = formats
        .iter()
        .enumerate()
        .map(|(location, format)| {
            let attribute = VertexAttribute {
                format: *format,
                offset,
                shader_location: location as u32,
            };
            offset += format.size();
            attribute
        })
        .collect();
    VertexBufferLayout {
        array_stride: size_of::<Quad>() as u64,
        step_mode: VertexStepMode::Instance,
        attributes,
    }
}

impl bevy::render::render_resource::Specializer<RenderPipeline> for Specializer {
    type Key = Key;

    fn specialize(
        &self,
        key: Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Key>, BevyError> {
        let view_layout = self.mesh_pipeline.get_view_layout(key.view.into());
        descriptor.set_layout(0, view_layout.main_layout.clone());
        descriptor.multisample.count = key.view.msaa_samples();
        descriptor.depth_stencil = Some(DepthStencilState {
            format: CORE_3D_DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(if key.depth_test {
                CompareFunction::GreaterEqual
            } else {
                CompareFunction::Always
            }),
            stencil: StencilState::default(),
            bias: DepthBiasState::default(),
        });
        let blend = if key.modulate {
            let component = BlendComponent {
                src_factor: BlendFactor::Dst,
                dst_factor: BlendFactor::Src,
                operation: BlendOperation::Add,
            };
            BlendState {
                color: component,
                alpha: component,
            }
        } else {
            BlendState::PREMULTIPLIED_ALPHA_BLENDING
        };
        let fragment = descriptor.fragment_mut()?;
        if key.modulate {
            fragment.shader_defs.push("MODULATE".into());
        }
        fragment.set_target(
            0,
            ColorTargetState {
                format: key.view.target_format(),
                blend: Some(blend),
                write_mask: ColorWrites::ALL,
            },
        );
        Ok(key)
    }
}

/// Copies the frame and, when they change, the looks into the render world, keeping the
/// buffers' capacity.
fn extract(
    mut commands: Commands,
    frame: Extract<Res<ParticleFrame>>,
    library: Extract<Option<Res<Library>>>,
    mut looks: ResMut<Looks>,
    buffers: Option<ResMut<Buffers>>,
) {
    if let Some(library) = library.as_ref()
        && library.looks.len() != looks.list.len()
    {
        looks.list = library
            .looks
            .iter()
            .map(|l| {
                let second = l.second.as_ref().unwrap_or(&l.image).id();
                (l.image.id(), second, l.modulate, l.depth_test)
            })
            .collect();
        let len = looks.list.len();
        looks.bind_groups.resize(len, None);
    }
    let Some(mut buffers) = buffers else {
        return;
    };
    let buffers = &mut *buffers;
    buffers.instances.clear();
    for quad in &frame.quads {
        buffers.instances.push(*quad);
    }
    buffers.batches.clear();
    buffers.batches.extend_from_slice(&frame.batches);
    let _ = &mut commands;
}

fn prepare(mut buffers: ResMut<Buffers>, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    if !buffers.instances.is_empty() {
        buffers.instances.write_buffer(&device, &queue);
    }
}

fn prepare_bind_groups(
    mut looks: ResMut<Looks>,
    images: Res<RenderAssets<GpuImage>>,
    pipeline: Res<ParticlePipeline>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
) {
    let looks = &mut *looks;
    for (index, (image, second, _, _)) in looks.list.iter().enumerate() {
        let (Some(gpu), Some(layer)) = (images.get(*image), images.get(*second)) else {
            continue;
        };
        let views = [gpu.texture_view.id(), layer.texture_view.id()];
        if looks.bind_groups[index]
            .as_ref()
            .is_some_and(|(made, _)| *made == views)
        {
            continue;
        }
        let bind_group = device.create_bind_group(
            "particle look",
            &cache.get_bind_group_layout(&pipeline.texture_layout),
            &BindGroupEntries::sequential((
                &gpu.texture_view,
                &pipeline.sampler,
                &layer.texture_view,
            )),
        );
        looks.bind_groups[index] = Some((views, bind_group));
    }
}

#[allow(clippy::too_many_arguments)]
fn queue(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    mut pipeline: ResMut<ParticlePipeline>,
    cache: Res<PipelineCache>,
    buffers: Res<Buffers>,
    looks: Res<Looks>,
    slots: Res<Slots>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    views: Query<&ExtractedView>,
    view_keys: Res<ViewKeyCache>,
) -> Result<(), BevyError> {
    let draw = draw_functions.read().id::<DrawParticles>();
    for view in &views {
        let (Some(phase), Some(&view_key)) = (
            phases.get_mut(&view.retained_view_entity),
            view_keys.get(&view.retained_view_entity),
        ) else {
            continue;
        };
        // Every variant is asked for every frame, drawn or not: their pipelines compile while
        // the game loads (and again when the view's settings change), not at the first missile.
        for (modulate, depth_test) in [(false, true), (false, false), (true, true), (true, false)] {
            pipeline.variants.specialize(
                &cache,
                Key {
                    view: view_key,
                    modulate,
                    depth_test,
                },
            )?;
        }
        for (slot, batch) in buffers.batches.iter().enumerate() {
            let look = usize::from(batch.look);
            let Some(&(_, _, modulate, depth_test)) = looks.list.get(look) else {
                continue;
            };
            if looks.bind_groups.get(look).is_none_or(Option::is_none) || slot >= slots.0.len() {
                continue;
            }
            let id = pipeline.variants.specialize(
                &cache,
                Key {
                    view: view_key,
                    modulate,
                    depth_test,
                },
            )?;
            phase.add_transient(Transparent3d {
                sorting_info: TransparentSortingInfo3d::Sorted {
                    mesh_center: batch.center,
                    depth_bias: 0.0,
                },
                entity: (slots.0[slot], MainEntity::from(Entity::PLACEHOLDER)),
                draw_function: draw,
                pipeline: id,
                distance: 0.0,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            });
        }
    }
    Ok(())
}

type DrawParticles = (SetItemPipeline, SetMeshViewBindGroup<0>, DrawBatch);

struct DrawBatch;

impl<P: PhaseItem> RenderCommand<P> for DrawBatch {
    type Param = (SRes<Buffers>, SRes<Looks>);
    type ViewQuery = ();
    type ItemQuery = &'static Slot;

    fn render<'w>(
        _: &P,
        _: ROQueryItem<'w, '_, Self::ViewQuery>,
        slot: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (buffers, looks): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (buffers, looks) = (buffers.into_inner(), looks.into_inner());
        let Some(batch) = slot.and_then(|s| buffers.batches.get(s.0)) else {
            return RenderCommandResult::Skip;
        };
        let (Some(Some((_, bind_group))), Some(buffer)) = (
            looks.bind_groups.get(usize::from(batch.look)),
            buffers.instances.buffer(),
        ) else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(1, bind_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..6, batch.range.clone());
        RenderCommandResult::Success
    }
}
