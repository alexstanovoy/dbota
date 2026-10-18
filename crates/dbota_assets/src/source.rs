use std::collections::HashMap;
use std::io;

use crate::error::{Error, Result};

/// Random-access reads of the files of a Dota install, by path relative to its `game/dota`
/// folder with `/` separators (`pak01_dir.vpk`, `pak01_042.vpk`, `maps/dota.vpk`).
///
/// Reads are synchronous: the browser backs this with `File.slice` read through `FileReaderSync`,
/// which a Web Worker (where the pipeline runs) may call.
pub trait Source {
    /// The size in bytes of the file at `path`.
    fn size(&self, path: &str) -> Result<u64>;

    /// Exactly `len` bytes at `offset`; a range past the end of the file is an error, so a
    /// malformed index never makes a read allocate more than the file holds.
    fn read_at(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>>;
}

impl<S: Source + ?Sized> Source for &S {
    fn size(&self, path: &str) -> Result<u64> {
        (**self).size(path)
    }

    fn read_at(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        (**self).read_at(path, offset, len)
    }
}

/// One random-access file: a buffer in memory, or a browser `File` read by slices.
pub trait Slice {
    fn size(&self) -> u64;

    /// Exactly `len` bytes at `offset`; the caller has checked the range against `size`.
    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>>;
}

impl Slice for Vec<u8> {
    fn size(&self) -> u64 {
        self.len() as u64
    }

    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        let start = usize::try_from(offset).map_err(io::Error::other)?;
        start
            .checked_add(len)
            .and_then(|end| self.get(start..end))
            .map(<[u8]>::to_vec)
            .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))
    }
}

/// A [`Source`] made of named [`Slice`]s: in-memory files, or the browser's picked files.
pub struct SliceSource<F> {
    files: HashMap<String, F>,
}

impl<F> Default for SliceSource<F> {
    fn default() -> Self {
        Self {
            files: HashMap::new(),
        }
    }
}

impl<F: Slice> SliceSource<F> {
    pub fn insert(&mut self, path: impl Into<String>, file: F) {
        self.files.insert(path.into(), file);
    }

    fn file(&self, path: &str) -> Result<&F> {
        self.files
            .get(path)
            .ok_or_else(|| Error::NotFound(path.to_owned()))
    }
}

impl<F: Slice> Source for SliceSource<F> {
    fn size(&self, path: &str) -> Result<u64> {
        self.file(path).map(Slice::size)
    }

    fn read_at(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let file = self.file(path)?;
        check_range(path, offset, len, file.size())?;
        file.read_at(offset, len).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })
    }
}

pub(crate) fn check_range(path: &str, offset: u64, len: usize, size: u64) -> Result<()> {
    let fits = offset
        .checked_add(len as u64)
        .is_some_and(|end| end <= size);
    if fits {
        Ok(())
    } else {
        Err(Error::OutOfRange {
            path: path.to_owned(),
            offset,
            len,
            size,
        })
    }
}

#[cfg(any(unix, windows))]
pub use native::DirSource;

#[cfg(any(unix, windows))]
mod native {
    use std::fs::File;
    use std::io;
    use std::path::{Component, Path, PathBuf};

    use super::{Source, check_range};
    use crate::error::{Error, Result};

    /// The files under a folder on disk (a Dota install's `game/dota`).
    pub struct DirSource {
        root: PathBuf,
    }

    impl DirSource {
        pub fn new(root: impl Into<PathBuf>) -> Self {
            Self { root: root.into() }
        }

        /// Opens `path` under the root; absolute paths and `..` never leave it.
        fn open(&self, path: &str) -> Result<File> {
            let relative = Path::new(path);
            if !relative
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
            {
                return Err(Error::NotFound(path.to_owned()));
            }
            File::open(self.root.join(relative)).map_err(|source| io_error(path, source))
        }
    }

    fn io_error(path: &str, source: io::Error) -> Error {
        if source.kind() == io::ErrorKind::NotFound {
            Error::NotFound(path.to_owned())
        } else {
            Error::Io {
                path: path.to_owned(),
                source,
            }
        }
    }

    impl Source for DirSource {
        fn size(&self, path: &str) -> Result<u64> {
            let file = self.open(path)?;
            file.metadata()
                .map(|m| m.len())
                .map_err(|source| io_error(path, source))
        }

        fn read_at(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
            let file = self.open(path)?;
            let size = file
                .metadata()
                .map_err(|source| io_error(path, source))?
                .len();
            check_range(path, offset, len, size)?;
            let mut buf = vec![0; len];
            read_exact_at(&file, &mut buf, offset).map_err(|source| io_error(path, source))?;
            Ok(buf)
        }
    }

    #[cfg(unix)]
    fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
        std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
    }

    #[cfg(windows)]
    fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            match file.seek_read(buf, offset)? {
                0 => return Err(io::ErrorKind::UnexpectedEof.into()),
                n => {
                    buf = &mut buf[n..];
                    offset += n as u64;
                }
            }
        }
        Ok(())
    }
}
