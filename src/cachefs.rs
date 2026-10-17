//! The asset cache read straight from disk: a file is read whole into the
//! loader's own buffer, on the IO thread that asked for it. Bevy's default
//! file reader passes every file through an 8 MiB pipe of its own: with
//! dozens of textures loading at once those pipes held over half a gigabyte.
//!
//! Textures are also let through only while the ones decoded before them
//! have reached the GPU, a budget of decoded bytes at a time: the IO threads
//! decode far faster than a frame takes them in while the map is prepared,
//! and every texture of the cache sat decoded in memory at once.

use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Condvar, Mutex},
    task::{Context, Poll},
    time::Duration,
};

use bevy::{
    asset::io::{
        AssetReader, AssetReaderError, PathStream, Reader, ReaderNotSeekableError,
        STACK_FUTURE_SIZE, SeekableReader, StackFuture, file::FileAssetReader,
    },
    prelude::*,
    tasks::futures_lite::io::{AsyncRead, AsyncSeek},
};

/// Decoded texture bytes let through at once, besides the one under way.
const DECODE_BUDGET: usize = 192 << 20;
/// How long a texture waits for the budget at most: a texture whose load
/// never reports back must not hold the others up for good.
const DECODE_WAIT: Duration = Duration::from_secs(2);

/// The textures read and not yet handed to the GPU, by cache path, with
/// their decoded size.
#[derive(Default)]
struct InFlight {
    bytes: usize,
    textures: HashMap<PathBuf, usize>,
}

/// The decode budget shared by the reader and the systems that settle it.
#[derive(Resource, Clone, Default)]
pub struct DecodeBudget(Arc<(Mutex<InFlight>, Condvar)>);

impl DecodeBudget {
    /// Waits until a texture of `bytes` decoded fits, then counts it.
    fn take(&self, path: &Path, bytes: usize) {
        let (lock, settled) = &*self.0;
        let Ok(mut flight) = lock.lock() else { return };
        let mut waited = Duration::ZERO;
        while flight.bytes > 0 && flight.bytes + bytes > DECODE_BUDGET && waited < DECODE_WAIT {
            let step = Duration::from_millis(50);
            match settled.wait_timeout(flight, step) {
                Ok((next, _)) => flight = next,
                Err(_) => return,
            }
            waited += step;
        }
        flight.bytes += bytes;
        if let Some(old) = flight.textures.insert(path.to_path_buf(), bytes) {
            flight.bytes -= old;
        }
    }

    /// Lets a texture's bytes go once it is on the GPU or failed.
    fn settle(&self, path: &Path) {
        let (lock, settled) = &*self.0;
        if let Ok(mut flight) = lock.lock()
            && let Some(bytes) = flight.textures.remove(path)
        {
            flight.bytes -= bytes;
            settled.notify_all();
        }
    }
}

/// Reads the cache folder's files; directories and meta files as Bevy does.
pub struct CacheReader {
    root: PathBuf,
    files: FileAssetReader,
    budget: DecodeBudget,
}

impl CacheReader {
    pub fn new(root: PathBuf, budget: DecodeBudget) -> CacheReader {
        CacheReader {
            files: FileAssetReader::new(&root),
            root,
            budget,
        }
    }
}

/// The bytes a PNG decodes to as RGBA, from its header; none for anything else.
fn decoded_size(file: &mut File) -> Option<usize> {
    let mut head = [0u8; 24];
    file.read_exact(&mut head).ok()?;
    file.seek(SeekFrom::Start(0)).ok()?;
    if head[..8] != *b"\x89PNG\r\n\x1a\n" || head[12..16] != *b"IHDR" {
        return None;
    }
    let side = |at: usize| u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    Some(side(16) as usize * side(20) as usize * 4)
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
}

impl AssetReader for CacheReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let full = self.root.join(path);
        match File::open(&full) {
            Ok(mut file) => {
                if let Some(bytes) = decoded_size(&mut file) {
                    self.budget.take(path, bytes);
                }
                Ok(CacheFile(file))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Err(AssetReaderError::NotFound(full))
            }
            Err(err) => Err(err.into()),
        }
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

/// An open cache file, read synchronously: the IO pool's threads are there
/// to wait on the disk.
struct CacheFile(File);

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
