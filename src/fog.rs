//! The fog of war of the side watched through: which cells it sees, and the
//! ground, trees and buildings everywhere else dulled and darkened, easing
//! in and out.

use std::collections::HashMap;

use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    image::{ImageSampler, ImageSamplerDescriptor},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat},
    shader::ShaderRef,
};
use std::sync::Arc;

use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future::poll_once};
use bota_client_core::SightGround;
use bota_proto::{SeenBy, Team, UnitKind, UnitView};

use crate::session::Session;
use crate::terrain::CELL;

/// The image's alpha of a fully fogged cell: what the minimap lays over its
/// photograph, and what the fog shader reads as full strength.
const VEIL: f32 = 0.74;

/// What the fog multiplies the dulled lit colour by, in linear light, by
/// day: Dota-dense, about two fifths of the seen ground's brightness on
/// screen (a tenth and a half in linear light), and cooled.
const DAY_CAST: [f32; 3] = [0.14, 0.14, 0.15];

/// The cells the watched side sees, worked out once a tick.
#[derive(Resource, Default)]
pub struct FogGrid {
    /// The side the fog is of; absent for both sides' vision together, or
    /// when there is no fog.
    pub team: Option<Team>,
    /// Whether there is fog at all: one side's, or both sides' together
    /// (a spectator's and a replay's default, as in Dota).
    pub on: bool,
    /// Debug: the ground is drawn without the veil, so a visual check can
    /// set each fogged cell against the same cell unfogged.
    pub unveiled: bool,
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
    /// Whether the fog of the watched side hides a unit, missile or ground
    /// item of a whole-map view, which says which sides see it (`seen_by`):
    /// one side's fog hides what that side does not see, both sides' together
    /// what neither does. A view of a side comes fogged by the server and
    /// says nothing, so nothing in it is hidden here.
    pub fn hides(&self, seen_by: Option<SeenBy>) -> bool {
        self.on
            && seen_by.is_some_and(|seen| match self.team {
                Some(team) => !seen.by(team),
                None => !(seen.radiant || seen.dire),
            })
    }

    /// Whether a unit the fog hides is drawn anyway, dulled: the neutral
    /// creeps, under one side's fog and under both sides' together. Only a
    /// full view, which the client fogs itself, ever gets here; a seat's view
    /// and a spectator through a seat's eyes come fogged by the server and
    /// hold no such unit.
    pub fn shows_fogged(&self, unit: &UnitView) -> bool {
        self.on && unit.kind == UnitKind::CreepNeutral
    }

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
    /// One texel: the fog's cast for the time of day ([`cast`]).
    cast: Handle<Image>,
    /// World units the image spans.
    span: f32,
    /// By the material, the surface drawn with it, and whether it is
    /// tinted black ([`untouchable`]).
    made: HashMap<(AssetId<StandardMaterial>, u8, bool), Handle<FoggedMaterial>>,
}

/// A mesh given its fogged copy: the material it was given it from, and the
/// surface it is drawn as.
#[derive(Component)]
struct Wrapped(Handle<StandardMaterial>, u8);

/// A mesh of a hero's model, by the unit it draws: what Dota tints black
/// while the hero is untouchable at its fountain.
#[derive(Component)]
pub struct HeroPart(pub Entity);

/// What a fountain-untouchable hero's colours are multiplied by: Dota's
/// model "tinted black" (`modifier_fountain_invulnerability`).
const UNTOUCHABLE_TINT: f32 = 0.08;

/// Marks a mesh whose material the fog darkens: the ground, the trees and
/// the buildings. Units need none: those in the fog are not drawn.
#[derive(Component)]
pub struct Fogged;

/// Marks a mesh of a unit other than a building, which the visual clarity
/// aid makes a little more vivid (and does not dull); fogged too when it is also
/// [`Fogged`] (a neutral creep).
#[derive(Component)]
pub struct Figure;

/// The standard material with the fog laid over its lit colour.
pub type FoggedMaterial = ExtendedMaterial<StandardMaterial, FogExt>;

/// Bindless where the GPU can (the base material is too): every fogged
/// material then shares one bind group, and the map's thousands of meshes of
/// hundreds of materials are drawn in a few multi-draws instead of a batch,
/// with its own buffers and bind groups, per material and view.
#[derive(Asset, AsBindGroup, Reflect, Clone)]
#[data(100, FogUniform, binding_array(110))]
#[bindless(index_table(range(100..107), binding(111)))]
pub struct FogExt {
    /// x: world units the veil spans; y: its alpha of a fully fogged cell;
    /// z: what the surface is ([`SURFACE_PROP`] and on); w: how strongly the
    /// visual clarity aid is drawn, 0 to 1.
    params: Vec4,
    #[texture(101)]
    #[sampler(102)]
    veil: Handle<Image>,
    #[texture(103)]
    cast: Handle<Image>,
    /// The terrain's path textures and path field (`layers.rs`); white
    /// stand-ins on every other surface.
    #[texture(104)]
    #[sampler(105)]
    paths: Handle<Image>,
    #[texture(106)]
    field: Handle<Image>,
}

/// [`FogExt::params`] as the shader reads it.
#[derive(Clone, Default, ShaderType)]
pub struct FogUniform {
    params: Vec4,
}

impl From<&FogExt> for FogUniform {
    fn from(fog: &FogExt) -> FogUniform {
        FogUniform { params: fog.params }
    }
}

/// What a surface is, as the shader reads it: a tree, a building or a
/// prop; the terrain without its path layers; the terrain with them; a
/// unit the fog dulls; a unit it never touches; the river.
const SURFACE_PROP: u8 = 0;
const SURFACE_GROUND: u8 = 1;
const SURFACE_PATHS: u8 = 2;
const SURFACE_FOGGED_FIGURE: u8 = 3;
const SURFACE_FIGURE: u8 = 4;
/// The river: a prop whose normal map the shader slides along the current.
const SURFACE_WATER: u8 = 5;

impl MaterialExtension for FogExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://dbota/fog.wgsl".into()
    }
}

pub fn plugin(app: &mut App) {
    embedded_asset!(app, "fog.wgsl");
    app.init_resource::<FogGrid>()
        .add_plugins(MaterialPlugin::<FoggedMaterial>::default())
        .add_systems(
            Update,
            (work_out, spawn_veil, shade_veil, cast_veil).chain(),
        )
        .add_systems(
            PostUpdate,
            (wrap, untouchable)
                .chain()
                .after(crate::units::mark_figures),
        )
        .add_systems(
            Update,
            clarity.run_if(resource_changed::<crate::config::Settings>),
        );
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
    if n == 0 {
        return;
    }
    // Laid with no fog too: its materials also draw the paths and clarity.
    if grid.n != n {
        grid.n = n;
        grid.cells = vec![true; n * n];
        grid.shade = vec![0.0; n * n];
    }
    if !grid.on {
        return;
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
    let cast = images.add(Image::new_fill(
        Extent3d::default(),
        TextureDimension::D2,
        &cast_texel(0.0),
        // Half floats: a bindless material's textures must filter.
        TextureFormat::Rgba16Float,
        RenderAssetUsages::default(),
    ));
    commands.insert_resource(Veil {
        image,
        cast,
        span: n as f32 * CELL,
        made: HashMap::new(),
    });
}

/// Gives every mesh marked [`Fogged`] or [`Figure`] the copy of its material
/// the fog and the visual clarity aid draw; the terrain's also draws its
/// path layers.
#[allow(clippy::type_complexity)]
fn wrap(
    mut commands: Commands,
    veil: Option<ResMut<Veil>>,
    parts: Query<
        (
            Entity,
            &MeshMaterial3d<StandardMaterial>,
            Has<Fogged>,
            Has<Figure>,
            Has<crate::water::Water>,
        ),
        Or<(With<Fogged>, With<Figure>)>,
    >,
    standard: Res<Assets<StandardMaterial>>,
    mut fogged: ResMut<Assets<FoggedMaterial>>,
    layers: Option<Res<crate::layers::GroundLayers>>,
    terrain: Option<Res<crate::map::TerrainMaterial>>,
    settings: Res<crate::config::Settings>,
) {
    let Some(mut veil) = veil else {
        return;
    };
    let veil = &mut *veil;
    for (entity, material, fogged_part, figure, water) in &parts {
        let ground = terrain.as_ref().is_some_and(|t| t.0 == material.id());
        let paths = layers.as_ref().filter(|_| ground);
        let surface = match (figure, fogged_part, ground, paths.is_some()) {
            _ if water => SURFACE_WATER,
            (true, true, ..) => SURFACE_FOGGED_FIGURE,
            (true, false, ..) => SURFACE_FIGURE,
            (false, _, _, true) => SURFACE_PATHS,
            (false, _, true, false) => SURFACE_GROUND,
            (false, _, false, false) => SURFACE_PROP,
        };
        let made = Made {
            standard: &standard,
            fogged: &mut fogged,
            clarity: settings.clarity.strength(),
        };
        let Some(handle) = veil.copy(made, &material.0, surface, false, paths.map(|l| &**l)) else {
            continue;
        };
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert((MeshMaterial3d(handle), Wrapped(material.0.clone(), surface)));
    }
}

/// What a fogged copy is made with.
struct Made<'a> {
    standard: &'a Assets<StandardMaterial>,
    fogged: &'a mut Assets<FoggedMaterial>,
    clarity: f32,
}

impl Veil {
    /// The fogged copy of a material drawn as `surface`, tinted black or
    /// not, made the first time it is asked for; none while the material
    /// is not loaded.
    fn copy(
        &mut self,
        made: Made,
        material: &Handle<StandardMaterial>,
        surface: u8,
        tinted: bool,
        paths: Option<&crate::layers::GroundLayers>,
    ) -> Option<Handle<FoggedMaterial>> {
        let key = (material.id(), surface, tinted);
        if let Some(handle) = self.made.get(&key) {
            return Some(handle.clone());
        }
        let mut base = made.standard.get(material)?.clone();
        if tinted {
            let l = base.base_color.to_linear();
            base.base_color = Color::linear_rgba(
                l.red * UNTOUCHABLE_TINT,
                l.green * UNTOUCHABLE_TINT,
                l.blue * UNTOUCHABLE_TINT,
                l.alpha,
            );
            // Its sheen too: Dota's tint leaves no colour on the model.
            base.emissive = LinearRgba::BLACK;
            base.specular_tint = Color::BLACK;
        }
        let handle = made.fogged.add(FoggedMaterial {
            base,
            extension: FogExt {
                veil: self.image.clone(),
                params: Vec4::new(self.span, VEIL, f32::from(surface), made.clarity),
                cast: self.cast.clone(),
                paths: paths.map(|l| l.detail.clone()).unwrap_or_default(),
                field: paths.map(|l| l.field.clone()).unwrap_or_default(),
            },
        });
        self.made.insert(key, handle.clone());
        Some(handle)
    }
}

/// Tints every hero black while it is untouchable at its fountain, as
/// Dota's `modifier_fountain_invulnerability` does, and gives its colours
/// back when that ends.
#[allow(clippy::type_complexity)]
fn untouchable(
    mut commands: Commands,
    session: NonSend<crate::session::Session>,
    veil: Option<ResMut<Veil>>,
    parts: Query<(Entity, &HeroPart, &Wrapped, &MeshMaterial3d<FoggedMaterial>)>,
    units: Query<&crate::units::Unit>,
    standard: Res<Assets<StandardMaterial>>,
    mut fogged: ResMut<Assets<FoggedMaterial>>,
    settings: Res<crate::config::Settings>,
) {
    let (Some(mut veil), Some(view)) = (veil, session.0.view()) else {
        return;
    };
    for (entity, part, Wrapped(material, surface), now) in &parts {
        let tinted = units
            .get(part.0)
            .ok()
            .and_then(|unit| view.unit(unit.id))
            .is_some_and(|unit| {
                unit.effects
                    .iter()
                    .any(|e| e.id == bota_proto::EffectId::FOUNTAIN_INVULNERABLE)
            });
        let made = Made {
            standard: &standard,
            fogged: &mut fogged,
            clarity: settings.clarity.strength(),
        };
        if let Some(handle) = veil.copy(made, material, *surface, tinted, None)
            && now.0 != handle
        {
            commands.entity(entity).insert(MeshMaterial3d(handle));
        }
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
    mut blur: Local<Blur>,
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
    let n = grid.n;
    if n == 0 {
        return;
    }
    let fogged = grid.on && !grid.unveiled;
    // The rows whose shades eased this frame: only their texels (and their
    // neighbours', through the blur) are worked out again.
    blur.changed.clear();
    blur.changed.resize(n, false);
    let rows_of = grid
        .shade
        .chunks_exact_mut(n)
        .zip(grid.cells.chunks_exact(n));
    for ((shades, lit), changed) in rows_of.zip(blur.changed.iter_mut()) {
        for (shade, lit) in shades.iter_mut().zip(lit) {
            let target = if *lit || !fogged { 0.0 } else { 1.0 };
            if (*shade - target).abs() > 0.003 {
                *shade += (target - *shade) * step;
                *changed = true;
            } else if *shade != target {
                *shade = target;
                *changed = true;
            }
        }
    }
    if !blur.changed.contains(&true) {
        return;
    }
    // The first write into a new grid or a new image: every row.
    if blur.rows.len() != n * n || blur.image != Some(veil.image.id()) {
        blur.rows.clear();
        blur.rows.resize(n * n, 0.0);
        blur.changed.fill(true);
        blur.image = Some(veil.image.id());
    }
    let Some(mut image) = images.get_mut(&veil.image) else {
        return;
    };
    let Some(data) = image.data.as_mut() else {
        return;
    };
    // A tent blur over a cell either way rounds the border's cell steps:
    // 1-2-1 along each row, then along each column, the edges repeated.
    let Blur { rows, changed, .. } = &mut *blur;
    let shade = &grid.shade;
    for ((row, out), _) in shade
        .chunks_exact(n)
        .zip(rows.chunks_exact_mut(n))
        .zip(changed.iter())
        .filter(|(_, c)| **c)
    {
        for x in 0..n {
            out[x] = row[x.saturating_sub(1)] + 2.0 * row[x] + row[(x + 1).min(n - 1)];
        }
    }
    for cy in 0..n {
        let near = |y: usize| changed[y];
        if !(near(cy) || near(cy.saturating_sub(1)) || near((cy + 1).min(n - 1))) {
            continue;
        }
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

/// The veil's blur between frames: each row's sums along the row, and which
/// rows' shades changed this frame.
#[derive(Default)]
struct Blur {
    rows: Vec<f32>,
    changed: Vec<bool>,
    /// The image the rows were written into.
    image: Option<AssetId<Image>>,
}

/// The fog's cast, by how far the light has turned towards night: the day
/// cast cools fogged ground against sunlit ground; by night the moonlight
/// already cools everything, and cooling the fog again tints it blue (near
/// black, where night's fog lies, a cast shows twice its daytime colour on
/// screen), so it turns to the grey of the same brightness.
fn cast(night: f32) -> [f32; 3] {
    let grey = 0.2126 * DAY_CAST[0] + 0.7152 * DAY_CAST[1] + 0.0722 * DAY_CAST[2];
    DAY_CAST.map(|c| c + (grey - c) * night.clamp(0.0, 1.0))
}

fn cast_texel(night: f32) -> Vec<u8> {
    let [r, g, b] = cast(night);
    [r, g, b, 1.0]
        .iter()
        .flat_map(|&c| half::f16::from_f32(c).to_le_bytes())
        .collect()
}

/// Rewrites the cast when the light turns; the materials share its image,
/// whose texel Bevy writes into the texture they are bound to.
fn cast_veil(
    daylight: Res<crate::daylight::Daylight>,
    veil: Option<Res<Veil>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(veil) = veil else {
        return;
    };
    if !daylight.is_changed() && !veil.is_added() {
        return;
    }
    let texel = cast_texel(daylight.night);
    if let Some(mut image) = images.get_mut(&veil.cast)
        && image.data.as_deref() != Some(&texel[..])
    {
        image.data = Some(texel);
    }
}

/// Sets the visual clarity aid's strength on every fogged material when the
/// settings change it.
fn clarity(
    settings: Res<crate::config::Settings>,
    veil: Option<Res<Veil>>,
    mut fogged: ResMut<Assets<FoggedMaterial>>,
) {
    let Some(veil) = veil else {
        return;
    };
    let strength = settings.clarity.strength();
    for handle in veil.made.values() {
        if fogged
            .get(handle)
            .is_some_and(|m| m.extension.params.w != strength)
            && let Some(mut m) = fogged.get_mut(handle)
        {
            m.extension.params.w = strength;
        }
    }
}
