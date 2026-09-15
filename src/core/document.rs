use serde::{Deserialize, Serialize};

/// Document and page geometry specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageGeometry {
    pub width: u32,
    pub height: u32,
}

impl PageGeometry {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Aspect ratio (width / height).
    pub fn aspect_ratio(&self) -> f64 {
        if self.height == 0 {
            1.0
        } else {
            self.width as f64 / self.height as f64
        }
    }

    pub fn is_landscape(&self) -> bool {
        self.width > self.height
    }

    pub fn is_portrait(&self) -> bool {
        self.height >= self.width
    }
}

/// Supported asset media types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetType {
    /// Authoritative source PDF exposed directly by publisher.
    DirectPdf,
    /// High-resolution full page JPEG image.
    ImageJpeg,
    /// Lossless or high-quality PNG image.
    ImagePng,
    /// WebP image format.
    ImageWebp,
    /// Vector SVG / SVGZ document page.
    VectorSvg,
    /// Fallback low-resolution thumbnail.
    Thumbnail,
}

impl AssetType {
    pub fn is_image(&self) -> bool {
        matches!(
            self,
            Self::ImageJpeg | Self::ImagePng | Self::ImageWebp | Self::Thumbnail
        )
    }

    pub fn file_extension(&self) -> &'static str {
        match self {
            Self::DirectPdf => "pdf",
            Self::ImageJpeg => "jpg",
            Self::ImagePng => "png",
            Self::ImageWebp => "webp",
            Self::VectorSvg => "svg",
            Self::Thumbnail => "thumb.jpg",
        }
    }
}

/// A candidate asset source for acquiring a single page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetCandidate {
    /// Acquisition priority (0 is highest, e.g. direct PDF or full-res image).
    pub priority: u32,
    /// Source URL for the asset.
    pub url: String,
    /// Classified media type.
    pub asset_type: AssetType,
    /// Dimensions if declared in metadata.
    pub geometry: Option<PageGeometry>,
    /// Optional HTTP headers required by the provider for fetching (e.g. Host/Referer).
    pub headers: Vec<(String, String)>,
}

/// Descriptor for a single logical page in a publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageDescriptor {
    /// 1-based page index.
    pub index: u32,
    /// Page geometry if known from metadata.
    pub geometry: Option<PageGeometry>,
    /// Available candidate asset URLs ordered by preference.
    pub candidates: Vec<AssetCandidate>,
}

impl PageDescriptor {
    pub fn new(index: u32) -> Self {
        Self {
            index,
            geometry: None,
            candidates: Vec::new(),
        }
    }

    pub fn best_candidate(&self) -> Option<&AssetCandidate> {
        self.candidates.iter().min_by_key(|c| c.priority)
    }
}

/// Normalized publication metadata returned by any provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Publication {
    /// Provider identifier (e.g. "calameo").
    pub provider: String,
    /// Canonical web reader URL.
    pub canonical_url: String,
    /// Platform-unique publication identifier.
    pub publication_id: String,
    /// Document title.
    pub title: String,
    /// Author, publisher, or account name.
    pub author: Option<String>,
    /// Publication description or summary.
    pub description: Option<String>,
    /// Authoritative logical page count.
    pub page_count: u32,
    /// Publication cover thumbnail URL.
    pub thumbnail_url: Option<String>,
    /// Base document dimensions.
    pub geometry: Option<PageGeometry>,
    /// Direct source PDF download URL if legitimately enabled by publisher.
    pub direct_pdf_url: Option<String>,
    /// Ordered list of pages.
    pub pages: Vec<PageDescriptor>,
}

impl Publication {
    /// Verifies that the page descriptors are strictly sequential and complete.
    pub fn validate_completeness(&self) -> Result<(), String> {
        if self.page_count == 0 {
            return Err("Publication declares 0 pages".to_string());
        }
        if self.pages.len() != self.page_count as usize {
            return Err(format!(
                "Page count mismatch: declared {}, but enumerated {}",
                self.page_count,
                self.pages.len()
            ));
        }
        for (i, page) in self.pages.iter().enumerate() {
            let expected_idx = (i + 1) as u32;
            if page.index != expected_idx {
                return Err(format!(
                    "Page ordering gap or disorder: at position {i} expected page index {expected_idx}, got {}",
                    page.index
                ));
            }
            if page.candidates.is_empty() {
                return Err(format!("Page {} has zero candidate assets", page.index));
            }
        }
        Ok(())
    }
}
