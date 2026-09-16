use docdownloader::core::document::{
    AssetCandidate, AssetType, PageDescriptor, PageGeometry, Publication,
};
use docdownloader::core::engine::{DownloadEngine, ProgressListener};
use docdownloader::core::job::{CompletedPageAsset, JobManifest, JobState};
use docdownloader::network::client::HttpClient;
use docdownloader::pdf::validator::validate_pdf_document;
use docdownloader::providers::{ProviderRegistry, PublicationProvider};
use docdownloader::storage::cache::CacheManager;
use image::{ImageBuffer, Rgb};
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::watch;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn generate_test_jpeg(width: u32, height: u32) -> Vec<u8> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(width, height, |_, _| Rgb([100, 150, 200]));
    let mut bytes = Cursor::new(Vec::new());
    img.write_to(&mut bytes, image::ImageFormat::Jpeg)
        .expect("encode jpeg");
    bytes.into_inner()
}

struct CountingListener {
    cache_hits: AtomicUsize,
    downloads: AtomicUsize,
}

impl ProgressListener for CountingListener {
    fn on_state_change(&self, _state: &JobState) {}
    fn on_page_completed(
        &self,
        _page_index: u32,
        _total_pages: u32,
        _bytes: u64,
        from_cache: bool,
    ) {
        if from_cache {
            self.cache_hits.fetch_add(1, Ordering::SeqCst);
        } else {
            self.downloads.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn on_log_message(&self, _message: &str) {}
}

struct TestProvider {
    publication: Publication,
}

#[async_trait::async_trait]
impl PublicationProvider for TestProvider {
    fn name(&self) -> &'static str {
        "test"
    }
    fn display_name(&self) -> &'static str {
        "Test"
    }
    fn can_handle(&self, _url: &Url) -> bool {
        true
    }
    fn extract_id(
        &self,
        _url: &Url,
    ) -> Result<String, docdownloader::core::error::DocDownloaderError> {
        Ok(self.publication.publication_id.clone())
    }
    async fn resolve(
        &self,
        _client: &HttpClient,
        _url: &Url,
    ) -> Result<Publication, docdownloader::core::error::DocDownloaderError> {
        Ok(self.publication.clone())
    }
}

#[tokio::test]
async fn test_resume_interrupted_download_reuses_valid_cache() {
    let mock_server = MockServer::start().await;
    let temp = tempdir().expect("tempdir");
    let cache = CacheManager::new(temp.path().join("cache"));
    let output_pdf = temp.path().join("resumed_output.pdf");

    let p1_bytes = generate_test_jpeg(600, 800);
    let p2_bytes = generate_test_jpeg(600, 800);
    let p3_bytes = generate_test_jpeg(600, 800);

    // Serve page 3 from mock server
    Mock::given(method("GET"))
        .and(path("/p3.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(p3_bytes.clone()))
        .mount(&mock_server)
        .await;

    // Pre-populate cache with page 1 and page 2
    let pub_dir = cache.publication_dir("test", "pub_resume_01");
    let pages_dir = pub_dir.join("pages");
    std::fs::create_dir_all(&pages_dir).unwrap();

    let p1_path = pages_dir.join("page_0001.jpg");
    let p2_path = pages_dir.join("page_0002.jpg");
    std::fs::write(&p1_path, &p1_bytes).unwrap();
    std::fs::write(&p2_path, &p2_bytes).unwrap();

    let manifest_path = cache.manifest_path("test", "pub_resume_01");
    let mut manifest = JobManifest::new(
        "test",
        "pub_resume_01",
        "https://example.com/read/pub_resume_01",
        "Resumed Document",
        3,
    );

    manifest.record_completed_page(CompletedPageAsset {
        page_index: 1,
        relative_path: "pages/page_0001.jpg".to_string(),
        sha256: "hash1".to_string(),
        byte_size: p1_bytes.len() as u64,
        width: 600,
        height: 800,
        asset_type: AssetType::ImageJpeg,
    });
    manifest.record_completed_page(CompletedPageAsset {
        page_index: 2,
        relative_path: "pages/page_0002.jpg".to_string(),
        sha256: "hash2".to_string(),
        byte_size: p2_bytes.len() as u64,
        width: 600,
        height: 800,
        asset_type: AssetType::ImageJpeg,
    });
    manifest.save_to_file(&manifest_path).unwrap();

    // Create publication definition
    let mut pages = Vec::new();
    for i in 1..=3 {
        let mut p = PageDescriptor::new(i);
        p.candidates.push(AssetCandidate {
            priority: 1,
            url: format!("{}/p{i}.jpg", mock_server.uri()),
            asset_type: AssetType::ImageJpeg,
            geometry: Some(PageGeometry::new(600, 800)),
            headers: Vec::new(),
        });
        pages.push(p);
    }

    let publication = Publication {
        provider: "test".to_string(),
        canonical_url: "https://example.com/read/pub_resume_01".to_string(),
        publication_id: "pub_resume_01".to_string(),
        title: "Resumed Document".to_string(),
        author: None,
        description: None,
        page_count: 3,
        thumbnail_url: None,
        geometry: Some(PageGeometry::new(600, 800)),
        direct_pdf_url: None,
        pages,
    };

    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(TestProvider { publication }));

    let client = HttpClient::new_test_client().unwrap();
    let engine = DownloadEngine::new(client, Arc::new(registry), cache, 2, true);

    let listener = Arc::new(CountingListener {
        cache_hits: AtomicUsize::new(0),
        downloads: AtomicUsize::new(0),
    });
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    let doc_url = Url::parse("https://example.com/read/pub_resume_01").unwrap();
    let result = engine
        .download(&doc_url, Some(&output_pdf), listener.clone(), cancel_rx)
        .await
        .expect("download resume");

    assert!(result.exists());
    assert_eq!(
        listener.cache_hits.load(Ordering::SeqCst),
        2,
        "Must reuse 2 cached pages"
    );
    assert_eq!(
        listener.downloads.load(Ordering::SeqCst),
        1,
        "Must only download 1 missing page"
    );

    validate_pdf_document(&result, 3).expect("validated 3-page PDF");
}

#[tokio::test]
async fn test_cache_corruption_detection_and_recovery() {
    let mock_server = MockServer::start().await;
    let temp = tempdir().expect("tempdir");
    let cache = CacheManager::new(temp.path().join("cache"));
    let output_pdf = temp.path().join("recovered_output.pdf");

    let p1_bytes = generate_test_jpeg(600, 800);
    let p2_bytes = generate_test_jpeg(600, 800);

    // Mock server serves valid page 2
    Mock::given(method("GET"))
        .and(path("/p2.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(p2_bytes.clone()))
        .mount(&mock_server)
        .await;

    // Pre-populate cache: page 1 valid, page 2 corrupted with garbage bytes
    let pub_dir = cache.publication_dir("test", "pub_corrupt_02");
    let pages_dir = pub_dir.join("pages");
    std::fs::create_dir_all(&pages_dir).unwrap();

    let p1_path = pages_dir.join("page_0001.jpg");
    let p2_path = pages_dir.join("page_0002.jpg");
    std::fs::write(&p1_path, &p1_bytes).unwrap();
    std::fs::write(&p2_path, b"CORRUPTED_GARBAGE_DATA_NOT_A_JPEG").unwrap();

    let manifest_path = cache.manifest_path("test", "pub_corrupt_02");
    let mut manifest = JobManifest::new(
        "test",
        "pub_corrupt_02",
        "https://example.com/read/pub_corrupt_02",
        "Recovered Document",
        2,
    );

    manifest.record_completed_page(CompletedPageAsset {
        page_index: 1,
        relative_path: "pages/page_0001.jpg".to_string(),
        sha256: "hash1".to_string(),
        byte_size: p1_bytes.len() as u64,
        width: 600,
        height: 800,
        asset_type: AssetType::ImageJpeg,
    });
    manifest.record_completed_page(CompletedPageAsset {
        page_index: 2,
        relative_path: "pages/page_0002.jpg".to_string(),
        sha256: "corrupt_hash".to_string(),
        byte_size: 34,
        width: 600,
        height: 800,
        asset_type: AssetType::ImageJpeg,
    });
    manifest.save_to_file(&manifest_path).unwrap();

    let mut pages = Vec::new();
    for i in 1..=2 {
        let mut p = PageDescriptor::new(i);
        p.candidates.push(AssetCandidate {
            priority: 1,
            url: format!("{}/p{i}.jpg", mock_server.uri()),
            asset_type: AssetType::ImageJpeg,
            geometry: Some(PageGeometry::new(600, 800)),
            headers: Vec::new(),
        });
        pages.push(p);
    }

    let publication = Publication {
        provider: "test".to_string(),
        canonical_url: "https://example.com/read/pub_corrupt_02".to_string(),
        publication_id: "pub_corrupt_02".to_string(),
        title: "Recovered Document".to_string(),
        author: None,
        description: None,
        page_count: 2,
        thumbnail_url: None,
        geometry: Some(PageGeometry::new(600, 800)),
        direct_pdf_url: None,
        pages,
    };

    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(TestProvider { publication }));

    let client = HttpClient::new_test_client().unwrap();
    let engine = DownloadEngine::new(client, Arc::new(registry), cache, 2, true);

    let listener = Arc::new(CountingListener {
        cache_hits: AtomicUsize::new(0),
        downloads: AtomicUsize::new(0),
    });
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    let doc_url = Url::parse("https://example.com/read/pub_corrupt_02").unwrap();
    let result = engine
        .download(&doc_url, Some(&output_pdf), listener.clone(), cancel_rx)
        .await
        .expect("download recovery");

    assert!(result.exists());
    // Page 1 was valid -> 1 cache hit
    assert_eq!(
        listener.cache_hits.load(Ordering::SeqCst),
        1,
        "Page 1 must be reused from cache"
    );
    // Page 2 was corrupt -> 1 re-download
    assert_eq!(
        listener.downloads.load(Ordering::SeqCst),
        1,
        "Page 2 must be re-downloaded to recover"
    );

    validate_pdf_document(&result, 2).expect("validated 2-page PDF");
}
