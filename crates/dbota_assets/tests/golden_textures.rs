//! Golden tests for the texture decoder: decode the `vtex_c` files behind the reference cache
//! (see `common`) from the local Dota 2 install, and compare with its PNG files. They need both, so they are ignored by default:
//!
//! ```sh
//! cargo test --release -p dbota_assets --test golden_textures -- --ignored --nocapture
//! ```
//!
//! `DBOTA_DOTA_PATH` (a `dota 2 beta` folder) and `DBOTA_REFERENCE` (the exported cache) override
//! where they are looked for.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dbota_assets::{DirSource, Encoding, Pack, PixelFormat, Texture, Vpk, find_game_dir};

struct Install {
    src: DirSource,
    pak: Vpk,
    cache: PathBuf,
}

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn install() -> Install {
    let dota_path = std::env::var_os("DBOTA_DOTA_PATH").map(PathBuf::from);
    let game = find_game_dir(dota_path.as_deref())
        .expect("Dota 2 not found; set DBOTA_DOTA_PATH to its \"dota 2 beta\" folder");
    let cache = common::reference();
    assert!(cache.is_dir(), "no asset cache at {}", cache.display());
    let src = DirSource::new(game);
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    Install { src, pak, cache }
}

/// The `(vpk path, cache path)` of every `texture` line of the manifest.
fn manifest_textures() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(repo().join("assets-manifest/manifest.txt")).unwrap();
    text.lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split('#').next()?.split_whitespace().collect();
            match words.as_slice() {
                ["texture", _, src, out, ..] => Some(((*src).to_owned(), (*out).to_owned())),
                _ => None,
            }
        })
        .collect()
}

/// An exported PNG as RGBA8: width, height, pixels.
fn read_png(path: &Path) -> (u32, u32, Vec<u8>) {
    let file = std::fs::File::open(path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; a texture added after the reference was exported needs its image there, \
             made by the same Source2Viewer-CLI 20 (`-i pak01_dir.vpk -d -f <vtex_c> -o <dir>/`)",
            path.display()
        )
    });
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut buf).unwrap();
    buf.truncate(frame.buffer_size());
    let rgba = match frame.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => unreachable!(),
    };
    (frame.width, frame.height, rgba)
}

/// The RGBA8 pixels of a level, decoded as a GPU would: texels of an uncompressed format as
/// they are, blocks through an independent decoder.
fn to_rgba(format: PixelFormat, width: u32, height: u32, level: &[u8]) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut px = vec![0u32; w * h];
    let blocks = match format {
        PixelFormat::Rgba8 => return level.to_vec(),
        PixelFormat::Bc1 => texture2ddecoder::decode_bc1(level, w, h, &mut px),
        PixelFormat::Bc3 => texture2ddecoder::decode_bc3(level, w, h, &mut px),
        PixelFormat::Bc4 => texture2ddecoder::decode_bc4(level, w, h, &mut px),
        PixelFormat::Bc5 => texture2ddecoder::decode_bc5(level, w, h, &mut px),
        PixelFormat::Bc7 => texture2ddecoder::decode_bc7(level, w, h, &mut px),
        other => panic!("no reference decoder for {other:?}"),
    };
    blocks.unwrap();
    px.iter()
        .flat_map(|p| {
            let [b, g, r, a] = p.to_le_bytes();
            [r, g, b, a]
        })
        .collect()
}

/// How far two RGBA8 images are apart: the largest and the mean difference of any channel.
fn distance(a: &[u8], b: &[u8]) -> (u8, f64) {
    assert_eq!(a.len(), b.len());
    let max = a
        .iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0);
    let sum: u64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    (max, sum as f64 / a.len().max(1) as f64)
}

/// The KTX2 file read back by an independent parser: the texture's size, format and levels.
fn check_ktx2(texture: &Texture, encoding: Encoding, what: &str) {
    let file = texture.to_ktx2(encoding);
    let reader = ktx2::Reader::new(&file[..]).unwrap_or_else(|e| panic!("{what}: {e:?}"));
    let header = reader.header();
    assert_eq!(
        (header.pixel_width, header.pixel_height),
        (texture.width(), texture.height()),
        "{what}"
    );
    assert_eq!(header.level_count as usize, texture.mips().len(), "{what}");
    assert!(header.supercompression_scheme.is_none(), "{what}");
    assert!(header.format.is_some(), "{what}");
    for (i, (level, mip)) in reader.levels().zip(texture.mips()).enumerate() {
        assert_eq!(level.data, &mip[..], "{what}: level {i}");
    }
    assert!(
        !reader.dfd_blocks().is_empty(),
        "{what}: no data format descriptor"
    );
}

/// The texture checks shared by every file: it parses, its levels have the sizes of their
/// dimensions, and the KTX2 of it reads back.
fn check_levels(texture: &Texture, what: &str) {
    for (i, mip) in texture.mips().iter().enumerate() {
        let size = texture.format().level_size(
            (texture.width() >> i).max(1),
            (texture.height() >> i).max(1),
        );
        assert_eq!(Some(mip.len()), size, "{what}: level {i}");
    }
    check_ktx2(texture, Encoding::Srgb, what);
    check_ktx2(texture, Encoding::Linear, what);
}

#[derive(Default)]
struct Tally {
    by_format: HashMap<String, (usize, u8, f64)>,
}

impl Tally {
    fn add(&mut self, class: &str, (max, mean): (u8, f64)) {
        let entry = self.by_format.entry(class.to_owned()).or_default();
        entry.0 += 1;
        entry.1 = entry.1.max(max);
        entry.2 = entry.2.max(mean);
    }

    fn report(&self, what: &str) {
        let mut rows: Vec<_> = self.by_format.iter().collect();
        rows.sort_by_key(|(name, _)| (*name).clone());
        for (name, (n, max, mean)) in rows {
            println!(
                "{what}: {name}: {n} textures, largest difference {max}, largest mean {mean:.4}"
            );
        }
    }
}

/// The `[x0, y0, x1, y1]` texels of an RGBA8 image `width` wide.
fn crop(rgba: &[u8], width: u32, [x0, y0, x1, y1]: [u32; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    for y in y0..y1 {
        let start = (y * width + x0) as usize * 4;
        out.extend_from_slice(&rgba[start..start + (x1 - x0) as usize * 4]);
    }
    out
}

/// How the decoded pixels may differ from the exported ones, by what the file stores.
struct Tolerance {
    class: &'static str,
    /// The largest difference of any colour channel, and of the mean.
    max: u8,
    mean: f64,
}

/// The format id of a `vtex_c` (its `DATA` block's byte 26).
fn stored_format(vtex: &[u8]) -> u8 {
    dbota_assets::Resource::parse(vtex)
        .unwrap()
        .block(b"DATA")
        .unwrap()[26]
}

/// Plain texels and PNGs come out bit for bit; blocks differ by the rounding of the interpolated
/// colours, which decoders do not agree on (one level at most); DXT5 images that hold YCoCg or a
/// normal are converted by both pipelines with their own rounding, and differ on the few normals
/// that block quantisation made longer than one (up to 18 levels in the blue channel).
fn tolerance(stored: u8, decoded: PixelFormat) -> Tolerance {
    match (stored, decoded) {
        (4 | 16 | 18 | 28, _) => Tolerance {
            class: "plain",
            max: 0,
            mean: 0.0,
        },
        (1 | 2, PixelFormat::Bc1 | PixelFormat::Bc3) => Tolerance {
            class: "blocks",
            max: 1,
            mean: 0.5,
        },
        (2, PixelFormat::Rgba8) => Tolerance {
            class: "converted DXT5",
            max: 20,
            mean: 1.0,
        },
        other => panic!("no tolerance for {other:?}"),
    }
}

/// Compares `texture` with the exported image; for a sprite sheet that is its first frame, as
/// Source2Viewer cuts it.
fn compare(
    texture: &Texture,
    stored: u8,
    exported: &Path,
    tally: &mut Tally,
) -> Result<(), String> {
    let (w, h, png) = read_png(exported);
    let what = exported.display().to_string();
    let whole = to_rgba(
        texture.format(),
        texture.width(),
        texture.height(),
        &texture.mips()[0],
    );
    let (ours, size) = match texture.first_frame_rect() {
        Some(rect) => (
            crop(&whole, texture.width(), rect),
            (rect[2] - rect[0], rect[3] - rect[1]),
        ),
        None => (whole, (texture.width(), texture.height())),
    };
    if size != (w, h) {
        return Err(format!("{what}: {size:?} against exported {:?}", (w, h)));
    }
    let d = distance(&ours, &png);
    let tol = tolerance(stored, texture.format());
    if d.0 > tol.max || d.1 > tol.mean {
        return Err(format!("{what}: {} differs by {d:?}", tol.class));
    }
    tally.add(tol.class, d);
    if tol.class == "blocks" {
        check_mips(texture, &what);
    }
    if let Some([x0, y0, ..]) = texture.first_frame_rect() {
        compare_first_frame(texture, [x0, y0], (w, h), &png, &what, &tol);
    }
    Ok(())
}

/// Every level down to 16 texels looks like the one above it shrunk by half: right sizes, in the right order,
/// whatever filter Valve's tool shrank them with.
fn check_mips(texture: &Texture, what: &str) {
    let rgba = |level: usize| {
        let (w, h) = (
            (texture.width() >> level).max(1),
            (texture.height() >> level).max(1),
        );
        (
            w as usize,
            h as usize,
            to_rgba(texture.format(), w, h, &texture.mips()[level]),
        )
    };
    let (mut w, mut h, mut above) = rgba(0);
    for level in 1..texture.mips().len() {
        let (lw, lh, this) = rgba(level);
        // A block of the tiny levels covers most of the image: they say little.
        if lw < 16 || lh < 16 {
            break;
        }
        let mut worst = 0.0f64;
        let mut sum = 0u64;
        for y in 0..lh {
            for x in 0..lw {
                for c in 0..4 {
                    let at = |dx: usize, dy: usize| {
                        let (sx, sy) = ((2 * x + dx).min(w - 1), (2 * y + dy).min(h - 1));
                        u32::from(above[(sy * w + sx) * 4 + c])
                    };
                    let box_filter = (at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4;
                    sum += u64::from(box_filter.abs_diff(u32::from(this[(y * lw + x) * 4 + c])));
                }
            }
        }
        worst = worst.max(sum as f64 / (lw * lh * 4) as f64);
        assert!(
            worst <= MAX_MIP_DISTANCE,
            "{what}: level {level} is {worst:.1} levels off the one above"
        );
        (w, h, above) = (lw, lh, this);
    }
}

/// The most a level may differ on average from the box-filtered one above it.
const MAX_MIP_DISTANCE: f64 = 20.0;

/// The frame cut from the compressed blocks holds the exported frame, at most three texels in.
fn compare_first_frame(
    texture: &Texture,
    [x0, y0]: [u32; 2],
    (w, h): (u32, u32),
    png: &[u8],
    what: &str,
    tol: &Tolerance,
) {
    let frame = texture.first_frame();
    let (bw, bh, _) = texture.format().block();
    let (dx, dy) = (x0 % bw, y0 % bh);
    assert_eq!(
        (frame.width(), frame.height()),
        (w + dx, h + dy),
        "{what}: first frame size"
    );
    let rgba = to_rgba(
        frame.format(),
        frame.width(),
        frame.height(),
        &frame.mips()[0],
    );
    let inner = crop(&rgba, frame.width(), [dx, dy, dx + w, dy + h]);
    let d = distance(&inner, png);
    assert!(
        d.0 <= tol.max && d.1 <= tol.mean,
        "{what}: first frame differs by {d:?}"
    );
    check_levels(&frame, what);
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn manifest_textures_match_the_exported_images() {
    let dota = install();
    let mut tally = Tally::default();
    let mut sheets = 0;
    for (vpk, out) in manifest_textures() {
        let bytes = dota.pak.read_file(&dota.src, &vpk).unwrap();
        let texture = Texture::parse(&bytes).unwrap_or_else(|e| panic!("{vpk}: {e}"));
        check_levels(&texture, &vpk);
        compare(
            &texture,
            stored_format(&bytes),
            &dota.cache.join(&out),
            &mut tally,
        )
        .unwrap();
        if texture.is_sheet() {
            sheets += 1;
        }
    }
    tally.report("manifest");
    println!("manifest: {sheets} sprite sheets");
}

/// Every `.png` below `dir`, by path.
fn pngs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            pngs(&path, out);
        } else if path.extension().is_some_and(|e| e == "png") {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn model_textures_match_the_exported_images() {
    let dota = install();
    let mut by_stem: HashMap<String, Vec<String>> = HashMap::new();
    for entry in dota.pak.entries().filter(|e| e.ext() == "vtex_c") {
        by_stem
            .entry(entry.name().to_owned())
            .or_default()
            .push(entry.path());
    }
    let mut files = Vec::new();
    pngs(&dota.cache.join("models"), &mut files);
    let mut tally = Tally::default();
    let (mut compared, mut skipped, mut mipped) = (0, 0, 0);
    let mut adapted = Vec::new();
    for png in files {
        let stem = png.file_stem().unwrap().to_str().unwrap().to_owned();
        // Source2Viewer packs occlusion, roughness and metalness into new `_orm_` images, and
        // gives physics hulls a material of its own.
        let source = match by_stem.get(&stem).map(Vec::as_slice) {
            Some([one]) if !stem.contains("_orm_") => one,
            _ => {
                skipped += 1;
                continue;
            }
        };
        let bytes = dota.pak.read_file(&dota.src, source).unwrap();
        let texture = Texture::parse(&bytes).unwrap_or_else(|e| panic!("{source}: {e}"));
        check_levels(&texture, source);
        if let Err(why) = compare(&texture, stored_format(&bytes), &png, &mut tally) {
            // Source2Viewer rewrites some material inputs for glTF (specular masks and the like
            // become other images); colours and normals are what it leaves alone.
            assert!(
                !["_color_png_", "_color_psd_", "_normal_png_", "_normal_psd_"]
                    .iter()
                    .any(|kind| stem.contains(kind)),
                "{why}"
            );
            adapted.push(why);
        }
        mipped += usize::from(texture.mips().len() > 1);
        compared += 1;
    }
    tally.report("models");
    println!(
        "models: {compared} compared ({mipped} with mip levels), {skipped} without a single source, {} adapted by the exporter",
        adapted.len()
    );
    for why in adapted.iter().take(5) {
        println!("models: adapted, for example: {why}");
    }
    assert!(
        compared > 100,
        "only {compared} model textures were found in the install"
    );
}

/// The images the exporter made from a material's mask textures: a grey image holding one
/// channel of the texture, and an "ORM" image with occlusion and roughness at full and metalness
/// from blue. The model side builds both from `Texture::to_rgba8`.
#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn packed_model_images_match_the_exported_ones() {
    let dota = install();
    let mut by_stem: HashMap<String, Vec<String>> = HashMap::new();
    for entry in dota.pak.entries().filter(|e| e.ext() == "vtex_c") {
        by_stem
            .entry(entry.name().to_owned())
            .or_default()
            .push(entry.path());
    }
    let mut files = Vec::new();
    pngs(&dota.cache.join("models"), &mut files);
    let (mut orm, mut grey, mut worst) = (0, 0, 0u8);
    let mut channels: HashMap<char, usize> = HashMap::new();
    for png in files {
        let stem = png.file_stem().unwrap().to_str().unwrap();
        let (source, orm_image) = match stem.split_once("_orm_") {
            Some((base, _)) => (base, true),
            None => (stem, false),
        };
        let [path] = by_stem.get(source).map(Vec::as_slice).unwrap_or_default() else {
            continue;
        };
        let (_, _, exported) = read_png(&png);
        let is_grey = exported
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0] == p[1] && p[1] == p[2] && p[3] == 255);
        let rgba = Texture::parse(&dota.pak.read_file(&dota.src, path).unwrap())
            .unwrap()
            .to_rgba8()
            .unwrap();
        if rgba.len() != exported.len() {
            // A material's constant mask, which the exporter wrote as a 1 x 1 image of its own.
            assert_eq!(exported.len(), 4, "{}: size", png.display());
            continue;
        }
        let packed = |pack: Pack| {
            let mut pixels = rgba.clone();
            pack.apply(&mut pixels);
            pixels
        };
        let ours: Vec<u8> = if orm_image {
            packed(Pack::Orm)
        } else if is_grey && !["_color_", "_normal_"].iter().any(|k| stem.contains(k)) {
            // The channel that is the image.
            let (name, best) = (0..4)
                .map(|c| {
                    (
                        "RGBA".chars().nth(c).unwrap(),
                        packed(Pack::Channel(c as u8)),
                    )
                })
                .min_by(|(_, a), (_, b)| {
                    distance(a, &exported)
                        .1
                        .total_cmp(&distance(b, &exported).1)
                })
                .unwrap();
            *channels.entry(name).or_default() += 1;
            best
        } else {
            continue;
        };
        let d = distance(&ours, &exported);
        worst = worst.max(d.0);
        assert!(
            d.0 <= 3 && d.1 <= 1.0,
            "{}: differs by {d:?}",
            png.display()
        );
        if orm_image { orm += 1 } else { grey += 1 }
    }
    println!(
        "packed: {orm} ORM and {grey} grey images, largest difference {worst}, channels {channels:?}"
    );
    assert!(orm > 0 && grey > 0);
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn rgba8_textures_written_as_png_read_back_the_same() {
    let dota = install();
    for (vpk, _) in manifest_textures().into_iter().take(40) {
        let texture = Texture::parse(&dota.pak.read_file(&dota.src, &vpk).unwrap()).unwrap();
        let Ok(pixels) = texture.to_rgba8() else {
            continue;
        };
        let made = Texture::from_rgba8(texture.width(), texture.height(), pixels.clone()).unwrap();
        let dir = std::env::temp_dir().join("dbota_assets_png_roundtrip.png");
        std::fs::write(&dir, made.to_png().unwrap()).unwrap();
        assert_eq!(read_png(&dir).2, pixels, "{vpk}");
        check_levels(&made, &vpk);
    }
}
