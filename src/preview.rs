use std::fs;
use std::path::Path;

use crate::error::Result;

/// Encoded image format for an extracted preview payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewFormat {
    /// PNG-encoded preview bytes.
    Png,
    /// JPEG-encoded preview bytes.
    Jpeg,
    /// WebP-encoded preview bytes.
    Webp,
    /// The preview was extracted, but the exact format is not known yet.
    Unknown,
}

impl PreviewFormat {
    pub(crate) fn detect(bytes: &[u8]) -> Self {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Self::Png
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Self::Jpeg
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Self::Webp
        } else {
            Self::Unknown
        }
    }

    /// Returns the recommended file extension for this image format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Unknown => "bin",
        }
    }

    /// Returns the media type for this image format.
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Unknown => "application/octet-stream",
        }
    }
}

/// Binary payload and metadata for an extracted preview image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    format: PreviewFormat,
    bytes: Vec<u8>,
    width: Option<u32>,
    height: Option<u32>,
}

impl Preview {
    /// Creates a preview object from encoded bytes.
    pub fn new(format: PreviewFormat, bytes: Vec<u8>) -> Self {
        Self {
            format,
            bytes,
            width: None,
            height: None,
        }
    }

    /// Stores optional pixel dimensions discovered during parsing.
    pub fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// Returns the encoded preview format.
    pub fn format(&self) -> PreviewFormat {
        self.format
    }

    /// Returns the raw encoded preview bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the encoded preview length in bytes.
    pub fn len(&self) -> usize {
        self.bytes().len()
    }

    /// Returns `true` if the preview payload is empty.
    pub fn is_empty(&self) -> bool {
        self.bytes().is_empty()
    }

    /// Returns the parsed preview dimensions, if known.
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        Some((self.width?, self.height?))
    }

    /// Writes the encoded preview bytes to disk.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        fs::write(path, self.bytes())?;
        Ok(())
    }
}
