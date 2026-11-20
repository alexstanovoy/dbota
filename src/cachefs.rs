//! The asset cache as the client reads it. On a desktop a file is read
//! whole into the loader's own buffer, on the IO thread that asked for it:
//! Bevy's default file reader passes every file through an 8 MiB pipe of
//! its own, and with dozens of textures loading at once those pipes held
//! over half a gigabyte. In a browser the files come over HTTP, as Bevy
//! fetches them.
//!
//! The cache's model and map textures are KTX2 files block-compressed as Dota
//! stores them; on a GPU that samples no block format (a browser's WebGPU may
//! lack `texture-compression-bc`) the reader hands them on decoded to plain
//! channels.
//!
//! A model (glTF or GLB) is let through only when it names no image inside itself and no file
//! but plain ones next to it (`gltf_file`): Bevy decodes an embedded image with no limits.
//!
//! Either way textures (PNG and KTX2 files) are let through only while the
//! ones decoded before them have reached the GPU, a budget of decoded bytes
//! at a time: loads finish far faster than a frame takes them in while the
//! map is prepared, and every texture of the cache sat decoded in memory at
//! once.
//!
//! PNG residency shares the importer's world and interface envelopes. A path holds its
//! largest admitted size while Bevy owns it. The terrain baker uses the world pool's
//! scratch allowance for one decode at a time.

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

use bevy::{
    asset::io::{AssetReader, AssetReaderError, PathStream, Reader},
    prelude::*,
};

use crate::texture_file::{HEAD_BYTES, decoded_size};
use crate::{cache_path, gltf_file};
use dbota_import::png::{self, Class, MAX_IMAGE_BYTES, MAX_PATHS_PER_CLASS};

/// Decoded texture bytes let through at once, besides the one under way:
/// less in a browser, whose memory never shrinks back.
const DECODE_BUDGET: usize = if cfg!(target_arch = "wasm32") {
    64 << 20
} else {
    192 << 20
};
/// Frames without a texture arriving after which the budget lets every
/// texture through: a load that never reports back must not stall the rest.
const STALL_FRAMES: u32 = 120;
fn png_class(path: &Path) -> Class {
    Class::of(path.to_str().expect("cache paths are validated UTF-8"))
}

fn pool_name(class: Class) -> &'static str {
    match class {
        Class::Interface => "resident interface image budget",
        Class::World => "resident world image budget",
    }
}

#[derive(Debug)]
struct ImageLimit {
    path: PathBuf,
    bytes: usize,
    limit_name: &'static str,
    used: usize,
    limit: usize,
    unit: &'static str,
}

impl std::fmt::Display for ImageLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} exceeded (image: {} bytes, {} > {} {})",
            self.path.display(),
            self.limit_name,
            self.bytes,
            self.used,
            self.limit,
            self.unit
        )
    }
}

impl std::error::Error for ImageLimit {}

impl ImageLimit {
    fn new(
        path: &Path,
        bytes: usize,
        limit_name: &'static str,
        used: usize,
        limit: usize,
        unit: &'static str,
    ) -> Self {
        Self {
            path: path.into(),
            bytes,
            limit_name,
            used,
            limit,
            unit,
        }
    }

    fn per_image(path: &Path, bytes: usize) -> Option<Self> {
        (bytes > MAX_IMAGE_BYTES).then(|| {
            Self::new(
                path,
                bytes,
                "per-image budget",
                bytes,
                MAX_IMAGE_BYTES,
                "decoded bytes",
            )
        })
    }
}

impl From<ImageLimit> for AssetReaderError {
    fn from(limit: ImageLimit) -> Self {
        std::io::Error::other(limit).into()
    }
}

/// The textures read and not yet handed to the GPU, by cache path, with
/// their decoded size, and the reads waiting for room.
#[derive(Default)]
struct InFlight {
    bytes: usize,
    textures: HashMap<PathBuf, usize>,
    waiting: Vec<Waker>,
    /// Frames since a texture last arrived while some were in flight.
    idle: u32,
    /// The app that settles the budget has ended: no frame will make room again.
    ended: bool,
}

#[derive(Default)]
struct ResidentImages {
    bytes: usize,
    generation: u64,
    images: HashMap<PathBuf, ResidentImage>,
}

#[derive(Clone, Copy)]
struct ResidentImage {
    bytes: usize,
    generation: u64,
}

/// Decode allowances shared by the reader and the systems that settle uploads.
#[derive(Clone, Default)]
pub struct DecodeBudget {
    in_flight: Arc<Mutex<InFlight>>,
    resident: Arc<Mutex<[ResidentImages; 2]>>,
    terrain_decode: Arc<Mutex<()>>,
    bake_refusals: Arc<Mutex<Vec<ImageLimit>>>,
}

/// The app's hold on the decode budget, through which its frames settle it.
///
/// The reader outlives the app: the asset server it is part of lives as long as its last load.
/// When the app ends, a read waiting for room would wait for a frame that never comes, and its
/// load would keep the asset server, and all it loaded, for the rest of the process. So the reads
/// waiting then, and those after, end refused.
#[derive(Resource)]
pub struct Settler(DecodeBudget);

impl Settler {
    pub fn new(budget: DecodeBudget) -> Settler {
        Settler(budget)
    }

    pub fn budget(&self) -> DecodeBudget {
        self.0.clone()
    }
}

impl Drop for Settler {
    fn drop(&mut self) {
        let mut flight = self
            .0
            .in_flight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        flight.ended = true;
        flight.textures.clear();
        flight.bytes = 0;
        DecodeBudget::wake(&mut flight);
    }
}

impl DecodeBudget {
    fn retain(&self, path: &Path, bytes: usize) -> Result<(), AssetReaderError> {
        if let Some(limit) = ImageLimit::per_image(path, bytes) {
            return Err(limit.into());
        }
        let class = png_class(path);
        let mut pools = self
            .resident
            .lock()
            .map_err(|_| std::io::Error::other("resident image budget is unavailable"))?;
        let resident = &mut pools[class as usize];
        let previous = resident.images.get(path).map(|entry| entry.bytes);
        // A failed shrinking reload can leave the previous GPU image alive.
        let reserved = bytes.max(previous.unwrap_or(0));
        let total = resident.bytes - previous.unwrap_or(0) + reserved;
        if total > class.playback_bytes() {
            return Err(ImageLimit::new(
                path,
                bytes,
                pool_name(class),
                total,
                class.playback_bytes(),
                "decoded bytes",
            )
            .into());
        }
        if previous.is_none() && resident.images.len() >= MAX_PATHS_PER_CLASS {
            return Err(ImageLimit::new(
                path,
                bytes,
                pool_name(class),
                resident.images.len() + 1,
                MAX_PATHS_PER_CLASS,
                "paths",
            )
            .into());
        }
        resident.bytes = total;
        resident.generation = resident.generation.wrapping_add(1);
        resident.images.insert(
            path.to_path_buf(),
            ResidentImage {
                bytes: reserved,
                generation: resident.generation,
            },
        );
        Ok(())
    }

    fn prune_resident(&self, mut live: impl FnMut(&Path) -> bool) {
        // AssetServer takes its own locks. Snapshot before calling it, then release only
        // reservations that a concurrent reload has not renewed since the snapshot.
        let Ok(pools) = self.resident.lock() else {
            return;
        };
        let candidates: Vec<_> = pools
            .iter()
            .flat_map(|pool| {
                pool.images
                    .iter()
                    .map(|(path, entry)| (path.clone(), entry.generation))
            })
            .collect();
        drop(pools);
        let unused: Vec<_> = candidates
            .into_iter()
            .filter(|(path, _)| !live(path))
            .collect();
        let Ok(mut pools) = self.resident.lock() else {
            return;
        };
        for (path, generation) in unused {
            let pool = &mut pools[png_class(&path) as usize];
            if let Some(entry) = pool.images.get(&path)
                && entry.generation == generation
            {
                pool.bytes -= entry.bytes;
                pool.images.remove(&path);
            }
        }
    }

    fn reserve_terrain(
        &self,
        path: &Path,
        bytes: usize,
    ) -> Result<TerrainReservation<'_>, ImageLimit> {
        if let Some(limit) = ImageLimit::per_image(path, bytes) {
            return Err(limit);
        }
        let class = Class::World;
        let mut pools = self
            .resident
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let pool = &mut pools[class as usize];
        let total = pool.bytes + bytes;
        if total > class.playback_bytes() {
            return Err(ImageLimit::new(
                path,
                bytes,
                pool_name(class),
                total,
                class.playback_bytes(),
                "decoded bytes",
            ));
        }
        pool.bytes = total;
        Ok(TerrainReservation {
            budget: self,
            bytes,
        })
    }

    /// Keeps the decode charged for the projection's lifetime. The baker only keeps the
    /// result of the projection, so its largest overlapping decode is one image.
    pub fn terrain_png<T>(
        &self,
        path: &Path,
        bytes: &[u8],
        project: impl FnOnce(&Image) -> Option<T>,
    ) -> Option<T> {
        let size = png::decoded_bytes(bytes).ok()?;
        let _serial = self.terrain_decode.lock().ok()?;
        let _charge = match self.reserve_terrain(path, size) {
            Ok(charge) => charge,
            Err(limit) => {
                if let Ok(mut failures) = self.bake_refusals.lock()
                    && failures.len() < dbota_import::index::MAX_FILES
                {
                    failures.push(limit);
                }
                return None;
            }
        };
        let image = Image::from_buffer(
            bytes,
            bevy::image::ImageType::Extension("png"),
            bevy::image::CompressedImageFormats::NONE,
            true,
            bevy::image::ImageSampler::Default,
            bevy::asset::RenderAssetUsages::MAIN_WORLD,
        )
        .ok()?;
        project(&image)
    }

    /// Resolves once a texture of `bytes` decoded fits, and counts it. One
    /// larger than the whole budget goes alone, so it counts as the budget.
    fn take<'a>(&'a self, path: &'a Path, bytes: usize) -> Take<'a> {
        Take {
            budget: self,
            path,
            bytes: bytes.min(DECODE_BUDGET),
        }
    }

    /// Lets the waiting reads try again.
    fn wake(flight: &mut InFlight) {
        for waker in flight.waiting.drain(..) {
            waker.wake();
        }
    }

    /// Lets a texture's bytes go once it is on the GPU or failed.
    fn settle(&self, path: &Path) {
        if let Ok(mut flight) = self.in_flight.lock()
            && let Some(bytes) = flight.textures.remove(path)
        {
            flight.bytes -= bytes;
            flight.idle = 0;
            Self::wake(&mut flight);
        }
    }

    /// The textures read and not yet settled.
    fn in_flight(&self) -> Vec<PathBuf> {
        self.in_flight
            .lock()
            .map(|flight| flight.textures.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Counts a frame; after [`STALL_FRAMES`] of nothing arriving, forgets
    /// what is in flight.
    fn tick(&self) {
        let Ok(mut flight) = self.in_flight.lock() else {
            return;
        };
        if flight.textures.is_empty() {
            flight.idle = 0;
            return;
        }
        flight.idle += 1;
        if flight.idle >= STALL_FRAMES {
            warn!(
                "textures: {} never reported loaded; the decode budget lets the rest through",
                flight.textures.len()
            );
            flight.textures.clear();
            flight.bytes = 0;
            flight.idle = 0;
            Self::wake(&mut flight);
        }
    }
}

struct TerrainReservation<'a> {
    budget: &'a DecodeBudget,
    bytes: usize,
}

impl Drop for TerrainReservation<'_> {
    fn drop(&mut self) {
        let mut pools = self
            .budget
            .resident
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pools[Class::World as usize].bytes -= self.bytes;
    }
}

/// A read waiting for room in the decode budget.
struct Take<'a> {
    budget: &'a DecodeBudget,
    path: &'a Path,
    bytes: usize,
}

impl Future for Take<'_> {
    type Output = Result<(), AssetReaderError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Ok(mut flight) = self.budget.in_flight.lock() else {
            return Poll::Ready(Ok(()));
        };
        if flight.ended {
            let why = format!(
                "{}: the client that takes textures in has ended",
                self.path.display()
            );
            return Poll::Ready(Err(AssetReaderError::Io(std::io::Error::other(why).into())));
        }
        if flight.bytes > 0 && flight.bytes + self.bytes > DECODE_BUDGET {
            flight.waiting.push(cx.waker().clone());
            return Poll::Pending;
        }
        flight.bytes += self.bytes;
        if let Some(old) = flight.textures.insert(self.path.to_path_buf(), self.bytes) {
            flight.bytes -= old;
        }
        Poll::Ready(Ok(()))
    }
}

/// Whether the GPU samples block-compressed textures (BC1 to BC7): taken to,
/// until the renderer says.
#[derive(Resource, Clone)]
pub struct BlockTextures(Arc<AtomicBool>);

impl Default for BlockTextures {
    fn default() -> Self {
        BlockTextures(Arc::new(AtomicBool::new(true)))
    }
}

impl BlockTextures {
    fn sampled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Notes before anything loads whether the GPU samples block formats.
fn note_block_textures(
    device: Option<Res<bevy::render::renderer::RenderDevice>>,
    block: Res<BlockTextures>,
) {
    let Some(device) = device else {
        return;
    };
    let sampled = device
        .features()
        .contains(bevy::render::settings::WgpuFeatures::TEXTURE_COMPRESSION_BC);
    if !sampled {
        info!("the GPU samples no BC textures: the cache's are decoded as they load");
    }
    block.0.store(sampled, Ordering::Relaxed);
}

/// Settles the decode budget: a texture added this frame reaches the GPU
/// when the frame ends, and is let go at the start of the next.
pub fn plugin(app: &mut App) {
    app.init_resource::<DueTextures>()
        .init_resource::<ImageRefusals>()
        .add_systems(PreStartup, note_block_textures)
        .add_systems(
            PostUpdate,
            (arrived, report_refusals)
                .chain()
                .after(bevy::asset::AssetEventSystems),
        )
        .add_systems(First, settle);
}

#[derive(Resource, Default)]
struct ImageRefusals {
    paths: HashSet<PathBuf>,
    drawn: HashSet<AssetId<Image>>,
    total: usize,
    label: Option<Entity>,
    dismissed: bool,
}

impl ImageRefusals {
    fn record(&mut self, limit: &ImageLimit) {
        if self.paths.contains(&limit.path) {
            return;
        }
        self.total = self.total.saturating_add(1);
        if self.paths.len() < dbota_import::index::MAX_FILES {
            self.paths.insert(limit.path.clone());
            warn!(count = self.total, "{limit}");
        } else if self.total == dbota_import::index::MAX_FILES + 1 {
            warn!("image refusal log is full; additional refusals still draw their fallback");
        }
    }

    fn notice(&self) -> String {
        match self.total {
            1 => "1 image could not be loaded".to_owned(),
            count => format!("{count} images could not be loaded"),
        }
    }
}

fn report_refusals(
    mut commands: Commands,
    mut failed: MessageReader<bevy::asset::AssetLoadFailedEvent<Image>>,
    mut images: ResMut<Assets<Image>>,
    server: Res<AssetServer>,
    settler: Res<Settler>,
    mut refusals: ResMut<ImageRefusals>,
    fonts: Option<Res<crate::ui::Fonts>>,
) {
    use bevy::asset::AssetLoadError;

    let previous = refusals.total;
    for event in failed.read() {
        let AssetLoadError::AssetReaderError(AssetReaderError::Io(error)) = &event.error else {
            continue;
        };
        let Some(limit) = error
            .get_ref()
            .and_then(|error| error.downcast_ref::<ImageLimit>())
        else {
            continue;
        };
        // A reload keeps the previous image while a failed first read has no drawable image.
        if !refusals.drawn.contains(&event.id)
            && !images.contains(event.id)
            && server.is_managed(event.id)
        {
            let fallback = match png_class(&limit.path) {
                Class::World => Image::default(),
                Class::Interface => missing_image(),
            };
            let _ = images.insert(event.id, fallback);
        }
        refusals.record(limit);
    }
    let direct = settler
        .0
        .bake_refusals
        .lock()
        .map(|mut failures| std::mem::take(&mut *failures))
        .unwrap_or_default();
    for limit in direct {
        refusals.record(&limit);
    }
    if refusals.total == previous || refusals.dismissed {
        return;
    }
    let message = Text::new(refusals.notice());
    if let Some(label) = refusals.label {
        commands.entity(label).insert(message);
    } else {
        refusals.label = Some(spawn_image_notice(&mut commands, fonts.as_deref(), message));
    }
}

fn spawn_image_notice(
    commands: &mut Commands,
    fonts: Option<&crate::ui::Fonts>,
    message: Text,
) -> Entity {
    let font = TextFont {
        font: fonts
            .map(|fonts| fonts.regular.clone().into())
            .unwrap_or_default(),
        ..TextFont::default().with_font_size(15.0)
    };
    let row = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(65),
                right: px(16),
                padding: UiRect::all(px(7)),
                column_gap: px(10),
                ..default()
            },
            BackgroundColor(crate::theme::FIELD_OFF.0),
            GlobalZIndex(500),
        ))
        .id();
    let label = commands
        .spawn((message, font.clone(), TextColor(Color::WHITE), ChildOf(row)))
        .id();
    commands
        .spawn((
            bevy::ui_widgets::Button,
            Text::new("×"),
            font,
            TextColor(Color::WHITE),
            ChildOf(row),
        ))
        .observe(
            move |_: On<bevy::ui_widgets::Activate>,
                  mut commands: Commands,
                  mut refusals: ResMut<ImageRefusals>| {
                refusals.dismissed = true;
                refusals.label = None;
                commands.entity(row).despawn();
            },
        );
    label
}

fn missing_image() -> Image {
    use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let mut image = Image::new_fill(
        Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[
            224, 45, 167, 255, 52, 37, 51, 255, 52, 37, 51, 255, 224, 45, 167, 255,
        ],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::nearest()
    });
    image
}

/// The textures that arrived this frame, to let go at the start of the next.
#[derive(Resource, Default)]
struct DueTextures(Vec<PathBuf>);

fn arrived(
    mut added: MessageReader<AssetEvent<Image>>,
    mut failed: MessageReader<bevy::asset::AssetLoadFailedEvent<Image>>,
    server: Res<AssetServer>,
    settler: Res<Settler>,
    mut due: ResMut<DueTextures>,
    mut refusals: ResMut<ImageRefusals>,
) {
    let budget = &settler.0;
    let mut dropped = false;
    for event in added.read() {
        dropped |= matches!(event, AssetEvent::Unused { .. });
        match event {
            AssetEvent::Added { id } => {
                if let Some(path) = server.get_path(*id) {
                    due.0.push(path.path().to_path_buf());
                    if path.source().as_str() == Some(crate::CACHE_SOURCE)
                        && cache_path::image_format(path.path())
                            == Some(cache_path::ImageFormat::Png)
                    {
                        // GPU-only images leave Assets<Image> without a Removed event.
                        refusals.drawn.insert(*id);
                    }
                }
            }
            AssetEvent::Removed { id } | AssetEvent::Unused { id } => {
                refusals.drawn.remove(id);
            }
            _ => {}
        }
    }
    for event in failed.read() {
        budget.settle(event.path.path());
    }
    // A texture whose every handle was let go while it loaded never arrives:
    // a glTF's loader reads every image the file lists and drops those no
    // material of its uses (a mask for a slot the renderer has not).
    for path in budget.in_flight() {
        let asset = bevy::asset::AssetPath::from(path.clone()).with_source(crate::CACHE_SOURCE);
        if server.get_path_ids(asset).is_empty() {
            budget.settle(&path);
            dropped = true;
        }
    }
    if dropped {
        budget.prune_resident(|path| {
            let asset =
                bevy::asset::AssetPath::from(path.to_path_buf()).with_source(crate::CACHE_SOURCE);
            server.get_path_id(asset).is_some()
        });
    }
}

fn settle(mut due: ResMut<DueTextures>, settler: Res<Settler>) {
    let budget = &settler.0;
    for path in due.0.drain(..) {
        budget.settle(&path);
    }
    budget.tick();
}

/// Reads cache files and their metadata; raw directory enumeration is disabled.
pub struct CacheReader {
    budget: DecodeBudget,
    block: BlockTextures,
    root: PathBuf,
    #[cfg(not(target_arch = "wasm32"))]
    files: bevy::asset::io::file::FileAssetReader,
    #[cfg(target_arch = "wasm32")]
    files: bevy::asset::io::wasm::HttpWasmAssetReader,
}

impl CacheReader {
    pub fn new(root: PathBuf, budget: DecodeBudget, block: BlockTextures) -> CacheReader {
        CacheReader {
            budget,
            block,
            #[cfg(not(target_arch = "wasm32"))]
            files: bevy::asset::io::file::FileAssetReader::new(&root),
            #[cfg(target_arch = "wasm32")]
            files: bevy::asset::io::wasm::HttpWasmAssetReader::new(&root),
            root,
        }
    }
}

impl CacheReader {
    fn check_path(path: &Path) -> Result<(), AssetReaderError> {
        if cache_path::is_safe(path) {
            return Ok(());
        }
        Err(AssetReaderError::Io(
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "not a plain relative cache path",
            )
            .into(),
        ))
    }

    /// Whether the file at `path` is handed on decoded: a KTX2 texture when
    /// the GPU samples no block format.
    fn decodes(&self, path: &Path) -> bool {
        !self.block.sampled()
            && cache_path::image_format(path) == Some(cache_path::ImageFormat::Ktx2)
    }

    /// A KTX2 texture's bytes decoded to plain channels, within the decode
    /// budget like any texture; one no decoder takes is refused.
    async fn decoded(&self, path: &Path, bytes: &[u8]) -> Result<Vec<u8>, AssetReaderError> {
        self.admit(path, &bytes[..bytes.len().min(HEAD_BYTES)])
            .await?;
        heatel::texture::ktx2_to_plain(bytes).map_err(|why| {
            let why = format!("{}: {why}", path.display());
            AssetReaderError::Io(std::io::Error::other(why).into())
        })
    }

    /// Refuses a model that names what the cache's models never do (see [`gltf_file`]).
    fn vet_model(path: &Path, bytes: &[u8]) -> Result<(), AssetReaderError> {
        gltf_file::check(bytes).map_err(|why| {
            let why = format!("{}: {why}", path.display());
            AssetReaderError::Io(std::io::Error::other(why).into())
        })
    }

    /// Waits until a texture file fits the decode budget, and refuses one
    /// whose header no decoder may be trusted with.
    async fn admit(&self, path: &Path, head: &[u8]) -> Result<(), AssetReaderError> {
        let png = cache_path::image_format(path) == Some(cache_path::ImageFormat::Png)
            || head.starts_with(&png::MAGIC);
        let size = if png {
            Some(png::decoded_bytes(head))
        } else {
            decoded_size(head)
        };
        let Some(size) = size else {
            return Ok(());
        };
        let failed = |why: String| {
            let why = format!("{}: {why}", path.display());
            AssetReaderError::Io(std::io::Error::other(why).into())
        };
        let bytes = size.map_err(failed)?;
        if png && let Some(limit) = ImageLimit::per_image(path, bytes) {
            return Err(limit.into());
        }
        self.budget.take(path, bytes).await?;
        if png {
            self.budget.retain(path, bytes)?;
        }
        Ok(())
    }
}

impl AssetReader for CacheReader {
    #[cfg(not(target_arch = "wasm32"))]
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        use std::io::{Read, Seek, SeekFrom};
        Self::check_path(path)?;
        let full = self.root.join(path);
        let mut file = match std::fs::File::open(&full) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(AssetReaderError::NotFound(full));
            }
            Err(err) => return Err(err.into()),
        };
        // No larger than a cache may hold a file of its kind: the loaders read it whole.
        let limit = dbota_import::pack::size_limit(&path.to_string_lossy());
        let too_big = || {
            let why = format!("{}: more than {limit} bytes", path.display());
            AssetReaderError::Io(std::io::Error::other(why).into())
        };
        if file.metadata()?.len() > limit {
            return Err(too_big());
        }
        let mut whole = || -> Result<Vec<u8>, AssetReaderError> {
            let mut bytes = Vec::new();
            (&mut file).take(limit + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > limit {
                return Err(too_big());
            }
            Ok(bytes)
        };
        if self.decodes(path) {
            let bytes = whole()?;
            let decoded = self.decoded(path, &bytes).await?;
            return Ok(desktop::Served::Memory(bevy::asset::io::VecReader::new(
                decoded,
            )));
        }
        if cache_path::is_model(path) {
            // Read whole, as the loader would: the file is the model's own bytes, vetted.
            let bytes = whole()?;
            Self::vet_model(path, &bytes)?;
            return Ok(desktop::Served::Memory(bevy::asset::io::VecReader::new(
                bytes,
            )));
        }
        let mut head = [0u8; HEAD_BYTES];
        let read = file.read(&mut head)?;
        file.seek(SeekFrom::Start(0))?;
        self.admit(path, &head[..read]).await?;
        Ok(desktop::Served::File(desktop::CacheFile(file)))
    }

    #[cfg(target_arch = "wasm32")]
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        Self::check_path(path)?;
        let bytes = if crate::webcache::is_private() {
            let rel = path.to_string_lossy();
            match crate::webcache::read(&rel).await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return Err(AssetReaderError::NotFound(path.to_path_buf())),
                Err(why) => return Err(AssetReaderError::Io(std::io::Error::other(why).into())),
            }
        } else if crate::webcache::is_served() {
            browser::fetch(&self.root.join(path)).await?
        } else {
            // No copy (the first-run screen): nothing is asked of the page's server.
            return Err(AssetReaderError::NotFound(path.to_path_buf()));
        };
        if self.decodes(path) {
            return Ok(browser::Bytes(self.decoded(path, &bytes).await?));
        }
        if cache_path::is_model(path) {
            Self::vet_model(path, &bytes)?;
        }
        self.admit(path, &bytes[..bytes.len().min(HEAD_BYTES)])
            .await?;
        Ok(browser::Bytes(bytes))
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        Self::check_path(path)?;
        self.files.read_meta(path).await
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        let _ = path;
        Err(AssetReaderError::Io(
            std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "cache directory listing is disabled; use the validated index",
            )
            .into(),
        ))
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        Self::check_path(path)?;
        self.files.is_directory(path).await
    }
}

/// Lets a frame come round before going on: in a browser everything runs
/// on the frame's own thread; on a desktop the frames run beside the loads.
pub async fn next_frame() {
    #[cfg(target_arch = "wasm32")]
    browser::next_frame().await;
}

/// Bytes a small cache file may hold; the largest, a ground texture, holds
/// 2.5 MB.
#[cfg(not(target_arch = "wasm32"))]
const MAX_SMALL: u64 = 16 << 20;

/// A small cache file read at once, where the client cannot wait for the
/// asset server: from disk, or in a browser with a synchronous request to
/// the page's server.
pub fn read_small(root: &Path, rel: &str) -> Option<Vec<u8>> {
    CacheReader::check_path(Path::new(rel)).ok()?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        let mut bytes = Vec::new();
        let file = std::fs::File::open(root.join(rel)).ok()?;
        file.take(MAX_SMALL + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > MAX_SMALL {
            warn!("{rel}: more than {MAX_SMALL} bytes; left unread");
            return None;
        }
        Some(bytes)
    }
    #[cfg(target_arch = "wasm32")]
    {
        if crate::webcache::is_served() {
            return browser::read(&root.join(rel));
        }
        crate::webcache::small(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interface_png(side: u32) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(side.to_be_bytes());
        png.extend(side.to_be_bytes());
        png.extend([8, 6, 0, 0, 0]);
        png
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn binary_model(json: &str) -> Vec<u8> {
        let json = format!("{json}{}", " ".repeat((4 - json.len() % 4) % 4));
        let mut file = b"glTF".to_vec();
        file.extend(2u32.to_le_bytes());
        file.extend((20 + json.len() as u32).to_le_bytes());
        file.extend((json.len() as u32).to_le_bytes());
        file.extend(0x4E4F_534Au32.to_le_bytes());
        file.extend(json.as_bytes());
        file
    }

    fn block_texture_header() -> Vec<u8> {
        let mut ktx2 = b"\xABKTX 20\xBB\r\n\x1A\n".to_vec();
        for word in [133u32, 1, 64, 64, 0, 0, 1, 1, 0] {
            ktx2.extend(word.to_le_bytes());
        }
        ktx2.resize(80, 0);
        for field in [104u64, 2048, 2048] {
            ktx2.extend(field.to_le_bytes());
        }
        ktx2
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn path_fixture(label: &str) -> (PathBuf, CacheReader) {
        let base =
            std::env::temp_dir().join(format!("dbota-cache-path-{label}-{}", std::process::id()));
        let root = base.join("cache");
        std::fs::create_dir_all(root.join("child")).unwrap();
        std::fs::create_dir_all(base.join("outside_dir")).unwrap();
        std::fs::write(root.join("inside.bin"), b"inside").unwrap();
        std::fs::write(root.join("inside.bin.meta"), b"inside metadata").unwrap();
        std::fs::write(base.join("outside.bin"), b"outside").unwrap();
        std::fs::write(base.join("outside.bin.meta"), b"outside metadata").unwrap();
        let reader = CacheReader::new(root, DecodeBudget::default(), BlockTextures::default());
        (base, reader)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cache_paths_stay_inside_the_reader_root() {
        let (base, reader) = path_fixture("read");
        let parsed = bevy::asset::AssetPath::parse("cache://../outside.bin");
        assert_eq!(parsed.path(), Path::new("../outside.bin"));
        assert!(bevy::tasks::block_on(reader.read(Path::new("inside.bin"))).is_ok());
        for path in [PathBuf::from("../outside.bin"), base.join("outside.bin")] {
            let why = bevy::tasks::block_on(reader.read(&path))
                .err()
                .expect("a cache request cannot open an owned marker outside the root")
                .to_string();
            assert!(why.contains("not a plain relative cache path"), "{why}");
        }
        std::fs::remove_dir_all(base).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cache_paths_stay_inside_the_metadata_root() {
        let (base, reader) = path_fixture("meta");
        assert!(bevy::tasks::block_on(reader.read_meta(Path::new("inside.bin"))).is_ok());
        for path in [PathBuf::from("../outside.bin"), base.join("outside.bin")] {
            let why = bevy::tasks::block_on(reader.read_meta(&path))
                .err()
                .expect("metadata cannot bypass the cache path boundary")
                .to_string();
            assert!(why.contains("not a plain relative cache path"), "{why}");
        }
        std::fs::remove_dir_all(base).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cache_paths_stay_inside_the_small_file_root() {
        let (base, _reader) = path_fixture("small");
        let root = base.join("cache");
        assert_eq!(read_small(&root, "inside.bin"), Some(b"inside".to_vec()));
        for path in [PathBuf::from("../outside.bin"), base.join("outside.bin")] {
            assert_eq!(
                read_small(&root, path.to_str().unwrap()),
                None,
                "a small read cannot escape its cache root"
            );
        }
        std::fs::remove_dir_all(base).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cache_paths_stay_inside_the_directory_probe_root() {
        let (base, reader) = path_fixture("directory");
        assert!(bevy::tasks::block_on(reader.is_directory(Path::new("child"))).unwrap());
        for path in [PathBuf::from("../outside_dir"), base.join("outside_dir")] {
            let why = bevy::tasks::block_on(reader.is_directory(&path))
                .expect_err("directory metadata cannot probe outside the cache root")
                .to_string();
            assert!(why.contains("not a plain relative cache path"), "{why}");
        }
        std::fs::remove_dir_all(base).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn resident_interface_images_are_limited_before_native_decoding() {
        let root = std::env::temp_dir().join(format!("dbota-resident-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let budget = DecodeBudget::default();
        let reader = CacheReader::new(root.clone(), budget.clone(), BlockTextures::default());
        let png = interface_png(2048);
        assert_eq!(decoded_size(&png), Some(Ok(16 << 20)));
        for index in 0..(Class::Interface.playback_bytes() / (16 << 20)) {
            let name = format!("{index}.png");
            std::fs::write(root.join(&name), &png).unwrap();
            assert!(bevy::tasks::block_on(reader.read(Path::new(&name))).is_ok());
            budget.settle(Path::new(&name));
        }
        std::fs::write(root.join("excess.png"), &png).unwrap();
        let why = bevy::tasks::block_on(reader.read(Path::new("excess.png")))
            .err()
            .expect("resident images must stay bounded after the GPU accepts them")
            .to_string();
        assert!(why.contains("resident interface image budget"), "{why}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resident_interface_images_are_limited_at_the_shared_browser_admission() {
        let budget = DecodeBudget::default();
        let reader = CacheReader::new(PathBuf::new(), budget.clone(), BlockTextures::default());
        let png = interface_png(2048);
        for index in 0..(Class::Interface.playback_bytes() / (16 << 20)) {
            let name = format!("{index}.png");
            assert!(bevy::tasks::block_on(reader.admit(Path::new(&name), &png)).is_ok());
            budget.settle(Path::new(&name));
        }
        let why = bevy::tasks::block_on(reader.admit(Path::new("excess.png"), &png))
            .expect_err("HTTP and private-storage reads share the resident image bound")
            .to_string();
        assert!(why.contains("resident interface image budget"), "{why}");
    }

    #[test]
    fn resident_interface_images_are_limited_with_mixed_case_png_extensions() {
        for (loaded_extension, excess_extension) in [("png", "PNG"), ("PNG", "png"), ("PnG", "pNg")]
        {
            for extension in [loaded_extension, excess_extension] {
                assert!(
                    matches!(
                        bevy::image::ImageFormat::from_extension(extension),
                        Some(bevy::image::ImageFormat::Png)
                    ),
                    "the budget must cover every spelling the decoder accepts"
                );
            }
            let budget = DecodeBudget::default();
            let reader = CacheReader::new(PathBuf::new(), budget.clone(), BlockTextures::default());
            let png = interface_png(2048);
            for index in 0..(Class::Interface.playback_bytes() / (16 << 20)) {
                let name = format!("{index}.{loaded_extension}");
                assert!(bevy::tasks::block_on(reader.admit(Path::new(&name), &png)).is_ok());
                budget.settle(Path::new(&name));
            }
            let excess = format!("excess.{excess_extension}");
            let why = bevy::tasks::block_on(reader.admit(Path::new(&excess), &png))
                .expect_err("extension case cannot bypass retained image admission")
                .to_string();
            assert!(why.contains("resident interface image budget"), "{why}");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cache_format_dispatch_vets_embedded_models_with_mixed_case_extensions() {
        let root = std::env::temp_dir().join(format!("dbota-model-case-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let reader = CacheReader::new(
            root.clone(),
            DecodeBudget::default(),
            BlockTextures::default(),
        );
        let json =
            r#"{"asset":{"version":"2.0"},"images":[{"bufferView":0,"mimeType":"image/png"}]}"#;
        for extension in ["gltf", "GLTF", "GlTf", "glb", "GLB", "GlB"] {
            let name = format!("embedded.{extension}");
            let bytes = if extension.eq_ignore_ascii_case("glb") {
                binary_model(json)
            } else {
                json.as_bytes().to_vec()
            };
            std::fs::write(root.join(&name), bytes).unwrap();
            let why = bevy::tasks::block_on(reader.read(Path::new(&name)))
                .err()
                .expect("extension case cannot bypass embedded-image validation")
                .to_string();
            assert!(why.contains("an image inside the model"), "{name}: {why}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cache_format_dispatch_decodes_ktx2_with_mixed_case_extensions() {
        let root = std::env::temp_dir().join(format!("dbota-ktx-case-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let block = BlockTextures::default();
        block.0.store(false, Ordering::Relaxed);
        let budget = DecodeBudget::default();
        let reader = CacheReader::new(root.clone(), budget.clone(), block);
        let ktx2 = block_texture_header();
        for extension in ["ktx2", "KTX2", "KtX2"] {
            assert!(matches!(
                bevy::image::ImageFormat::from_extension(extension),
                Some(bevy::image::ImageFormat::Ktx2)
            ));
            let name = format!("truncated.{extension}");
            std::fs::write(root.join(&name), &ktx2).unwrap();
            let why = bevy::tasks::block_on(reader.read(Path::new(&name)))
                .err()
                .expect("a GPU without BC support must attempt plain decoding")
                .to_string();
            assert!(why.contains("level 0 out of the file"), "{name}: {why}");
            budget.settle(Path::new(&name));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_complete_import_fits_the_image_budgets_independent_of_load_order() {
        let root = PathBuf::from(std::env::var_os("DBOTA_ASSETS").expect("an imported cache"));
        let index = dbota_import::index::CacheIndex::read(&root).expect("the imported index");
        let mut inventory = png::Inventory::default();
        let mut images = imported_pngs(&root, &index).peekable();
        assert!(
            images.peek().is_some(),
            "the imported index contains PNG images"
        );
        for (path, bytes) in images {
            inventory
                .record(path.to_str().unwrap(), bytes)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        }
    }

    fn image_app(root: &Path) -> App {
        use bevy::asset::AssetApp;
        use bevy::image::{CompressedImageFormats, ImageLoader, ImagePlugin};

        let mut app = App::new();
        let source = crate::cache_source(&mut app, root);
        app.register_asset_source(crate::CACHE_SOURCE, source)
            .add_plugins((
                MinimalPlugins,
                AssetPlugin::default(),
                ImagePlugin::default(),
            ))
            .register_asset_loader(ImageLoader::new(CompressedImageFormats::NONE));
        app
    }

    #[test]
    fn dropping_the_last_image_handle_refunds_its_reservation() {
        let file = crate::test_support::scratch_path("released_image.png");
        Image::default()
            .try_into_dynamic()
            .unwrap()
            .save(&file)
            .unwrap();
        let mut app = image_app(file.parent().unwrap());
        let server = app.world().resource::<AssetServer>().clone();
        let budget = app.world().resource::<Settler>().0.clone();
        let handle = server.load::<Image>(crate::manifest::cache_path("released_image.png"));
        let id = handle.id();
        update_until(&mut app, || server.is_loaded(id));
        let held = handle.clone();
        drop(handle);
        app.update();
        assert!(
            server.is_loaded(id),
            "the other handle still owns the image"
        );
        assert_eq!(budget.resident.lock().unwrap()[0].bytes, 4);
        drop(held);
        update_until(&mut app, || !server.is_managed(id));
        app.update();
        assert_eq!(
            budget.resident.lock().unwrap()[0].bytes,
            0,
            "the pool refunds the dropped image"
        );
    }

    #[test]
    fn an_oversized_png_draws_the_missing_image_and_warns_once_for_its_path() {
        let file = crate::test_support::scratch_path("oversized_cache_image.png");
        std::fs::write(&file, interface_png(4096)).expect("oversized PNG header");
        let name = file.file_name().unwrap().to_str().unwrap();
        let mut app = image_app(file.parent().unwrap());
        let path = crate::manifest::cache_path(name);
        let server = app.world().resource::<AssetServer>().clone();
        let handle = server.load::<Image>(path.clone());
        wait_for_image_failure(&mut app, &handle);
        let refusals = app.world().resource::<ImageRefusals>();
        assert_eq!(refusals.paths.len(), 1);
        let label = refusals.label.expect("an on-screen warning");
        let text = app.world().get::<Text>(label).expect("warning text");
        assert_eq!(text.0, "1 image could not be loaded");
        let assets = app.world().resource::<Assets<Image>>();
        let placeholder = assets
            .get(&handle)
            .expect("the failed handle has a drawable image");
        assert_eq!(placeholder.data, missing_image().data);
        server.reload(path);
        wait_for_image_failure(&mut app, &handle);
        assert_eq!(app.world().resource::<ImageRefusals>().paths.len(), 1);
        let root = app.world().get::<ChildOf>(label).unwrap().parent();
        let close = app.world().get::<Children>(root).unwrap()[1];
        app.world_mut()
            .trigger(bevy::ui_widgets::Activate { entity: close });
        app.update();
        assert!(
            app.world().get::<Text>(label).is_none(),
            "dismissal removes the notice"
        );
        server.reload(crate::manifest::cache_path(name));
        wait_for_image_failure(&mut app, &handle);
        assert!(
            app.world().resource::<ImageRefusals>().label.is_none(),
            "dismissal survives repeat refusals"
        );
    }

    #[test]
    fn oversized_world_art_gets_its_fallback_after_warning_tracking_fills() {
        let root = crate::test_support::scratch_path("world_fallback");
        let path = "textures/ground/oversized.png";
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, interface_png(4096)).unwrap();
        let mut app = image_app(&root);
        app.world_mut()
            .resource_mut::<ImageRefusals>()
            .paths
            .extend(
                (0..dbota_import::index::MAX_FILES)
                    .map(|index| PathBuf::from(format!("{index}.png"))),
            );
        let server = app.world().resource::<AssetServer>().clone();
        let handle = server.load::<Image>(crate::manifest::cache_path(path));
        wait_for_image_failure(&mut app, &handle);
        let assets = app.world().resource::<Assets<Image>>();
        let fallback = assets
            .get(&handle)
            .expect("the next refused world image still draws");
        assert_eq!(
            fallback.data,
            Image::default().data,
            "white preserves the baked colours"
        );
    }

    fn update_until(app: &mut App, mut ready: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !ready() {
            assert!(std::time::Instant::now() < deadline, "asset did not settle");
            app.update();
            std::thread::yield_now();
        }
    }

    fn wait_for_image_failure(app: &mut App, handle: &Handle<Image>) {
        let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut failed = bevy::ecs::message::MessageCursor::<
            bevy::asset::AssetLoadFailedEvent<Image>,
        >::default();
        loop {
            app.update();
            let events = app
                .world()
                .resource::<Messages<bevy::asset::AssetLoadFailedEvent<Image>>>();
            if failed.read(events).any(|event| event.id == handle.id()) {
                return;
            }
            assert!(
                std::time::Instant::now() < limit,
                "the image load failed to settle"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn resident_interface_images_are_limited_when_their_header_is_unreadable() {
        let reader = CacheReader::new(
            PathBuf::new(),
            DecodeBudget::default(),
            BlockTextures::default(),
        );
        let why = bevy::tasks::block_on(reader.admit(Path::new("broken.png"), b"not an image"))
            .expect_err("a missing image size cannot consume zero resident bytes")
            .to_string();
        assert!(why.contains("broken.png: no texture header"), "{why}");
    }

    #[test]
    fn resident_interface_images_are_limited_to_png_headers() {
        let reader = CacheReader::new(
            PathBuf::new(),
            DecodeBudget::default(),
            BlockTextures::default(),
        );
        let ktx2 = block_texture_header();
        assert_eq!(decoded_size(&ktx2), Some(Ok(2048)));
        let why = bevy::tasks::block_on(reader.admit(Path::new("wrong.png"), &ktx2))
            .expect_err("a recognized KTX2 header cannot stand for a PNG image")
            .to_string();
        assert!(why.contains("wrong.png: no texture header"), "{why}");
        assert!(bevy::tasks::block_on(reader.admit(Path::new("model.ktx2"), &ktx2)).is_ok());
    }

    #[test]
    fn resident_interface_images_are_limited_when_a_retained_path_grows() {
        let budget = DecodeBudget::default();
        let reader = CacheReader::new(PathBuf::new(), budget.clone(), BlockTextures::default());
        let png = interface_png(1024);
        for index in 0..(Class::Interface.playback_bytes() / (4 << 20)) {
            let name = format!("{index}.png");
            assert!(bevy::tasks::block_on(reader.admit(Path::new(&name), &png)).is_ok());
            budget.settle(Path::new(&name));
        }
        let retained = Path::new("0.png");
        assert!(bevy::tasks::block_on(reader.admit(retained, &png)).is_ok());
        budget.settle(retained);
        let why = bevy::tasks::block_on(reader.admit(retained, &interface_png(1152)))
            .expect_err("a larger replacement must reserve its extra decoded bytes")
            .to_string();
        assert!(why.contains("resident interface image budget"), "{why}");
    }

    #[test]
    fn resident_interface_images_do_not_refund_a_failed_shrinking_reload() {
        let budget = DecodeBudget::default();
        let reader = CacheReader::new(PathBuf::new(), budget.clone(), BlockTextures::default());
        let large = interface_png(1024);
        for index in 0..(Class::Interface.playback_bytes() / (4 << 20)) {
            let name = format!("{index}.png");
            assert!(bevy::tasks::block_on(reader.admit(Path::new(&name), &large)).is_ok());
            budget.settle(Path::new(&name));
        }
        let retained = Path::new("0.png");
        let corrupt_replacement = interface_png(1);
        assert!(bevy::tasks::block_on(reader.admit(retained, &corrupt_replacement)).is_ok());
        // Its header passes, but its absent pixel data cannot replace the old GPU image.
        // Failed loads settle transient decoding just as successful uploads do.
        budget.settle(retained);
        let why = bevy::tasks::block_on(reader.admit(Path::new("new.png"), &interface_png(1)))
            .expect_err("a failed shrinking reload leaves the old large image resident")
            .to_string();
        assert!(why.contains("resident interface image budget"), "{why}");
    }

    #[test]
    fn resident_interface_images_are_limited_when_many_tiny_paths_arrive() {
        let budget = DecodeBudget::default();
        let reader = CacheReader::new(PathBuf::new(), budget.clone(), BlockTextures::default());
        let png = interface_png(1);
        for index in 0..MAX_PATHS_PER_CLASS {
            let name = format!("{index}.png");
            assert!(bevy::tasks::block_on(reader.admit(Path::new(&name), &png)).is_ok());
            budget.settle(Path::new(&name));
        }
        let why = bevy::tasks::block_on(reader.admit(Path::new("excess.png"), &png))
            .expect_err("tiny files cannot grow retained path metadata without a bound")
            .to_string();
        assert!(why.contains("resident interface image budget"), "{why}");
    }

    #[test]
    fn resident_interface_images_are_limited_after_decode_stall_recovery() {
        let budget = DecodeBudget::default();
        let reader = CacheReader::new(PathBuf::new(), budget.clone(), BlockTextures::default());
        let png = interface_png(1024);
        let count = Class::Interface.playback_bytes() / (4 << 20);
        for index in 0..count {
            let name = format!("{index}.png");
            assert!(bevy::tasks::block_on(reader.admit(Path::new(&name), &png)).is_ok());
            if index + 1 < count {
                budget.settle(Path::new(&name));
            }
        }
        for _ in 0..STALL_FRAMES {
            budget.tick();
        }
        let why = bevy::tasks::block_on(reader.admit(Path::new("excess.png"), &png))
            .expect_err("decode progress recovery cannot renew resident allowance")
            .to_string();
        assert!(why.contains("resident interface image budget"), "{why}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cache_directories_cannot_be_listed_without_index_validation() {
        let root = std::env::temp_dir().join(format!("dbota-listing-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let reader = CacheReader::new(
            root.clone(),
            DecodeBudget::default(),
            BlockTextures::default(),
        );
        let error = bevy::tasks::block_on(reader.read_directory(Path::new("")))
            .err()
            .expect("raw directory listing must not bypass the index")
            .to_string();
        assert!(
            error.contains("cache directory listing is disabled"),
            "{error}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The small files are read whole: a huge one planted in the cache
    /// filled the memory.
    #[test]
    fn a_small_file_past_its_cap_is_left_unread() {
        let root = std::env::temp_dir().join(format!("dbota-small-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let file = std::fs::File::create(root.join("huge.json")).unwrap();
        // Sparse: nothing is written.
        file.set_len(MAX_SMALL + 1).unwrap();
        std::fs::write(root.join("small.json"), b"{}").unwrap();
        assert_eq!(read_small(&root, "huge.json"), None);
        assert_eq!(read_small(&root, "small.json"), Some(b"{}".to_vec()));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A model with an image inside it is refused by the reader the asset server loads the cache
    /// through, before Bevy decodes the image (with no limits); a model with its image next to
    /// it is served.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_model_with_an_embedded_image_is_not_served() {
        let root = std::env::temp_dir().join(format!("dbota-model-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let embedded = r#"{"images":[{"bufferView":0,"mimeType":"image/png"}]}"#;
        let beside = r#"{"images":[{"uri":"head.ktx2"}]}"#;
        std::fs::write(root.join("embedded.glb"), binary_model(embedded)).unwrap();
        std::fs::write(root.join("beside.glb"), binary_model(beside)).unwrap();
        let reader = CacheReader::new(
            root.clone(),
            DecodeBudget::default(),
            BlockTextures::default(),
        );

        let refused = bevy::tasks::block_on(reader.read(Path::new("embedded.glb")));
        let served = bevy::tasks::block_on(reader.read(Path::new("beside.glb")));

        let why = refused.err().expect("refused").to_string();
        assert!(
            why.contains("embedded.glb: an image inside the model"),
            "{why}"
        );
        assert!(served.is_ok());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A texture is served open or read whole by the size its file has, which the header that
    /// is checked says nothing of: a file larger than any of its kind a cache may hold is not
    /// served, even with a header within the caps.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_texture_file_past_the_size_of_its_kind_is_not_served() {
        let root = std::env::temp_dir().join(format!("dbota-big-texture-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(16u32.to_be_bytes());
        png.extend(16u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0]);
        std::fs::write(root.join("small.png"), &png).unwrap();
        std::fs::write(root.join("huge.png"), &png).unwrap();
        // Sparse: nothing is written past the header.
        std::fs::File::options()
            .write(true)
            .open(root.join("huge.png"))
            .unwrap()
            .set_len(64 << 30)
            .unwrap();
        let reader = CacheReader::new(
            root.clone(),
            DecodeBudget::default(),
            BlockTextures::default(),
        );

        let refused = bevy::tasks::block_on(reader.read(Path::new("huge.png")));
        let served = bevy::tasks::block_on(reader.read(Path::new("small.png")));

        let why = refused.err().expect("refused").to_string();
        assert!(why.contains("huge.png: more than"), "{why}");
        assert!(served.is_ok());
        std::fs::remove_dir_all(&root).unwrap();
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod desktop {
    use std::{
        fs::File,
        io::{Read, Seek, SeekFrom},
        pin::Pin,
        task::{Context, Poll},
    };

    use bevy::{
        asset::io::{
            Reader, ReaderNotSeekableError, STACK_FUTURE_SIZE, SeekableReader, StackFuture,
        },
        tasks::futures_lite::io::{AsyncRead, AsyncSeek},
    };

    /// A cache file as the reader serves it: open on the disk, or decoded
    /// in memory.
    pub enum Served {
        File(CacheFile),
        Memory(bevy::asset::io::VecReader),
    }

    impl AsyncRead for Served {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
            match self.get_mut() {
                Served::File(file) => Pin::new(file).poll_read(cx, buf),
                Served::Memory(bytes) => Pin::new(bytes).poll_read(cx, buf),
            }
        }
    }

    impl Reader for Served {
        fn read_to_end<'a>(
            &'a mut self,
            buf: &'a mut Vec<u8>,
        ) -> StackFuture<'a, std::io::Result<usize>, STACK_FUTURE_SIZE> {
            match self {
                Served::File(file) => file.read_to_end(buf),
                Served::Memory(bytes) => bytes.read_to_end(buf),
            }
        }

        fn seekable(&mut self) -> Result<&mut dyn SeekableReader, ReaderNotSeekableError> {
            match self {
                Served::File(file) => file.seekable(),
                Served::Memory(bytes) => bytes.seekable(),
            }
        }
    }

    /// An open cache file, read synchronously: the IO pool's threads are
    /// there to wait on the disk.
    pub struct CacheFile(pub File);

    impl AsyncRead for CacheFile {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(self.get_mut().0.read(buf))
        }
    }

    impl AsyncSeek for CacheFile {
        fn poll_seek(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            pos: SeekFrom,
        ) -> Poll<std::io::Result<u64>> {
            Poll::Ready(self.get_mut().0.seek(pos))
        }
    }

    impl Reader for CacheFile {
        fn read_to_end<'a>(
            &'a mut self,
            buf: &'a mut Vec<u8>,
        ) -> StackFuture<'a, std::io::Result<usize>, STACK_FUTURE_SIZE> {
            // One allocation of the file's size, filled by the kernel directly.
            let size = self.0.metadata().map_or(0, |m| m.len() as usize);
            buf.reserve_exact(size);
            StackFuture::from(async { self.0.read_to_end(buf) })
        }

        fn seekable(&mut self) -> Result<&mut dyn SeekableReader, ReaderNotSeekableError> {
            Ok(self)
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::{
        path::Path,
        pin::Pin,
        task::{Context, Poll},
    };

    use bevy::{
        asset::io::{
            AssetReaderError, Reader, ReaderNotSeekableError, STACK_FUTURE_SIZE, SeekableReader,
            StackFuture,
        },
        tasks::futures_lite::io::AsyncRead,
    };
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    /// A file next to the page, fetched whole into the client's memory once.
    pub async fn fetch(path: &Path) -> Result<Vec<u8>, AssetReaderError> {
        let failed = |e: wasm_bindgen::JsValue| {
            AssetReaderError::Io(std::io::Error::other(format!("fetch: {e:?}")).into())
        };
        let window = web_sys::window().ok_or_else(|| failed("no window".into()))?;
        let response: web_sys::Response =
            JsFuture::from(window.fetch_with_str(&path.to_string_lossy()))
                .await
                .map_err(failed)?
                .dyn_into()
                .map_err(failed)?;
        match response.status() {
            200 => {
                let body = JsFuture::from(response.array_buffer().map_err(failed)?)
                    .await
                    .map_err(failed)?;
                Ok(js_sys::Uint8Array::new(&body).to_vec())
            }
            403 | 404 => Err(AssetReaderError::NotFound(path.to_path_buf())),
            status => Err(AssetReaderError::HttpError(status)),
        }
    }

    /// Resolves at the browser's next animation frame.
    pub async fn next_frame() {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(window) = web_sys::window() {
                let _ = window.request_animation_frame(&resolve);
            }
        });
        let _ = JsFuture::from(promise).await;
    }

    /// A fetched file, handed whole to a loader that reads it to the end:
    /// moved, not copied, for the map's geometry is a fifth of a gigabyte
    /// and a browser's memory never shrinks back.
    pub struct Bytes(pub Vec<u8>);

    impl AsyncRead for Bytes {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
            let this = self.get_mut();
            let n = buf.len().min(this.0.len());
            buf[..n].copy_from_slice(&this.0[..n]);
            this.0.drain(..n);
            Poll::Ready(Ok(n))
        }
    }

    impl Reader for Bytes {
        fn read_to_end<'a>(
            &'a mut self,
            buf: &'a mut Vec<u8>,
        ) -> StackFuture<'a, std::io::Result<usize>, STACK_FUTURE_SIZE> {
            StackFuture::from(async move {
                let n = self.0.len();
                if buf.is_empty() {
                    std::mem::swap(buf, &mut self.0);
                } else {
                    buf.append(&mut self.0);
                }
                Ok(n)
            })
        }

        fn seekable(&mut self) -> Result<&mut dyn SeekableReader, ReaderNotSeekableError> {
            Err(ReaderNotSeekableError)
        }
    }

    /// A file next to the page, fetched synchronously: its bytes come back
    /// one per character of a text read as `x-user-defined`.
    pub fn read(path: &Path) -> Option<Vec<u8>> {
        let request = web_sys::XmlHttpRequest::new().ok()?;
        request
            .open_with_async("GET", &path.to_string_lossy(), false)
            .ok()?;
        request
            .override_mime_type("text/plain; charset=x-user-defined")
            .ok()?;
        request.send().ok()?;
        if request.status().ok()? != 200 {
            return None;
        }
        let text = request.response_text().ok()??;
        Some(text.chars().map(|c| (c as u32 & 0xff) as u8).collect())
    }
}

#[cfg(test)]
fn imported_pngs<'a>(
    root: &'a Path,
    index: &'a dbota_import::index::CacheIndex,
) -> impl Iterator<Item = (&'a Path, usize)> {
    use std::io::Read;

    index.files().iter().filter_map(|file| {
        let path = Path::new(&file.path);
        if cache_path::image_format(path) != Some(cache_path::ImageFormat::Png) {
            return None;
        }
        let mut header = [0; 26];
        std::fs::File::open(root.join(path))
            .and_then(|mut file| file.read_exact(&mut header))
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let size = crate::texture_file::png_size(&header)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        Some((path, size))
    })
}

#[cfg(test)]
pub(crate) fn image_budget_refusals(world: &World) -> usize {
    world.resource::<ImageRefusals>().paths.len()
}
