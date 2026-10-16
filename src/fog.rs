//! The fog of war of the side watched through: which cells it sees, and the
//! ground, trees and buildings everywhere else dulled and darkened, easing
//! in and out.

use std::collections::HashMap;

use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    image::{ImageSampler, ImageSamplerDescriptor},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat},
    shader::ShaderRef,
};
use std::sync::Arc;

use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future::poll_once};
use bota_client_core::SightGround;
use bota_proto::Team;

use crate::session::Session;
use crate::terrain::CELL;

/// The image's alpha of a fully fogged cell: what the minimap lays over its
/// photograph, and what the fog shader reads as full strength.
const VEIL: f32 = 0.74;

/// The cells the watched side sees, worked out once a tick.
#[derive(Resource, Default)]
pub struct FogGrid {
    /// The side the fog is of; absent for both sides' vision together, or
    /// when there is no fog.
    pub team: Option<Team>,
    /// Whether there is fog at all: one side's, or both sides' together
    /// (a spectator's and a replay's default, as in Dota).
    pub on: bool,
    /// Cells per axis.
    pub n: usize,
    /// One per cell, row-major from the south-west.
    pub cells: Vec<bool>,
    /// The tick and side the cells were worked out for.
    stamp: Option<(u32, Option<Team>)>,
    /// How dark each cell is drawn now, 0 lit to 1 fogged.
    shade: Vec<f32>,
}

impl FogGrid {
    /// Whether a bota world point is seen; everything is before the fog is
    /// known.
    pub fn lit(&self, x: f32, y: f32) -> bool {
        if self.n == 0 || x < 0.0 || y < 0.0 {
            return true;
        }
        let (cx, cy) = ((x / CELL) as usize, (y / CELL) as usize);
        cx >= self.n || cy >= self.n || self.cells[cy * self.n + cx]
    }
}

/// The image the veil is drawn from, for the minimap to lay over its photo.
#[derive(Resource)]
pub struct VeilImage(pub Handle<Image>);

/// The fog image and the fogged copies of the materials made so far.
#[derive(Resource)]
struct Veil {
    image: Handle<Image>,
    /// World units the image spans.
    span: f32,
    made: HashMap<AssetId<StandardMaterial>, Handle<FoggedMaterial>>,
}

/// Marks a mesh whose material the fog darkens: the ground, the trees and
/// the buildings. Units need none: those in the fog are not drawn.
#[derive(Component)]
pub struct Fogged;

/// The standard material with the fog laid over its lit colour.
pub type FoggedMaterial = ExtendedMaterial<StandardMaterial, FogExt>;

#[derive(Asset, AsBindGroup, Reflect, Clone)]
pub struct FogExt {
    #[texture(100)]
    #[sampler(101)]
    veil: Handle<Image>,
    #[uniform(102)]
    extent: Vec4,
}

impl MaterialExtension for FogExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://dbota/fog.wgsl".into()
    }
}

pub fn plugin(app: &mut App) {
    embedded_asset!(app, "fog.wgsl");
    app.init_resource::<FogGrid>()
        .add_plugins(MaterialPlugin::<FoggedMaterial>::default())
        .add_systems(Update, (work_out, spawn_veil, shade_veil).chain())
        .add_systems(PostUpdate, wrap)
        .add_systems(Startup, spawn_label)
        .add_systems(Update, label);
}

/// The vision a spectator or a replay is shown with, under the clock.
#[derive(Component)]
struct VisionLabel;

fn spawn_label(mut commands: Commands, fonts: Res<crate::ui::Fonts>) {
    commands.spawn((
        VisionLabel,
        crate::ui::text("", &fonts.bold, 13.0, crate::ui::INK),
        crate::ui::shadow(),
        Node {
            position_type: PositionType::Absolute,
            top: px(76),
            left: percent(50),
            margin: UiRect::left(px(-90)),
            width: px(180),
            justify_content: JustifyContent::Center,
            ..default()
        },
        TextLayout::justify(Justify::Center),
    ));
}

fn label(
    session: NonSend<Session>,
    grid: Res<FogGrid>,
    mut text: Single<&mut Text, With<VisionLabel>>,
) {
    let core = &session.0;
    let s = if core.my_slot.is_some() && core.pov.is_none() {
        String::new()
    } else {
        match (grid.on, grid.team) {
            (true, None) => "VISION: BOTH TEAMS [Tab]".to_string(),
            (true, Some(team)) => format!("VISION: {} [Tab]", format!("{team:?}").to_uppercase()),
            (false, _) => "VISION: ALL [Tab]".to_string(),
        }
    };
    if text.0 != s {
        text.0 = s;
    }
}

/// Works out the lit cells when a new tick or a new side comes.
fn work_out(session: NonSend<Session>, mut grid: ResMut<FogGrid>, mut lighting: Local<Lighting>) {
    let core = &session.0;
    let team = core.fog_team();
    let n = core.terrain_cells();
    let Some(view) = core.view() else {
        return;
    };
    // Without a side of one's own, a fogless stream (a replay, a spectator
    // seeing everything) is shown with the fog of both sides together:
    // lit wherever either side's units see.
    let union = team.is_none() && core.my_slot.is_none() && view.viewer.is_none();
    grid.team = team;
    grid.on = team.is_some() || union;
    if !grid.on {
        return;
    }
    if n == 0 {
        return;
    }
    if grid.n != n {
        grid.n = n;
        grid.cells = vec![true; n * n];
        grid.shade = vec![0.0; n * n];
    }
    // The last lighting worked out, once its task is done.
    if let Some(task) = &mut lighting.task
        && let Some((stamp, cells)) = block_on(poll_once(task))
    {
        lighting.task = None;
        if cells.len() == n * n {
            grid.cells = cells;
            grid.stamp = Some(stamp);
        }
    }
    if lighting.task.is_some() || grid.stamp == Some((view.tick, team)) {
        return;
    }
    // Lighting walks a line to every cell each unit could see, several
    // milliseconds a tick: it runs on another thread, a frame behind.
    let Some(statics) = core.statics() else {
        return;
    };
    if lighting
        .ground
        .as_ref()
        .is_none_or(|ground| !ground.fits(view))
    {
        lighting.ground = SightGround::lay(statics, Some(view)).map(Arc::new);
    }
    // The sight rule is laid on one grid size; any other has no fog.
    let Some(ground) = lighting.ground.clone() else {
        grid.on = false;
        return;
    };
    let view = view.clone();
    lighting.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let mut cells = vec![false; n * n];
        match team {
            Some(team) => ground.lit(&view, team, &mut cells),
            None => {
                let mut dire = vec![false; n * n];
                ground.lit(&view, Team::Radiant, &mut cells);
                ground.lit(&view, Team::Dire, &mut dire);
                for (c, d) in cells.iter_mut().zip(dire) {
                    *c |= d;
                }
            }
        }
        ((view.tick, team), cells)
    }));
}

/// The lighting under way on another thread, and the ground it reads.
#[derive(Default)]
struct Lighting {
    task: Option<Task<((u32, Option<Team>), Vec<bool>)>>,
    /// The ground, laid again when the forest changes.
    ground: Option<Arc<SightGround>>,
}

/// Makes the fog image once the fog is known.
fn spawn_veil(
    mut commands: Commands,
    grid: Res<FogGrid>,
    veil: Option<Res<Veil>>,
    mut images: ResMut<Assets<Image>>,
) {
    if veil.is_some() || grid.n == 0 {
        return;
    }
    let n = grid.n;
    let mut image = Image::new_fill(
        Extent3d {
            width: n as u32,
            height: n as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    let image = images.add(image);
    commands.insert_resource(VeilImage(image.clone()));
    commands.insert_resource(Veil {
        image,
        span: n as f32 * CELL,
        made: HashMap::new(),
    });
}

/// Gives every mesh marked [`Fogged`] the fogged copy of its material.
fn wrap(
    mut commands: Commands,
    veil: Option<ResMut<Veil>>,
    parts: Query<(Entity, &MeshMaterial3d<StandardMaterial>), With<Fogged>>,
    standard: Res<Assets<StandardMaterial>>,
    mut fogged: ResMut<Assets<FoggedMaterial>>,
) {
    let Some(mut veil) = veil else {
        return;
    };
    let veil = &mut *veil;
    for (entity, material) in &parts {
        let handle = match veil.made.get(&material.id()) {
            Some(handle) => handle.clone(),
            None => {
                let Some(base) = standard.get(&material.0) else {
                    continue;
                };
                let handle = fogged.add(FoggedMaterial {
                    base: base.clone(),
                    extension: FogExt {
                        veil: veil.image.clone(),
                        extent: Vec4::new(veil.span, VEIL, 0.0, 0.0),
                    },
                });
                veil.made.insert(material.id(), handle.clone());
                handle
            }
        };
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(handle));
    }
}

/// Eases every cell's darkness towards lit or fogged and writes the image
/// when anything moved; hides the veil when everything is seen.
fn shade_veil(
    time: Res<Time>,
    mut grid: ResMut<FogGrid>,
    veil: Option<Res<Veil>>,
    mut images: ResMut<Assets<Image>>,
    photo: Res<crate::minimap::PhotoTaken>,
    mut rows: Local<Vec<f32>>,
) {
    let Some(veil) = veil else {
        return;
    };
    // The minimap's photograph is of the map unfogged; when everything is
    // seen the shades ease back to lit.
    if !photo.0 {
        return;
    }
    let step = (time.delta_secs() * 5.0).clamp(0.05, 1.0);
    let grid = &mut *grid;
    let mut changed = false;
    let fogged = grid.on;
    for (shade, lit) in grid.shade.iter_mut().zip(&grid.cells) {
        let target = if *lit || !fogged { 0.0 } else { 1.0 };
        if (*shade - target).abs() > 0.003 {
            *shade += (target - *shade) * step;
            changed = true;
        } else if *shade != target {
            *shade = target;
            changed = true;
        }
    }
    if !changed {
        return;
    }
    let Some(mut image) = images.get_mut(&veil.image) else {
        return;
    };
    let n = grid.n;
    let Some(data) = image.data.as_mut() else {
        return;
    };
    // A tent blur over a cell either way rounds the border's cell steps:
    // 1-2-1 along each row, then along each column, the edges repeated.
    let shade = &grid.shade;
    rows.resize(n * n, 0.0);
    for (row, out) in shade.chunks_exact(n).zip(rows.chunks_exact_mut(n)) {
        for x in 0..n {
            out[x] = row[x.saturating_sub(1)] + 2.0 * row[x] + row[(x + 1).min(n - 1)];
        }
    }
    for cy in 0..n {
        let line = |y: usize| &rows[y * n..(y + 1) * n];
        let (south, here, north) = (
            line(cy.saturating_sub(1)),
            line(cy),
            line((cy + 1).min(n - 1)),
        );
        let o = (n - 1 - cy) * n * 4;
        for (cx, texel) in data[o..o + n * 4]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            let sum = south[cx] + 2.0 * here[cx] + north[cx];
            let a = (sum / 16.0 * VEIL * 255.0) as u8;
            texel.copy_from_slice(&[18, 20, 26, a]);
        }
    }
    // The image keeps its GPU texture when its data changes (Bevy writes
    // the new texels into it), so the materials bound to it need no
    // touching: touching them all rebuilt every one's bind group each frame.
}
