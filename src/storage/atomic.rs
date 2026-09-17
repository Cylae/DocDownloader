use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::error::DocDownloaderError;

/// Safely writes content atomically to a target destination using a `.part` temporary file
/// followed by an atomic filesystem rename.
pub struct AtomicFileWriter {
    target_path: PathBuf,
    temp_path: PathBuf,
    file: Option<File>,
}

impl AtomicFileWriter {
    pub fn new(target_path: &Path) -> Result<Self, DocDownloaderError> {
        let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| DocDownloaderError::FileSystemError {
                path: parent.to_path_buf(),
                reason: format!("Failed to create parent directory: {e}"),
            })?;
        }

        let file_name = target_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("output");
        static ATOMIC_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let count = ATOMIC_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp_name = format!(".{file_name}.{}.{count}.part", std::process::id());
        let temp_path = parent.join(temp_name);

        let file = File::create(&temp_path).map_err(|e| DocDownloaderError::FileSystemError {
            path: temp_path.clone(),
            reason: format!("Failed to create temporary file for atomic write: {e}"),
        })?;

        Ok(Self {
            target_path: target_path.to_path_buf(),
            temp_path,
            file: Some(file),
        })
    }

    pub fn temp_path(&self) -> &Path {
        &self.temp_path
    }

    pub fn target_path(&self) -> &Path {
        &self.target_path
    }

    /// Appends bytes to the in-progress temporary file.
    pub fn write_all(&mut self, data: &[u8]) -> Result<(), DocDownloaderError> {
        if let Some(ref mut f) = self.file {
            f.write_all(data)
                .map_err(|e| DocDownloaderError::FileSystemError {
                    path: self.temp_path.clone(),
                    reason: format!("Write failure on temporary file: {e}"),
                })?;
            Ok(())
        } else {
            Err(DocDownloaderError::InternalInvariantViolation {
                reason: "Attempted to write to already closed AtomicFileWriter".to_string(),
            })
        }
    }

    /// Flushes, closes, and atomically commits the file to its final destination.
    pub fn commit(mut self) -> Result<PathBuf, DocDownloaderError> {
        if let Some(mut f) = self.file.take() {
            f.flush().map_err(|e| DocDownloaderError::FileSystemError {
                path: self.temp_path.clone(),
                reason: format!("Flush failure on temporary file: {e}"),
            })?;
            drop(f);
        }

        // On Windows, std::fs::rename will fail if target exists, so remove existing first if needed
        if self.target_path.exists() {
            let _ = std::fs::remove_file(&self.target_path);
        }

        std::fs::rename(&self.temp_path, &self.target_path).map_err(|e| {
            let _ = std::fs::remove_file(&self.temp_path);
            DocDownloaderError::FileSystemError {
                path: self.target_path.clone(),
                reason: format!(
                    "Failed atomic rename from {:?} to {:?}: {e}",
                    self.temp_path, self.target_path
                ),
            }
        })?;

        Ok(self.target_path.clone())
    }

    /// Aborts the atomic write, removing the temporary file if present.
    pub fn abort(mut self) {
        if let Some(f) = self.file.take() {
            drop(f);
        }
        let _ = std::fs::remove_file(&self.temp_path);
    }
}

impl Drop for AtomicFileWriter {
    fn drop(&mut self) {
        if self.file.is_some() {
            // Commit was not called; clean up temporary part file
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}
