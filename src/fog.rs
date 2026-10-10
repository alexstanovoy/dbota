//! The fog of war of the side watched through: which cells it sees, and a
//! dark veil laid over the ground everywhere else, easing in and out.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageSampler, ImageSamplerDescriptor},
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use bota_proto::Team;

use crate::map::Ground;
use crate::session::Session;
use crate::terrain::CELL;

/// How dark the veil is at full strength, 0 to 1.
const VEIL: f32 = 0.62;
/// World units the veil floats over the ground.
const LIFT: f32 = 10.0;

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

/// The veil over the ground and the image it is drawn from.
#[derive(Resource)]
struct Veil {
    entity: Entity,
    image: Handle<Image>,
    material: Handle<StandardMaterial>,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<FogGrid>()
        .add_systems(Update, (work_out, spawn_veil, shade_veil).chain());
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
    debug!("fog of {team:?} at tick {}: {} cells lit", view.tick, grid.cells.iter().filter(|c| **c).count());
}

/// Lays the veil over the ground once the ground is known.
fn spawn_veil(
    mut commands: Commands,
    grid: Res<FogGrid>,
    ground: Res<Ground>,
    veil: Option<Res<Veil>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if veil.is_some() || grid.n == 0 || !ground.known() {
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
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(image.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let entity = commands
        .spawn((
            Mesh3d(meshes.add(drape(&ground, n))),
            MeshMaterial3d(material.clone()),
            Transform::IDENTITY,
            NotShadowCaster,
            NotShadowReceiver,
            bevy::camera::visibility::RenderLayers::layer(1),
        ))
        .id();
    commands.insert_resource(VeilImage(image.clone()));
    commands.insert_resource(Veil {
        entity,
        image,
        material,
    });
    debug!("fog veil laid over {n}x{n} cells");
}

/// A grid over the whole map one cell apart, laid on the ground.
fn drape(ground: &Ground, n: usize) -> Mesh {
    let m = n + 1;
    let mut positions = Vec::with_capacity(m * m);
    let mut uvs = Vec::with_capacity(m * m);
    for cy in 0..m {
        for cx in 0..m {
            let (x, y) = (cx as f32 * CELL, cy as f32 * CELL);
            positions.push([x, ground.at(x, y) + LIFT, -y]);
            uvs.push([cx as f32 / n as f32, 1.0 - cy as f32 / n as f32]);
        }
    }
    let mut indices = Vec::with_capacity(n * n * 6);
    for cy in 0..n {
        for cx in 0..n {
            let a = (cy * m + cx) as u32;
            let (b, c) = (a + 1, a + m as u32);
            let d = c + 1;
            indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; m * m]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Eases every cell's darkness towards lit or fogged and writes the image
/// when anything moved; hides the veil when everything is seen.
fn shade_veil(
    time: Res<Time>,
    mut grid: ResMut<FogGrid>,
    veil: Option<Res<Veil>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut visibility: Query<&mut Visibility>,
) {
    let Some(veil) = veil else {
        return;
    };
    let shown = grid.team.is_some();
    if let Ok(mut v) = visibility.get_mut(veil.entity) {
        let want = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if !shown {
        return;
    }
    let step = (time.delta_secs() * 5.0).clamp(0.05, 1.0);
    let grid = &mut *grid;
    let mut changed = false;
    for (shade, lit) in grid.shade.iter_mut().zip(&grid.cells) {
        let target = if *lit { 0.0 } else { 1.0 };
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
            data[at..at + 4].copy_from_slice(&[4, 6, 14, a]);
        }
    }
    // The material's binding of the image is only rebuilt when the
    // material itself is touched.
    let _ = materials.get_mut(&veil.material);
}
