use docdownloader::core::job::JobManifest;
use docdownloader::core::job::JobState;
use docdownloader::network::retry::RetryPolicy;
use docdownloader::storage::cache::CacheManager;
use sha2::Digest;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn test_manifest_roundtrip_serialization() {
    let manifest = JobManifest::new(
        "calameo",
        "0061133461a5012e8961a",
        "https://www.calameo.com/read/0061133461a5012e8961a",
        "Test Publication Title",
        4,
    );

    let serialized = serde_json::to_string_pretty(&manifest).expect("serialize");
    let deserialized: JobManifest = serde_json::from_str(&serialized).expect("deserialize");

    assert_eq!(deserialized.provider, manifest.provider);
    assert_eq!(deserialized.publication_id, manifest.publication_id);
    assert_eq!(deserialized.canonical_url, manifest.canonical_url);
    assert_eq!(deserialized.title, manifest.title);
    assert_eq!(deserialized.expected_pages, 4);
}

#[test]
fn test_publication_cache_integrity_and_validation() {
    let temp = tempdir().expect("tempdir");
    let cache = CacheManager::new(temp.path().to_path_buf());

    let page_path = cache.page_asset_path("calameo", "book123", 1, "jpg");
    std::fs::create_dir_all(page_path.parent().unwrap()).unwrap();
    let test_bytes = b"JPEG_TEST_VALID_DATA_CONTENT";
    std::fs::write(&page_path, test_bytes).unwrap();

    assert!(page_path.exists());
    let computed_hash = hex::encode(sha2::Sha256::digest(test_bytes));
    let read_bytes = std::fs::read(&page_path).unwrap();
    assert_eq!(
        computed_hash,
        hex::encode(sha2::Sha256::digest(&read_bytes))
    );
}

#[test]
fn test_retry_policy_exponential_backoff() {
    let policy = RetryPolicy {
        max_retries: 4,
        initial_delay: Duration::from_millis(50),
        max_delay: Duration::from_millis(500),
        jitter_factor: 0.1,
    };

    let d1 = policy.delay_for_attempt(1, None);
    let d2 = policy.delay_for_attempt(2, None);
    let d3 = policy.delay_for_attempt(3, None);

    assert!(d1 >= Duration::from_millis(45) && d1 <= Duration::from_millis(55));
    assert!(d2 >= Duration::from_millis(90) && d2 <= Duration::from_millis(110));
    assert!(d3 >= Duration::from_millis(180) && d3 <= Duration::from_millis(220));
}

#[test]
fn test_job_state_descriptions() {
    assert_eq!(JobState::Queued.description(), "Queued");
    assert_eq!(JobState::ValidatingUrl.description(), "Validating URL");
    assert_eq!(
        JobState::ResolvingPages.description(),
        "Resolving page manifest"
    );
    assert_eq!(
        JobState::Completed {
            output_path: std::path::PathBuf::from("test.pdf"),
            total_pages: 10,
            bytes: 1024,
        }
        .description(),
        "Completed successfully"
    );
}

#[tokio::test]
async fn test_direct_pdf_optimization_and_fallback() {
    use docdownloader::core::document::{
        AssetCandidate, AssetType, PageDescriptor, PageGeometry, Publication,
    };
    use docdownloader::core::engine::{DownloadEngine, NoopProgressListener};
    use docdownloader::core::job::CompletedPageAsset;
    use docdownloader::network::client::HttpClient;
    use docdownloader::pdf::builder::PdfBuilder;
    use docdownloader::pdf::validator::validate_pdf_document;
    use docdownloader::providers::{ProviderRegistry, PublicationProvider};
    use image::{ImageBuffer, Rgb};
    use std::io::Cursor;
    use std::sync::Arc;
    use tokio::sync::watch;
    use url::Url;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;
    let temp = tempdir().expect("tempdir");
    let cache = CacheManager::new(temp.path().join("cache"));
    let direct_pdf_output = temp.path().join("direct_output.pdf");

    // Generate a valid 2-page PDF to serve as direct PDF
    let img_data = {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(400, 600, |_, _| Rgb([50, 100, 150]));
        let mut bytes = Cursor::new(Vec::new());
        img.write_to(&mut bytes, image::ImageFormat::Jpeg).unwrap();
        bytes.into_inner()
    };
    let p1_cache = temp.path().join("p1.jpg");
    let p2_cache = temp.path().join("p2.jpg");
    std::fs::write(&p1_cache, &img_data).unwrap();
    std::fs::write(&p2_cache, &img_data).unwrap();

    let dummy_pub = Publication {
        provider: "test".to_string(),
        canonical_url: "https://example.com/direct".to_string(),
        publication_id: "direct_doc".to_string(),
        title: "Direct Document".to_string(),
        author: None,
        description: None,
        page_count: 2,
        thumbnail_url: None,
        geometry: Some(PageGeometry::new(400, 600)),
        direct_pdf_url: None,
        pages: Vec::new(),
    };
    let builder = PdfBuilder::new(&dummy_pub);
    let sample_pdf_path = temp.path().join("sample_direct.pdf");
    builder
        .build(
            &sample_pdf_path,
            temp.path(),
            &[
                CompletedPageAsset {
                    page_index: 1,
                    relative_path: "p1.jpg".to_string(),
                    sha256: "h1".to_string(),
                    byte_size: img_data.len() as u64,
                    width: 400,
                    height: 600,
                    asset_type: AssetType::ImageJpeg,
                },
                CompletedPageAsset {
                    page_index: 2,
                    relative_path: "p2.jpg".to_string(),
                    sha256: "h2".to_string(),
                    byte_size: img_data.len() as u64,
                    width: 400,
                    height: 600,
                    asset_type: AssetType::ImageJpeg,
                },
            ],
        )
        .unwrap();

    let direct_pdf_bytes = std::fs::read(&sample_pdf_path).unwrap();

    // 1. Mock server serving direct PDF at /download.pdf
    Mock::given(method("GET"))
        .and(path("/download.pdf"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(direct_pdf_bytes))
        .mount(&mock_server)
        .await;

    struct DirectPdfProvider {
        pub_info: Publication,
    }

    #[async_trait::async_trait]
    impl PublicationProvider for DirectPdfProvider {
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
            Ok(self.pub_info.publication_id.clone())
        }
        async fn resolve(
            &self,
            _client: &HttpClient,
            _url: &Url,
        ) -> Result<Publication, docdownloader::core::error::DocDownloaderError> {
            Ok(self.pub_info.clone())
        }
    }

    let publication = Publication {
        provider: "test".to_string(),
        canonical_url: "https://example.com/direct".to_string(),
        publication_id: "direct_doc".to_string(),
        title: "Direct Document".to_string(),
        author: None,
        description: None,
        page_count: 2,
        thumbnail_url: None,
        geometry: Some(PageGeometry::new(400, 600)),
        direct_pdf_url: Some(format!("{}/download.pdf", mock_server.uri())),
        pages: vec![
            PageDescriptor {
                index: 1,
                geometry: Some(PageGeometry::new(400, 600)),
                candidates: vec![AssetCandidate {
                    priority: 1,
                    url: format!("{}/page1.jpg", mock_server.uri()),
                    asset_type: AssetType::ImageJpeg,
                    geometry: Some(PageGeometry::new(400, 600)),
                    headers: Vec::new(),
                }],
            },
            PageDescriptor {
                index: 2,
                geometry: Some(PageGeometry::new(400, 600)),
                candidates: vec![AssetCandidate {
                    priority: 1,
                    url: format!("{}/page2.jpg", mock_server.uri()),
                    asset_type: AssetType::ImageJpeg,
                    geometry: Some(PageGeometry::new(400, 600)),
                    headers: Vec::new(),
                }],
            },
        ],
    };

    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(DirectPdfProvider {
        pub_info: publication,
    }));

    let client = HttpClient::new_test_client().unwrap();
    let engine = DownloadEngine::new(client, Arc::new(registry), cache, 2, true);

    let (_cancel_tx, cancel_rx) = watch::channel(false);
    let doc_url = Url::parse("https://example.com/direct").unwrap();

    let result = engine
        .download(
            &doc_url,
            Some(&direct_pdf_output),
            Arc::new(NoopProgressListener),
            cancel_rx,
        )
        .await
        .expect("direct pdf download");

    assert!(result.exists());
    validate_pdf_document(&result, 2).expect("direct PDF passes 2-page validation");
}

#[test]
fn test_registry_dispatches_all_supported_providers() {
    use docdownloader::providers::ProviderRegistry;
    use url::Url;

    let registry = ProviderRegistry::new();

    let calameo_url = Url::parse("https://www.calameo.com/read/0061133461a5012e8961a").unwrap();
    let issuu_url = Url::parse("https://issuu.com/magazine/docs/spring2026").unwrap();
    let slideshare_url = Url::parse("https://www.slideshare.net/author/deck-slug").unwrap();
    let scribd_url = Url::parse("https://www.scribd.com/document/123456789/Title").unwrap();

    let calameo = registry.find_provider(&calameo_url).expect("calameo provider");
    assert_eq!(calameo.name(), "calameo");

    let issuu = registry.find_provider(&issuu_url).expect("issuu provider");
    assert_eq!(issuu.name(), "issuu");

    let slideshare = registry.find_provider(&slideshare_url).expect("slideshare provider");
    assert_eq!(slideshare.name(), "slideshare");

    let scribd = registry.find_provider(&scribd_url).expect("scribd provider");
    assert_eq!(scribd.name(), "scribd");

    let unknown_url = Url::parse("https://example.com/unsupported").unwrap();
    assert!(registry.find_provider(&unknown_url).is_none());
}
