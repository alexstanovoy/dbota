//! Typed loads can select a decoder without a recognized filename suffix.
//! Only the cache formats whose headers and dependencies are vetted may reach
//! those loaders; the interface's retained backgrounds use PNG alone.

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Ktx2,
}

pub fn is_safe(path: &Path) -> bool {
    path.to_str().is_some_and(dbota_import::import::safe_path)
}

pub fn image_format(path: &Path) -> Option<ImageFormat> {
    if !is_safe(path) {
        return None;
    }
    if extension_is(path, "png") {
        Some(ImageFormat::Png)
    } else if extension_is(path, "ktx2") {
        Some(ImageFormat::Ktx2)
    } else {
        None
    }
}

pub fn is_model(path: &Path) -> bool {
    is_safe(path) && (extension_is(path, "glb") || extension_is(path, "gltf"))
}

fn extension_is(path: &Path, expected: &str) -> bool {
    path.extension().is_some_and(|extension| {
        extension
            .as_encoded_bytes()
            .eq_ignore_ascii_case(expected.as_bytes())
    })
}
