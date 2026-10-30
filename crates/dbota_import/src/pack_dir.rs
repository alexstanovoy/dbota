//! Prepared caches on the desktop: a folder or a zip read, checked and installed in place of the
//! cache (made beside it and swapped in when whole, like an import), and a cache written as a zip
//! for someone else.

use std::fs;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use heatel::{DirSource, Slice, Source};

use crate::cache_dir::{DirSink, remove, replace, sibling};
use crate::import::{INDEX, STAMP, Sink};
use crate::pack::{Container, Entry, MAX_FILES, Pack, PackError, shown};
use crate::zip::{ZipContainer, ZipWriter};

/// Deepest folder looked into: a cache's is four below its top.
const MAX_DEPTH: usize = 8;

/// What an install or an export moved.
#[derive(Debug, PartialEq, Eq)]
pub struct Summary {
    pub files: usize,
    pub bytes: u64,
    pub stamp: String,
}

/// A cache folder on disk. Links are not followed (a link in a cache is refused), and what is
/// not a plain file is refused.
pub struct DirContainer {
    root: PathBuf,
    entries: Vec<Entry>,
}

impl DirContainer {
    /// Lists the files below `root`; none is read yet.
    pub fn open(root: &Path) -> Result<Self, PackError> {
        let io = |why: io::Error, path: &Path| {
            PackError::Io(format!("cannot read {}: {why}", path.display()))
        };
        let meta = fs::symlink_metadata(root).map_err(|e| io(e, root))?;
        if !meta.is_dir() {
            return Err(PackError::Invalid(format!(
                "{} is not a folder",
                root.display()
            )));
        }
        let mut entries = Vec::new();
        let mut pending = vec![(root.to_path_buf(), String::new(), 0)];
        while let Some((dir, prefix, depth)) = pending.pop() {
            for item in fs::read_dir(&dir).map_err(|e| io(e, &dir))? {
                let item = item.map_err(|e| io(e, &dir))?;
                let name = item.file_name().to_string_lossy().into_owned();
                let path = format!("{prefix}{name}");
                let kind = item.file_type().map_err(|e| io(e, &item.path()))?;
                if kind.is_dir() && depth < MAX_DEPTH {
                    pending.push((item.path(), format!("{path}/"), depth + 1));
                } else if kind.is_file() {
                    let size = item.metadata().map_err(|e| io(e, &item.path()))?.len();
                    entries.push(Entry { path, size });
                } else {
                    return Err(PackError::Unsafe(shown(&path)));
                }
                if entries.len() > MAX_FILES + 2 {
                    return Err(PackError::TooBig(format!(
                        "it has more than {} files, a cache has at most {MAX_FILES}",
                        MAX_FILES + 2
                    )));
                }
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self {
            root: root.to_path_buf(),
            entries,
        })
    }
}

impl Container for DirContainer {
    fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn read(&self, at: usize) -> Result<Vec<u8>, PackError> {
        let entry = &self.entries[at];
        let fail =
            |why: io::Error| PackError::Io(format!("cannot read {}: {why}", shown(&entry.path)));
        // One byte more than it was listed with shows a file that grew since.
        let file = fs::File::open(self.root.join(&entry.path)).map_err(fail)?;
        let mut bytes = Vec::new();
        file.take(entry.size + 1)
            .read_to_end(&mut bytes)
            .map_err(fail)?;
        Ok(bytes)
    }
}

/// A file read by slices through the install-reading `Source`.
struct FileSlice {
    source: DirSource,
    name: String,
}

impl Slice for FileSlice {
    fn size(&self) -> u64 {
        self.source.size(&self.name).unwrap_or(0)
    }

    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        self.source
            .read_at(&self.name, offset, len)
            .map_err(io::Error::other)
    }
}

/// The prepared cache at `from`, a folder or a zip file, checked as a cache of this build.
pub fn open(from: &Path) -> Result<Pack<Box<dyn Container>>, PackError> {
    let meta = fs::metadata(from)
        .map_err(|e| PackError::Io(format!("cannot read {}: {e}", from.display())))?;
    if meta.is_dir() {
        return Pack::open(Box::new(DirContainer::open(from)?));
    }
    let (Some(parent), Some(name)) = (from.parent(), from.file_name()) else {
        return Err(PackError::Invalid(format!("{} is no file", from.display())));
    };
    let slice = FileSlice {
        source: DirSource::new(if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        }),
        name: name.to_string_lossy().into_owned(),
    };
    Pack::open(Box::new(ZipContainer::open(slice)?))
}

/// Puts the prepared cache at `from` in place of the one at `cache`, once every file of it has
/// been read and checked: a cache that fails half way leaves the old one. `progress` is told how
/// many files are done of how many, and the one at hand.
pub fn install(
    from: &Path,
    cache: &Path,
    progress: &mut dyn FnMut(usize, usize, &str),
) -> Result<Summary, PackError> {
    let pack = open(from)?;
    let io = |e: heatel::Error| PackError::Io(e.to_string());
    let staging = sibling(cache, "importing").map_err(io)?;
    remove(&staging).map_err(io)?;
    let made = copy(&pack, &staging, progress);
    if let Err(why) = made {
        let _ = remove(&staging);
        return Err(why);
    }
    replace(&staging, cache).map_err(io)?;
    Ok(Summary {
        files: pack.len(),
        bytes: pack.bytes(),
        stamp: pack.stamp().to_owned(),
    })
}

/// The files of `pack`, then its index and its stamp last, in the folder `staging`.
fn copy(
    pack: &Pack<impl Container>,
    staging: &Path,
    progress: &mut dyn FnMut(usize, usize, &str),
) -> Result<(), PackError> {
    let io = |e: heatel::Error| PackError::Io(e.to_string());
    let mut sink = DirSink::new(staging);
    for n in 0..pack.len() {
        let entry = pack.entry(n).expect("n is below the pack's length");
        progress(n, pack.len(), &entry.path);
        sink.write(&entry.path, &pack.read(n)?).map_err(io)?;
    }
    progress(pack.len(), pack.len(), "");
    sink.write(INDEX, pack.index().as_bytes()).map_err(io)?;
    sink.write(STAMP, pack.stamp().as_bytes()).map_err(io)
}

/// Writes the cache at `cache` (which must be this build's) as the zip `to`, which must not
/// exist yet. The zip is made beside it and renamed when whole.
pub fn export(cache: &Path, to: &Path) -> Result<Summary, PackError> {
    let pack = Pack::open(DirContainer::open(cache)?)?;
    if to.exists() {
        return Err(PackError::Io(format!("{} exists already", to.display())));
    }
    let io = |e: io::Error| PackError::Io(format!("cannot write {}: {e}", to.display()));
    let part = sibling(to, "part").map_err(|e| PackError::Io(e.to_string()))?;
    let result = write_zip(&pack, &part).map_err(io);
    if let Err(why) = result.and_then(|()| fs::rename(&part, to).map_err(io)) {
        let _ = fs::remove_file(&part);
        return Err(why);
    }
    Ok(Summary {
        files: pack.len(),
        bytes: pack.bytes(),
        stamp: pack.stamp().to_owned(),
    })
}

fn write_zip(pack: &Pack<DirContainer>, part: &Path) -> io::Result<()> {
    let file = fs::File::create_new(part)?;
    let mut zip = ZipWriter::new(BufWriter::new(file));
    zip.add(STAMP, pack.stamp().as_bytes())?;
    zip.add(INDEX, pack.index().as_bytes())?;
    for n in 0..pack.len() {
        let entry = pack.entry(n).expect("n is below the pack's length");
        let bytes = pack.read(n).map_err(io::Error::other)?;
        zip.add(&entry.path, &bytes)?;
    }
    zip.finish()?.flush()
}
