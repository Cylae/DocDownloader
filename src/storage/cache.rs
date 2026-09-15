use std::path::{Path, PathBuf};

use crate::core::error::DocDownloaderError;
use crate::storage::atomic::AtomicFileWriter;

/// Manages local cache directories, guaranteeing strict provider and document isolation.
#[derive(Debug, Clone)]
pub struct CacheManager {
    base_dir: PathBuf,
}

impl CacheManager {
    /// Creates a CacheManager with a specified root directory.
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Default system cache directory (~/.cache/docdownloader or %LOCALAPPDATA%/DocDownloader/cache).
    pub fn default_dir() -> PathBuf {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            PathBuf::from(local_app_data).join("DocDownloader").join("cache")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".cache").join("docdownloader")
        } else {
            std::env::temp_dir().join("docdownloader_cache")
        }
    }

    pub fn default_manager() -> Self {
        Self::new(Self::default_dir())
    }

    /// Returns the isolated cache directory for a specific provider and publication identifier.
    pub fn publication_dir(&self, provider: &str, publication_id: &str) -> PathBuf {
        let safe_provider = crate::storage::sanitize::sanitize_filename(provider);
        let safe_id = crate::storage::sanitize::sanitize_filename(publication_id);
        self.base_dir.join(safe_provider).join(safe_id)
    }

    /// Returns path to the job manifest JSON file for the publication.
    pub fn manifest_path(&self, provider: &str, publication_id: &str) -> PathBuf {
        self.publication_dir(provider, publication_id).join("job.json")
    }

    /// Returns deterministic path for a specific page asset.
    pub fn page_asset_path(
        &self,
        provider: &str,
        publication_id: &str,
        page_index: u32,
        extension: &str,
    ) -> PathBuf {
        self.publication_dir(provider, publication_id)
            .join("pages")
            .join(format!("page_{page_index:04}.{extension}"))
    }

    /// Creates an atomic writer for a page asset, creating directories as needed.
    pub fn atomic_page_writer(
        &self,
        provider: &str,
        publication_id: &str,
        page_index: u32,
        extension: &str,
    ) -> Result<AtomicFileWriter, DocDownloaderError> {
        let target = self.page_asset_path(provider, publication_id, page_index, extension);
        AtomicFileWriter::new(&target)
    }

    /// Cleans cache for a specific publication.
    pub fn clean_publication(&self, provider: &str, publication_id: &str) -> Result<(), DocDownloaderError> {
        let dir = self.publication_dir(provider, publication_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| DocDownloaderError::FileSystemError {
                path: dir,
                reason: format!("Failed to remove publication cache: {e}"),
            })?;
        }
        Ok(())
    }

    /// Cleans all cached publications.
    pub fn clean_all(&self) -> Result<(), DocDownloaderError> {
        if self.base_dir.exists() {
            std::fs::remove_dir_all(&self.base_dir).map_err(|e| DocDownloaderError::FileSystemError {
                path: self.base_dir.clone(),
                reason: format!("Failed to clean cache root: {e}"),
            })?;
        }
        Ok(())
    }

    /// Lists all cached publication IDs grouped by provider.
    pub fn list_cached_publications(&self) -> Result<Vec<(String, String, PathBuf)>, DocDownloaderError> {
        let mut result = Vec::new();
        if !self.base_dir.exists() {
            return Ok(result);
        }

        let provider_entries = std::fs::read_dir(&self.base_dir).map_err(|e| DocDownloaderError::FileSystemError {
            path: self.base_dir.clone(),
            reason: format!("Failed to read cache root: {e}"),
        })?;

        for prov_entry in provider_entries.flatten() {
            if prov_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let provider_name = prov_entry.file_name().to_string_lossy().to_string();
                if let Ok(pub_entries) = std::fs::read_dir(prov_entry.path()) {
                    for pub_entry in pub_entries.flatten() {
                        if pub_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                            let pub_id = pub_entry.file_name().to_string_lossy().to_string();
                            result.push((provider_name.clone(), pub_id, pub_entry.path()));
                        }
                    }
                }
            }
        }

        Ok(result)
    }
}
