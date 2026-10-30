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

use std::{
    collections::HashMap,
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

use crate::gltf_file;
use crate::texture_file::{HEAD_BYTES, decoded_size};

/// Decoded texture bytes let through at once, besides the one under way:
/// less in a browser, whose memory never shrinks back.
const DECODE_BUDGET: usize = if cfg!(target_arch = "wasm32") {
    64 << 20
} else {
    192 << 20
};
/// Longest model file read (the importer writes the map's glTF, 15 MB, as the largest).
#[cfg(not(target_arch = "wasm32"))]
const MAX_MODEL: u64 = 64 << 20;
/// Frames without a texture arriving after which the budget lets every
/// texture through: a load that never reports back must not stall the rest.
const STALL_FRAMES: u32 = 120;

/// The textures read and not yet handed to the GPU, by cache path, with
/// their decoded size, and the reads waiting for room.
#[derive(Default)]
struct InFlight {
    bytes: usize,
    textures: HashMap<PathBuf, usize>,
    waiting: Vec<Waker>,
    /// Frames since a texture last arrived while some were in flight.
    idle: u32,
}

/// The decode budget shared by the reader and the systems that settle it.
#[derive(Resource, Clone, Default)]
pub struct DecodeBudget(Arc<Mutex<InFlight>>);

impl DecodeBudget {
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
        if let Ok(mut flight) = self.0.lock()
            && let Some(bytes) = flight.textures.remove(path)
        {
            flight.bytes -= bytes;
            flight.idle = 0;
            Self::wake(&mut flight);
        }
    }

    /// The textures read and not yet settled.
    fn in_flight(&self) -> Vec<PathBuf> {
        self.0
            .lock()
            .map(|flight| flight.textures.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Counts a frame; after [`STALL_FRAMES`] of nothing arriving, forgets
    /// what is in flight.
    fn tick(&self) {
        let Ok(mut flight) = self.0.lock() else {
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

/// A read waiting for room in the decode budget.
struct Take<'a> {
    budget: &'a DecodeBudget,
    path: &'a Path,
    bytes: usize,
}

impl Future for Take<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let Ok(mut flight) = self.budget.0.lock() else {
            return Poll::Ready(());
        };
        if flight.bytes > 0 && flight.bytes + self.bytes > DECODE_BUDGET {
            flight.waiting.push(cx.waker().clone());
            return Poll::Pending;
        }
        flight.bytes += self.bytes;
        if let Some(old) = flight.textures.insert(self.path.to_path_buf(), self.bytes) {
            flight.bytes -= old;
        }
        Poll::Ready(())
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
        .add_systems(PreStartup, note_block_textures)
        .add_systems(PostUpdate, arrived.after(bevy::asset::AssetEventSystems))
        .add_systems(First, settle);
}

/// The textures that arrived this frame, to let go at the start of the next.
#[derive(Resource, Default)]
struct DueTextures(Vec<PathBuf>);

fn arrived(
    mut added: MessageReader<AssetEvent<Image>>,
    mut failed: MessageReader<bevy::asset::AssetLoadFailedEvent<Image>>,
    server: Res<AssetServer>,
    budget: Res<DecodeBudget>,
    mut due: ResMut<DueTextures>,
) {
    for event in added.read() {
        if let AssetEvent::Added { id } = event
            && let Some(path) = server.get_path(*id)
        {
            due.0.push(path.path().to_path_buf());
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
        }
    }
}

fn settle(mut due: ResMut<DueTextures>, budget: Res<DecodeBudget>) {
    for path in due.0.drain(..) {
        budget.settle(&path);
    }
    budget.tick();
}

/// Reads the cache folder's files; directories and meta files as Bevy does.
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
    /// Whether the file at `path` is handed on decoded: a KTX2 texture when
    /// the GPU samples no block format.
    fn decodes(&self, path: &Path) -> bool {
        !self.block.sampled() && path.extension().is_some_and(|e| e == "ktx2")
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

    /// Whether the file at `path` is a model, which is read whole and looked into.
    fn is_model(path: &Path) -> bool {
        path.extension().is_some_and(|e| e == "glb" || e == "gltf")
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
        match decoded_size(head) {
            Some(Ok(bytes)) => self.budget.take(path, bytes).await,
            Some(Err(why)) => {
                let why = format!("{}: {why}", path.display());
                return Err(AssetReaderError::Io(std::io::Error::other(why).into()));
            }
            None => {}
        }
        Ok(())
    }
}

impl AssetReader for CacheReader {
    #[cfg(not(target_arch = "wasm32"))]
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        use std::io::{Read, Seek, SeekFrom};
        let full = self.root.join(path);
        let mut file = match std::fs::File::open(&full) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(AssetReaderError::NotFound(full));
            }
            Err(err) => return Err(err.into()),
        };
        if self.decodes(path) {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            let decoded = self.decoded(path, &bytes).await?;
            return Ok(desktop::Served::Memory(bevy::asset::io::VecReader::new(
                decoded,
            )));
        }
        if Self::is_model(path) {
            // Read whole, as the loader would: the file is the model's own bytes, vetted.
            let mut bytes = Vec::new();
            (&mut file).take(MAX_MODEL + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_MODEL {
                return Err(AssetReaderError::Io(
                    std::io::Error::other(format!(
                        "{}: more than {MAX_MODEL} bytes",
                        path.display()
                    ))
                    .into(),
                ));
            }
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
        let bytes = if crate::webcache::is_opfs() {
            let rel = path.to_string_lossy();
            match crate::webcache::read(&rel).await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return Err(AssetReaderError::NotFound(path.to_path_buf())),
                Err(why) => return Err(AssetReaderError::Io(std::io::Error::other(why).into())),
            }
        } else {
            browser::fetch(&self.root.join(path)).await?
        };
        if self.decodes(path) {
            return Ok(browser::Bytes(self.decoded(path, &bytes).await?));
        }
        if Self::is_model(path) {
            Self::vet_model(path, &bytes)?;
        }
        self.admit(path, &bytes[..bytes.len().min(HEAD_BYTES)])
            .await?;
        Ok(browser::Bytes(bytes))
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.files.read_meta(path).await
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        self.files.read_directory(path).await
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
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
        if crate::webcache::is_opfs() {
            return crate::webcache::small(rel);
        }
        browser::read(&root.join(rel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let model = |json: &str| {
            let mut file = b"glTF".to_vec();
            file.extend(2u32.to_le_bytes());
            file.extend((20 + json.len() as u32).to_le_bytes());
            file.extend((json.len() as u32).to_le_bytes());
            file.extend(0x4E4F_534Au32.to_le_bytes());
            file.extend(json.as_bytes());
            file
        };
        let padded = |json: &str| format!("{json}{}", " ".repeat((4 - json.len() % 4) % 4));
        let embedded = padded(r#"{"images":[{"bufferView":0,"mimeType":"image/png"}]}"#);
        let beside = padded(r#"{"images":[{"uri":"head.ktx2"}]}"#);
        std::fs::write(root.join("embedded.glb"), model(&embedded)).unwrap();
        std::fs::write(root.join("beside.glb"), model(&beside)).unwrap();
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
