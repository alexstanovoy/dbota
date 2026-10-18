//! The asset cache as the client reads it. On a desktop a file is read
//! whole into the loader's own buffer, on the IO thread that asked for it:
//! Bevy's default file reader passes every file through an 8 MiB pipe of
//! its own, and with dozens of textures loading at once those pipes held
//! over half a gigabyte. In a browser the files come over HTTP, as Bevy
//! fetches them.
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
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use bevy::{
    asset::io::{AssetReader, AssetReaderError, PathStream, Reader},
    prelude::*,
};

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
    /// Resolves once a texture of `bytes` decoded fits, and counts it.
    fn take<'a>(&'a self, path: &'a Path, bytes: usize) -> Take<'a> {
        Take {
            budget: self,
            path,
            bytes,
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

/// Bytes of a texture file's start that [`decoded_size`] needs: a KTX2
/// header and the index of its most levels (a texture has at most 32).
#[cfg(not(target_arch = "wasm32"))]
const HEAD_BYTES: usize = KTX2_INDEX + 24 * 32;
const KTX2_INDEX: usize = 80;
const KTX2_MAGIC: [u8; 12] = [
    0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB, b'\r', b'\n', 0x1A, b'\n',
];

/// The bytes a texture file holds once loaded, from its first
/// [`HEAD_BYTES`]: a PNG decodes to RGBA, a KTX2 file to all its mip levels
/// as they sit in memory (compressed blocks stay compressed, a zstd
/// supercompression is undone). None for anything else.
fn decoded_size(head: &[u8]) -> Option<usize> {
    png_size(head).or_else(|| ktx2_size(head))
}

fn png_size(head: &[u8]) -> Option<usize> {
    if head.len() < 24 || head[..8] != *b"\x89PNG\r\n\x1a\n" || head[12..16] != *b"IHDR" {
        return None;
    }
    let side = |at: usize| u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    Some(side(16) as usize * side(20) as usize * 4)
}

/// The sum of the level index's uncompressed sizes. A file that claims more
/// levels than a texture has, or whose index is cut off, is not counted.
fn ktx2_size(head: &[u8]) -> Option<usize> {
    if head.len() < KTX2_INDEX || head[..12] != KTX2_MAGIC {
        return None;
    }
    let word = |at: usize| u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    // Zero levels ask the loader to make the chain: the one level is there.
    let levels = (word(40) as usize).max(1);
    if levels > 32 {
        return None;
    }
    let index = head.get(KTX2_INDEX..KTX2_INDEX + 24 * levels)?;
    let mut total = 0usize;
    for level in index.as_chunks::<24>().0 {
        let size = u64::from_le_bytes(level[16..24].try_into().ok()?);
        total = total.checked_add(usize::try_from(size).ok()?)?;
    }
    Some(total)
}

/// Settles the decode budget: a texture added this frame reaches the GPU
/// when the frame ends, and is let go at the start of the next.
pub fn plugin(app: &mut App) {
    app.init_resource::<DueTextures>()
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
    root: PathBuf,
    #[cfg(not(target_arch = "wasm32"))]
    files: bevy::asset::io::file::FileAssetReader,
    #[cfg(target_arch = "wasm32")]
    files: bevy::asset::io::wasm::HttpWasmAssetReader,
}

impl CacheReader {
    pub fn new(root: PathBuf, budget: DecodeBudget) -> CacheReader {
        CacheReader {
            budget,
            #[cfg(not(target_arch = "wasm32"))]
            files: bevy::asset::io::file::FileAssetReader::new(&root),
            #[cfg(target_arch = "wasm32")]
            files: bevy::asset::io::wasm::HttpWasmAssetReader::new(&root),
            root,
        }
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
        let mut head = [0u8; HEAD_BYTES];
        let read = file.read(&mut head)?;
        file.seek(SeekFrom::Start(0))?;
        if let Some(bytes) = decoded_size(&head[..read]) {
            self.budget.take(path, bytes).await;
        }
        Ok(desktop::CacheFile(file))
    }

    #[cfg(target_arch = "wasm32")]
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let bytes = browser::fetch(&self.root.join(path)).await?;
        if let Some(size) = decoded_size(&bytes) {
            self.budget.take(path, size).await;
        }
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

/// A small cache file read at once, where the client cannot wait for the
/// asset server: from disk, or in a browser with a synchronous request to
/// the page's server.
pub fn read_small(root: &Path, rel: &str) -> Option<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::read(root.join(rel)).ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        browser::read(&root.join(rel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A KTX2 start: the magic, `levels` in the header, and an index whose level `i` holds
    /// `sizes[i]` bytes uncompressed (and half that on disk).
    fn ktx2(levels: u32, sizes: &[u64]) -> Vec<u8> {
        let mut head = KTX2_MAGIC.to_vec();
        head.resize(40, 0);
        head.extend_from_slice(&levels.to_le_bytes());
        head.resize(KTX2_INDEX, 0);
        for &size in sizes {
            for field in [0, size / 2, size] {
                head.extend_from_slice(&u64::to_le_bytes(field));
            }
        }
        head
    }

    #[test]
    fn a_ktx2_file_counts_all_its_levels_uncompressed() {
        assert_eq!(decoded_size(&ktx2(3, &[64, 16, 4])), Some(84));
        assert_eq!(decoded_size(&ktx2(0, &[64])), Some(64));
    }

    #[test]
    fn a_ktx2_file_with_a_cut_off_or_absurd_index_is_not_counted() {
        assert_eq!(decoded_size(&ktx2(3, &[64, 16])), None);
        assert_eq!(decoded_size(&ktx2(33, &[64; 33])), None);
        assert_eq!(decoded_size(&ktx2(2, &[u64::MAX, 1])), None);
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
