use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{watch, Semaphore};
use url::Url;

use crate::core::document::{AssetCandidate, Publication};
use crate::core::error::DocDownloaderError;
use crate::core::job::{CompletedPageAsset, JobManifest, JobState};
use crate::network::client::{HttpClient, MAX_PAGE_BYTE_LIMIT};
use crate::pdf::builder::PdfBuilder;
use crate::pdf::image::inspect_and_validate_asset;
use crate::providers::ProviderRegistry;
use crate::storage::cache::CacheManager;
use crate::storage::sanitize::safe_output_path;

/// Trait implemented by CLI progress bars or Web UI live event streams.
pub trait ProgressListener: Send + Sync {
    fn on_state_change(&self, state: &JobState);
    fn on_page_completed(&self, page_index: u32, total_pages: u32, bytes: u64, from_cache: bool);
    fn on_log_message(&self, message: &str);
}

/// No-op listener used when running headless or in tests.
pub struct NoopProgressListener;
impl ProgressListener for NoopProgressListener {
    fn on_state_change(&self, _state: &JobState) {}
    fn on_page_completed(
        &self,
        _page_index: u32,
        _total_pages: u32,
        _bytes: u64,
        _from_cache: bool,
    ) {
    }
    fn on_log_message(&self, _message: &str) {}
}

/// Orchestrates the entire document lifecycle from URL to verified PDF.
pub struct DownloadEngine {
    client: HttpClient,
    registry: Arc<ProviderRegistry>,
    cache: CacheManager,
    concurrency: usize,
    force_overwrite: bool,
    no_resume: bool,
}

impl DownloadEngine {
    pub fn new(
        client: HttpClient,
        registry: Arc<ProviderRegistry>,
        cache: CacheManager,
        concurrency: usize,
        force_overwrite: bool,
    ) -> Self {
        Self::with_options(client, registry, cache, concurrency, force_overwrite, false)
    }

    pub fn with_options(
        client: HttpClient,
        registry: Arc<ProviderRegistry>,
        cache: CacheManager,
        concurrency: usize,
        force_overwrite: bool,
        no_resume: bool,
    ) -> Self {
        Self {
            client,
            registry,
            cache,
            concurrency: concurrency.clamp(1, 16),
            force_overwrite,
            no_resume,
        }
    }

    /// Inspects publication without downloading the full document.
    pub async fn inspect(&self, url: &Url) -> Result<Publication, DocDownloaderError> {
        self.registry.resolve(&self.client, url).await
    }

    /// Executes the full download and assembly pipeline.
    pub async fn download(
        &self,
        url: &Url,
        output_arg: Option<&Path>,
        listener: Arc<dyn ProgressListener>,
        mut cancel_rx: watch::Receiver<bool>,
    ) -> Result<PathBuf, DocDownloaderError> {
        listener.on_state_change(&JobState::ValidatingUrl);
        if !self.client.is_local_mock_allowed() {
            crate::network::security::validate_url_security(url)?;
        }

        if *cancel_rx.borrow() {
            return Err(DocDownloaderError::Cancelled);
        }

        listener.on_state_change(&JobState::ProbingProvider);
        let publication = self.registry.resolve(&self.client, url).await?;

        listener.on_state_change(&JobState::ResolvingMetadata);
        publication
            .validate_completeness()
            .map_err(|e| DocDownloaderError::PageListInvalid {
                id: publication.publication_id.clone(),
                reason: e,
            })?;

        // Determine output path
        let final_pdf_path = match output_arg {
            Some(path) => {
                if path.is_dir() {
                    safe_output_path(path, &publication.title)
                } else {
                    path.to_path_buf()
                }
            }
            None => safe_output_path(Path::new("."), &publication.title),
        };

        if final_pdf_path.exists() && !self.force_overwrite {
            return Err(DocDownloaderError::OutputExists {
                path: final_pdf_path,
            });
        }

        let pub_dir = self
            .cache
            .publication_dir(&publication.provider, &publication.publication_id);
        std::fs::create_dir_all(&pub_dir).map_err(|e| DocDownloaderError::FileSystemError {
            path: pub_dir.clone(),
            reason: format!("Failed to create publication cache directory: {e}"),
        })?;

        let manifest_file = self
            .cache
            .manifest_path(&publication.provider, &publication.publication_id);

        // Attempt direct PDF acquisition if legitimately exposed by the provider
        if let Some(ref direct_pdf_url) = publication.direct_pdf_url {
            listener.on_log_message(
                "Publisher exposed direct PDF download; attempting direct acquisition...",
            );
            let mut direct_writer = crate::storage::atomic::AtomicFileWriter::new(&final_pdf_path)?;
            match self
                .client
                .stream_to_atomic_file(
                    direct_pdf_url,
                    None,
                    &mut direct_writer,
                    crate::network::client::MAX_PAGE_BYTE_LIMIT * 10,
                )
                .await
            {
                Ok((bytes, _hash)) => {
                    if let Ok(()) = crate::pdf::validator::validate_pdf_document(
                        direct_writer.temp_path(),
                        publication.page_count,
                    ) {
                        let committed_path = direct_writer.commit()?;
                        let mut manifest = JobManifest::new(
                            &publication.provider,
                            &publication.publication_id,
                            &publication.canonical_url,
                            &publication.title,
                            publication.page_count,
                        );
                        manifest.direct_pdf_downloaded = true;
                        let _ = manifest.save_to_file(&manifest_file);
                        listener.on_state_change(&JobState::Completed {
                            output_path: committed_path.clone(),
                            total_pages: publication.page_count,
                            bytes,
                        });
                        return Ok(committed_path);
                    } else {
                        listener.on_log_message(
                            "Direct PDF failed structural or page count validation; falling back to page reconstruction",
                        );
                        direct_writer.abort();
                    }
                }
                Err(e) => {
                    listener.on_log_message(&format!(
                        "Direct PDF acquisition failed ({e}); falling back to page reconstruction"
                    ));
                    direct_writer.abort();
                }
            }
        }

        // Load or create job manifest
        let mut manifest = if !self.no_resume {
            match JobManifest::load_from_file(&manifest_file) {
                Ok(existing) if existing.expected_pages == publication.page_count => {
                    listener.on_log_message(
                        "Found existing job checkpoint. Validating cache integrity...",
                    );
                    existing
                }
                _ => JobManifest::new(
                    &publication.provider,
                    &publication.publication_id,
                    &publication.canonical_url,
                    &publication.title,
                    publication.page_count,
                ),
            }
        } else {
            JobManifest::new(
                &publication.provider,
                &publication.publication_id,
                &publication.canonical_url,
                &publication.title,
                publication.page_count,
            )
        };

        // Validate any existing completed pages in manifest
        let mut valid_completed = BTreeMap::new();
        for (page_idx, asset) in &manifest.completed_pages {
            let asset_path = pub_dir.join(&asset.relative_path);
            if asset_path.exists() {
                if let Ok(meta) = inspect_and_validate_asset(&asset_path, *page_idx) {
                    if meta.width == asset.width && meta.height == asset.height {
                        valid_completed.insert(*page_idx, asset.clone());
                        listener.on_page_completed(
                            *page_idx,
                            publication.page_count,
                            asset.byte_size,
                            true,
                        );
                    }
                }
            }
        }
        manifest.completed_pages = valid_completed;
        manifest.save_to_file(&manifest_file)?;

        // Download missing pages with bounded concurrency
        let missing_pages = manifest.missing_page_indices();
        let total_pages = publication.page_count;

        if !missing_pages.is_empty() {
            listener.on_state_change(&JobState::Downloading {
                completed: total_pages - missing_pages.len() as u32,
                total: total_pages,
            });

            let semaphore = Arc::new(Semaphore::new(self.concurrency));
            let mut tasks = Vec::new();
            let cancelled = Arc::new(AtomicBool::new(false));

            for page_idx in missing_pages {
                let page_desc = publication.pages[(page_idx - 1) as usize].clone();
                let client = self.client.clone();
                let cache = self.cache.clone();
                let provider_name = publication.provider.clone();
                let pub_id = publication.publication_id.clone();
                let sem = semaphore.clone();
                let is_cancelled = cancelled.clone();
                let task_cancel_rx = cancel_rx.clone();

                let task = tokio::spawn(async move {
                    let _permit = sem.acquire().await.map_err(|_| {
                        DocDownloaderError::InternalInvariantViolation {
                            reason: "Semaphore closed".to_string(),
                        }
                    })?;

                    if is_cancelled.load(Ordering::Relaxed) || *task_cancel_rx.borrow() {
                        return Err(DocDownloaderError::Cancelled);
                    }

                    // Attempt candidates in order of priority
                    let mut last_err = None;
                    for candidate in &page_desc.candidates {
                        match download_single_candidate(
                            &client,
                            &cache,
                            &provider_name,
                            &pub_id,
                            &page_desc,
                            candidate,
                        )
                        .await
                        {
                            Ok(completed_asset) => return Ok(completed_asset),
                            Err(e) => {
                                last_err = Some(e);
                            }
                        }
                    }

                    Err(
                        last_err.unwrap_or_else(|| DocDownloaderError::PageUnavailable {
                            page_index: page_desc.index,
                            reason: "All candidate sources failed".to_string(),
                            status: None,
                        }),
                    )
                });

                tasks.push((page_idx, task));
            }

            for (idx, task) in tasks {
                tokio::select! {
                    res = task => {
                        match res {
                            Ok(Ok(asset)) => {
                                manifest.record_completed_page(asset.clone());
                                manifest.save_to_file(&manifest_file)?;
                                listener.on_page_completed(idx, total_pages, asset.byte_size, false);
                            }
                            Ok(Err(e)) => {
                                cancelled.store(true, Ordering::Relaxed);
                                manifest.save_to_file(&manifest_file)?;
                                return Err(e);
                            }
                            Err(join_err) => {
                                cancelled.store(true, Ordering::Relaxed);
                                manifest.save_to_file(&manifest_file)?;
                                return Err(DocDownloaderError::InternalInvariantViolation {
                                    reason: format!("Task panicked: {join_err}"),
                                });
                            }
                        }
                    }
                    _ = cancel_rx.changed() => {
                        cancelled.store(true, Ordering::Relaxed);
                        manifest.save_to_file(&manifest_file)?;
                        return Err(DocDownloaderError::Cancelled);
                    }
                }
            }
        }

        // Verify publication completeness
        listener.on_state_change(&JobState::VerifyingPages);
        if !manifest.is_complete() {
            return Err(DocDownloaderError::PageListInvalid {
                id: publication.publication_id,
                reason: format!(
                    "Incomplete publication: acquired {}/{} pages",
                    manifest.completed_pages.len(),
                    publication.page_count
                ),
            });
        }

        // Check for suspicious identical placeholder pages (heuristic)
        if total_pages >= 5 {
            let mut unique_hashes = HashSet::new();
            for asset in manifest.completed_pages.values() {
                unique_hashes.insert(&asset.sha256);
            }
            if unique_hashes.len() == 1 {
                return Err(DocDownloaderError::PageCorrupt {
                    page_index: 1,
                    reason: format!(
                        "Detected suspicious identical placeholder image across all {total_pages} pages"
                    ),
                });
            }
        }

        // Build PDF
        listener.on_state_change(&JobState::BuildingPdf);
        let builder = PdfBuilder::new(&publication);
        let ordered_pages: Vec<CompletedPageAsset> =
            manifest.completed_pages.values().cloned().collect();

        let generated_pdf = builder.build(&final_pdf_path, &pub_dir, &ordered_pages)?;

        // Validate final PDF
        listener.on_state_change(&JobState::ValidatingPdf);
        crate::pdf::validator::validate_pdf_document(&generated_pdf, total_pages)?;

        let final_bytes = std::fs::metadata(&generated_pdf)
            .map(|m| m.len())
            .unwrap_or(0);

        listener.on_state_change(&JobState::Completed {
            output_path: generated_pdf.clone(),
            total_pages,
            bytes: final_bytes,
        });

        Ok(generated_pdf)
    }
}

async fn download_single_candidate(
    client: &HttpClient,
    cache: &CacheManager,
    provider: &str,
    pub_id: &str,
    page_desc: &crate::core::document::PageDescriptor,
    candidate: &AssetCandidate,
) -> Result<CompletedPageAsset, DocDownloaderError> {
    let ext = candidate.asset_type.file_extension();
    let mut writer = cache.atomic_page_writer(provider, pub_id, page_desc.index, ext)?;

    let (bytes_downloaded, hash) = client
        .stream_to_atomic_file(
            &candidate.url,
            Some(&candidate.headers),
            &mut writer,
            MAX_PAGE_BYTE_LIMIT,
        )
        .await?;

    let meta = inspect_and_validate_asset(writer.temp_path(), page_desc.index)?;
    let final_path = writer.commit()?;

    let relative_path = match final_path.file_name() {
        Some(name) => format!("pages/{}", name.to_string_lossy()),
        None => format!("pages/page_{:04}.{ext}", page_desc.index),
    };

    Ok(CompletedPageAsset {
        page_index: page_desc.index,
        relative_path,
        sha256: hash,
        byte_size: bytes_downloaded,
        width: meta.width,
        height: meta.height,
        asset_type: candidate.asset_type,
    })
}
