//! File-kind detection: what viewer a changed path needs. The text diff is only
//! meaningful for text; binaries would otherwise be shaped line-by-line and hang
//! the UI, and images get their own side-by-side viewer.

use std::path::Path;

/// Image formats pm can render in the image diff viewer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageKind {
    Png,
    Jpeg,
}

impl ImageKind {
    fn from_ext(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            _ => None,
        }
    }

    fn from_magic(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
            Some(Self::Png)
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            Some(Self::Jpeg)
        } else {
            None
        }
    }
}

/// What kind of content a path holds, used to pick a viewer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    /// Plain text — the line diff applies.
    Text,
    /// A raster image pm can show side by side.
    Image(ImageKind),
    /// Anything else: shown as an "unviewable" placeholder, never diffed.
    Binary,
}

impl FileKind {
    /// Classify from the path plus a sample of each side's raw bytes (either may
    /// be empty for an added/deleted file).
    pub fn detect(path: &Path, old: &[u8], new: &[u8]) -> FileKind {
        let sample = if new.is_empty() { old } else { new };

        let ext_image = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(ImageKind::from_ext);
        if let Some(kind) = ext_image {
            // Trust the extension, but only if the bytes back it up (or are
            // absent on this side).
            if sample.is_empty() || ImageKind::from_magic(sample) == Some(kind) {
                return FileKind::Image(kind);
            }
        }
        if let Some(kind) = ImageKind::from_magic(sample) {
            return FileKind::Image(kind);
        }

        if looks_binary(old) || looks_binary(new) {
            FileKind::Binary
        } else {
            FileKind::Text
        }
    }
}

/// Git's own heuristic: a NUL byte in the first 8KB means binary.
fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|&b| b == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn text_is_text() {
        assert_eq!(
            FileKind::detect(Path::new("a.rs"), b"fn main() {}", b"fn main() {}\n"),
            FileKind::Text
        );
    }

    #[test]
    fn nul_byte_is_binary() {
        assert_eq!(
            FileKind::detect(Path::new("a.bin"), b"", b"ELF\0\0\0stuff"),
            FileKind::Binary
        );
    }

    #[test]
    fn png_magic_is_image() {
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];
        assert_eq!(
            FileKind::detect(Path::new("logo.png"), b"", &png),
            FileKind::Image(ImageKind::Png)
        );
    }

    #[test]
    fn wrong_ext_falls_back_to_bytes() {
        // Named .png but actually text — not an image.
        assert_eq!(
            FileKind::detect(Path::new("notes.png"), b"", b"just text"),
            FileKind::Text
        );
    }
}
