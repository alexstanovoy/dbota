//! What a glTF or GLB file of the asset cache may name, read from its JSON before Bevy's loader
//! sees it. The cache is a folder on the user's disk like any other (or a prepared one someone
//! handed on), and the loader takes a model at its word: an image embedded in it (a `data:` URI
//! or a buffer view) is decoded without the caps `cachefs` puts on the texture files it reads
//! (Bevy decodes it with no limits at all: a PNG of 256 x 4 billion pixels is a 4 TB
//! allocation, and a supercompressed KTX2 inflates without a bound), and a URI that is no plain
//! file name is a way out of the folder. The importer writes every image and buffer of a model
//! as a file next to it (a binary buffer inside a GLB has no URI), so that is all a model is
//! let to name.
//!
//! Plain std and serde, no Bevy: the fuzz targets read this file as their own module.

use serde::Deserialize;

const GLB_MAGIC: &[u8; 4] = b"glTF";
const GLB_JSON: u32 = 0x4E4F_534A;
/// The GLB header and the first chunk's length and type.
const GLB_HEAD: usize = 20;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Document {
    images: Vec<Image>,
    buffers: Vec<Buffer>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Image {
    uri: Option<String>,
    buffer_view: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Buffer {
    uri: Option<String>,
}

/// Whether `file` is a model the cache may hold: its images are files next to it, its buffers
/// the GLB's own or files next to it.
pub fn check(file: &[u8]) -> Result<(), String> {
    let json = json(file)?;
    let document: Document = serde_json::from_slice(json).map_err(|e| format!("not glTF: {e}"))?;
    for image in &document.images {
        if image.buffer_view.is_some() {
            return Err("an image inside the model".to_owned());
        }
        let uri = image.uri.as_deref().ok_or("an image without a file")?;
        plain(uri)?;
    }
    for uri in document.buffers.iter().filter_map(|b| b.uri.as_deref()) {
        plain(uri)?;
    }
    Ok(())
}

/// The JSON of a glTF file: all of it, or a GLB's first chunk.
fn json(file: &[u8]) -> Result<&[u8], String> {
    if !file.starts_with(GLB_MAGIC) {
        return Ok(file);
    }
    let word = |at: usize| {
        file.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or("a GLB cut short")
    };
    let (version, length, chunk, kind) = (word(4)?, word(8)?, word(12)?, word(16)?);
    if version != 2 || kind != GLB_JSON || length as usize > file.len() {
        return Err("a GLB that is not as the export writes it".to_owned());
    }
    file.get(GLB_HEAD..GLB_HEAD + chunk as usize)
        .ok_or_else(|| "a GLB cut short".to_owned())
}

/// `uri` as a bare file name of the characters the export writes in them.
fn plain(uri: &str) -> Result<(), String> {
    let name = !uri.is_empty()
        && uri != "."
        && uri != ".."
        && uri
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'+'));
    if name {
        Ok(())
    } else {
        Err(format!(
            "a file named {:?}, not a plain name",
            uri.escape_debug().take(64).collect::<String>()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glb(json: &str) -> Vec<u8> {
        let mut json = json.as_bytes().to_vec();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let mut file = GLB_MAGIC.to_vec();
        file.extend(2u32.to_le_bytes());
        file.extend(((GLB_HEAD + json.len()) as u32).to_le_bytes());
        file.extend((json.len() as u32).to_le_bytes());
        file.extend(GLB_JSON.to_le_bytes());
        file.extend(json);
        file
    }

    /// The models the export writes (images next to them, a GLB's buffer inside it) are let
    /// through; whatever else names an image or a buffer is not.
    #[test]
    fn a_model_names_only_files_next_to_it() {
        let ok =
            r#"{"images":[{"uri":"head_color.ktx2"}],"buffers":[{"byteLength":8}],"meshes":[]}"#;
        assert_eq!(check(&glb(ok)), Ok(()));
        assert_eq!(check(ok.as_bytes()), Ok(()));
        let refused = [
            (
                r#"{"images":[{"bufferView":0,"mimeType":"image/png"}]}"#,
                "an image inside the model",
            ),
            (
                r#"{"images":[{"uri":"data:image/png;base64,iVBORw0KGgo="}]}"#,
                "not a plain name",
            ),
            (
                r#"{"images":[{"uri":"../hud/escape.png"}]}"#,
                "not a plain name",
            ),
            (
                r#"{"images":[{"uri":"textures/escape.png"}]}"#,
                "not a plain name",
            ),
            (
                r#"{"images":[{"uri":"cache://hud/a.png"}]}"#,
                "not a plain name",
            ),
            (r#"{"images":[{"uri":"%2e%2e"}]}"#, "not a plain name"),
            (r#"{"images":[{"uri":".."}]}"#, "not a plain name"),
            (
                r#"{"images":[{"mimeType":"image/png"}]}"#,
                "an image without a file",
            ),
            (
                r#"{"buffers":[{"uri":"data:application/octet-stream;base64,AAAA"}]}"#,
                "not a plain name",
            ),
            (r#"{"buffers":[{"uri":"/etc/passwd"}]}"#, "not a plain name"),
            ("not json", "not glTF"),
        ];
        for (json, why) in refused {
            for file in [glb(json), json.as_bytes().to_vec()] {
                let error = check(&file).expect_err(json);
                assert!(error.contains(why), "{json}: {error}");
            }
        }
    }

    #[test]
    fn a_glb_cut_short_or_of_another_kind_is_refused() {
        let whole = glb(r#"{"images":[]}"#);
        for cut in [4, 12, 19, 22] {
            assert!(check(&whole[..cut]).is_err(), "cut at {cut}");
        }
        let mut version_1 = whole.clone();
        version_1[4] = 1;
        assert!(check(&version_1).is_err());
        let mut long_chunk = whole;
        long_chunk[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(check(&long_chunk).is_err());
    }
}
