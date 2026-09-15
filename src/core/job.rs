use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::document::AssetType;
use crate::core::error::DocDownloaderError;

/// State machine for a document acquisition job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    Queued,
    ValidatingUrl,
    ProbingProvider,
    ResolvingMetadata,
    ResolvingPages,
    Downloading { completed: u32, total: u32 },
    VerifyingPages,
    BuildingPdf,
    ValidatingPdf,
    Completed {
        output_path: PathBuf,
        total_pages: u32,
        bytes: u64,
    },
    Failed { error: String },
    Cancelled,
}

impl JobState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed { .. } | Self::Cancelled)
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::ValidatingUrl => "Validating URL",
            Self::ProbingProvider => "Detecting provider",
            Self::ResolvingMetadata => "Resolving publication metadata",
            Self::ResolvingPages => "Resolving page manifest",
            Self::Downloading { .. } => "Downloading page assets",
            Self::VerifyingPages => "Verifying page integrity",
            Self::BuildingPdf => "Reconstructing PDF document",
            Self::ValidatingPdf => "Validating generated PDF",
            Self::Completed { .. } => "Completed successfully",
            Self::Failed { .. } => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

/// Metadata recorded for a downloaded, verified page asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedPageAsset {
    pub page_index: u32,
    pub relative_path: String,
    pub sha256: String,
    pub byte_size: u64,
    pub width: u32,
    pub height: u32,
    pub asset_type: AssetType,
}

/// Persisted checkpoint manifest enabling safe resumption of interrupted jobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobManifest {
    pub tool_version: String,
    pub provider: String,
    pub publication_id: String,
    pub canonical_url: String,
    pub title: String,
    pub expected_pages: u32,
    /// BTreeMap ensures deterministic JSON serialization ordered by page index.
    pub completed_pages: BTreeMap<u32, CompletedPageAsset>,
    pub direct_pdf_downloaded: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

impl JobManifest {
    pub fn new(
        provider: &str,
        publication_id: &str,
        canonical_url: &str,
        title: &str,
        expected_pages: u32,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            provider: provider.to_string(),
            publication_id: publication_id.to_string(),
            canonical_url: canonical_url.to_string(),
            title: title.to_string(),
            expected_pages,
            completed_pages: BTreeMap::new(),
            direct_pdf_downloaded: false,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn record_completed_page(&mut self, asset: CompletedPageAsset) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.completed_pages.insert(asset.page_index, asset);
        self.updated_at = now;
    }

    pub fn is_complete(&self) -> bool {
        self.completed_pages.len() as u32 == self.expected_pages
    }

    pub fn missing_page_indices(&self) -> Vec<u32> {
        let mut missing = Vec::new();
        for i in 1..=self.expected_pages {
            if !self.completed_pages.contains_key(&i) {
                missing.push(i);
            }
        }
        missing
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), DocDownloaderError> {
        let serialized = serde_json::to_string_pretty(self).map_err(|e| {
            DocDownloaderError::InternalInvariantViolation {
                reason: format!("Failed to serialize job manifest: {e}"),
            }
        })?;

        // Write atomically via a temporary file in the same directory
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let temp_path = parent.join(format!(
            ".tmp_manifest_{}.json",
            uuid_simple()
        ));

        std::fs::write(&temp_path, serialized.as_bytes()).map_err(|e| {
            DocDownloaderError::FileSystemError {
                path: temp_path.clone(),
                reason: format!("Failed to write temporary job manifest: {e}"),
            }
        })?;

        std::fs::rename(&temp_path, path).map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            DocDownloaderError::FileSystemError {
                path: path.to_path_buf(),
                reason: format!("Failed to atomically commit job manifest: {e}"),
            }
        })?;

        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Result<Self, DocDownloaderError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            DocDownloaderError::FileSystemError {
                path: path.to_path_buf(),
                reason: format!("Failed to read job manifest: {e}"),
            }
        })?;

        let manifest: Self = serde_json::from_str(&content).map_err(|e| {
            DocDownloaderError::InvalidMetadata {
                id: path.display().to_string(),
                reason: format!("Corrupted job manifest JSON: {e}"),
            }
        })?;

        Ok(manifest)
    }
}

fn uuid_simple() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{time}_{count}")
}
