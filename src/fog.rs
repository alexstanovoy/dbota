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
use bota_proto::Team;

use crate::session::Session;
use crate::terrain::CELL;

/// The image's alpha of a fully fogged cell: what the minimap lays over its
/// photograph, and what the fog shader reads as full strength.
const VEIL: f32 = 0.74;

/// The cells the watched side sees, worked out once a tick.
#[derive(Resource, Default)]
pub struct FogGrid {
    /// The side the fog is of; absent when everything is seen.
    pub team: Option<Team>,
    /// Cells per axis.
    pub n: usize,
    /// One per cell, row-major from the south-west.
    pub cells: Vec<bool>,
    /// The tick and side the cells were worked out for.
    stamp: Option<(u32, Team)>,
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
        .add_systems(PostUpdate, wrap);
}

/// Works out the lit cells when a new tick or a new side comes.
fn work_out(session: NonSend<Session>, mut grid: ResMut<FogGrid>) {
    let core = &session.0;
    let team = core.fog_team();
    let n = core.terrain_cells;
    grid.team = team;
    let (Some(team), Some(view)) = (team, &core.view) else {
        return;
    };
    if n == 0 {
        return;
    }
    if grid.n != n {
        grid.n = n;
        grid.cells = vec![true; n * n];
        grid.shade = vec![0.0; n * n];
    }
    if grid.stamp == Some((view.tick, team)) {
        return;
    }
    grid.stamp = Some((view.tick, team));
    bota_client_core::light(core, view, team, &mut grid.cells);
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
    mut materials: ResMut<Assets<FoggedMaterial>>,
    photo: Res<crate::minimap::PhotoTaken>,
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
    let fogged = grid.team.is_some();
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
    for cy in 0..n {
        for cx in 0..n {
            let at = ((n - 1 - cy) * n + cx) * 4;
            let a = (grid.shade[cy * n + cx] * VEIL * 255.0) as u8;
            data[at..at + 4].copy_from_slice(&[18, 20, 26, a]);
        }
    }
    // A material's binding of the image is only rebuilt when the material
    // itself is touched.
    for handle in veil.made.values() {
        let _ = materials.get_mut(handle);
    }
}
