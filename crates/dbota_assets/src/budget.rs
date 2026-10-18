use crate::error::{Result, malformed};
use crate::kv3::Value;

/// Most memory one decoded tree may take (values plus the strings and blobs they copy); the
/// largest tree of the install (the map's world node) takes ~20 MB.
const MAX_TREE_BYTES: usize = 256 << 20;
/// Deepest nesting of arrays, objects and structs.
const MAX_DEPTH: usize = 256;

/// Bounds the tree a decoder builds, so no input (a string referenced a million times, a typed
/// array of zero-width constants, a pointer cycle) grows it without limit.
pub(crate) struct Budget {
    what: &'static str,
    left: usize,
}

impl Budget {
    pub(crate) fn new(what: &'static str) -> Self {
        Self {
            what,
            left: MAX_TREE_BYTES,
        }
    }

    /// Charges one value at nesting `depth`.
    pub(crate) fn node(&mut self, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(malformed(
                self.what,
                format!("nested deeper than {MAX_DEPTH}"),
            ));
        }
        self.bytes(size_of::<Value>())
    }

    /// Charges `len` copied bytes.
    pub(crate) fn bytes(&mut self, len: usize) -> Result<()> {
        self.left = self.left.checked_sub(len).ok_or_else(|| {
            malformed(
                self.what,
                format!("decodes to more than {MAX_TREE_BYTES} bytes"),
            )
        })?;
        Ok(())
    }

    /// A copy of `text`, charged.
    pub(crate) fn copy(&mut self, text: &str) -> Result<String> {
        self.bytes(text.len())?;
        Ok(text.to_owned())
    }
}
