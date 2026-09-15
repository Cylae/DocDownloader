use std::path::Path;
use crate::core::document::{AssetType, PageGeometry};
use crate::core::error::DocDownloaderError;

/// Validated image metadata extracted from file header/stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub asset_type: AssetType,
    pub is_direct_jpeg: bool,
}

impl ImageMetadata {
    pub fn geometry(&self) -> PageGeometry {
        PageGeometry::new(self.width, self.height)
    }
}

/// Inspects a downloaded asset file on disk, validates its magic bytes and dimensions,
/// and detects corrupted files, HTML disguised as images, or 1x1 tracking pixels.
pub fn inspect_and_validate_asset(
    file_path: &Path,
    page_index: u32,
) -> Result<ImageMetadata, DocDownloaderError> {
    let bytes = std::fs::read(file_path).map_err(|e| DocDownloaderError::FileSystemError {
        path: file_path.to_path_buf(),
        reason: format!("Failed to read asset for validation: {e}"),
    })?;

    if bytes.is_empty() {
        return Err(DocDownloaderError::PageCorrupt {
            page_index,
            reason: "Downloaded asset file is 0 bytes".to_string(),
        });
    }

    // Check for HTML disguised as image (common when server returns 200 with HTML error page)
    if bytes.len() >= 15 {
        let prefix = String::from_utf8_lossy(&bytes[..15.min(bytes.len())]).to_ascii_lowercase();
        if prefix.contains("<html")
            || prefix.contains("<!doctype")
            || prefix.contains("{\"status\"")
            || prefix.contains("{\"error\"")
        {
            return Err(DocDownloaderError::PageCorrupt {
                page_index,
                reason: "Downloaded asset is an HTML/JSON error document disguised as an image".to_string(),
            });
        }
    }

    // 1. JPEG: 0xFF, 0xD8, 0xFF
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        match parse_jpeg_dimensions(&bytes) {
            Some((width, height)) => {
                validate_dimensions(width, height, page_index)?;
                return Ok(ImageMetadata {
                    width,
                    height,
                    asset_type: AssetType::ImageJpeg,
                    is_direct_jpeg: true,
                });
            }
            None => {
                // Try fallback to image crate reader
                let img = image::load_from_memory(&bytes).map_err(|e| {
                    DocDownloaderError::PageCorrupt {
                        page_index,
                        reason: format!("Corrupted JPEG image stream: {e}"),
                    }
                })?;
                let (w, h) = (img.width(), img.height());
                validate_dimensions(w, h, page_index)?;
                return Ok(ImageMetadata {
                    width: w,
                    height: h,
                    asset_type: AssetType::ImageJpeg,
                    is_direct_jpeg: true,
                });
            }
        }
    }

    // 2. PNG: 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A
    if bytes.len() >= 8 && bytes[0..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        let img = image::load_from_memory(&bytes).map_err(|e| {
            DocDownloaderError::PageCorrupt {
                page_index,
                reason: format!("Corrupted PNG image: {e}"),
            }
        })?;
        let (w, h) = (img.width(), img.height());
        validate_dimensions(w, h, page_index)?;
        return Ok(ImageMetadata {
            width: w,
            height: h,
            asset_type: AssetType::ImagePng,
            is_direct_jpeg: false,
        });
    }

    // 3. WebP: RIFF....WEBP
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let img = image::load_from_memory(&bytes).map_err(|e| {
            DocDownloaderError::PageCorrupt {
                page_index,
                reason: format!("Corrupted WebP image: {e}"),
            }
        })?;
        let (w, h) = (img.width(), img.height());
        validate_dimensions(w, h, page_index)?;
        return Ok(ImageMetadata {
            width: w,
            height: h,
            asset_type: AssetType::ImageWebp,
            is_direct_jpeg: false,
        });
    }

    // 4. Source PDF: %PDF-
    if bytes.len() >= 5 && &bytes[0..5] == b"%PDF-" {
        return Ok(ImageMetadata {
            width: 595,
            height: 842,
            asset_type: AssetType::DirectPdf,
            is_direct_jpeg: false,
        });
    }

    Err(DocDownloaderError::PageCorrupt {
        page_index,
        reason: format!(
            "Unrecognized or unsupported image magic bytes: {:02x?}",
            &bytes[..4.min(bytes.len())]
        ),
    })
}

fn validate_dimensions(width: u32, height: u32, page_index: u32) -> Result<(), DocDownloaderError> {
    if width == 0 || height == 0 {
        return Err(DocDownloaderError::PageCorrupt {
            page_index,
            reason: format!("Page image has invalid zero dimension ({width}x{height})"),
        });
    }

    // Detect 1x1 or tiny tracking image
    if width <= 2 && height <= 2 {
        return Err(DocDownloaderError::PageCorrupt {
            page_index,
            reason: format!("Detected placeholder tracking pixel ({width}x{height}) instead of valid page"),
        });
    }

    // Detect absurd decompression bomb dimensions (> 50,000 pixels)
    if width > 50_000 || height > 50_000 {
        return Err(DocDownloaderError::PageCorrupt {
            page_index,
            reason: format!("Absurd image dimensions ({width}x{height}) exceed safety bounds"),
        });
    }

    Ok(())
}

/// Fast JPEG dimension parser reading SOF markers (SOF0=0xC0, SOF1=0xC1, SOF2=0xC2).
fn parse_jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2; // Skip SOI (0xFF, 0xD8)
    while i + 8 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        // Standalone markers
        if marker == 0xD8 || marker == 0xD9 || marker == 0x00 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }

        let length = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if i + 2 + length > data.len() {
            break;
        }

        // SOF markers contain: marker(2) + len(2) + precision(1) + height(2) + width(2)
        if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            if length >= 7 && i + 8 < data.len() {
                let height = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
                let width = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
                if width > 0 && height > 0 {
                    return Some((width, height));
                }
            }
        }

        i += 2 + length;
    }
    None
}
