use docdownloader::core::document::{
    AssetCandidate, AssetType, PageDescriptor, PageGeometry, Publication,
};
use docdownloader::core::error::DocDownloaderError;
use docdownloader::core::job::CompletedPageAsset;
use docdownloader::network::retry::parse_retry_after;
use docdownloader::network::security::validate_url_security;
use docdownloader::pdf::builder::PdfBuilder;
use docdownloader::pdf::image::inspect_and_validate_asset;
use docdownloader::pdf::validator::validate_pdf_document;
use docdownloader::storage::atomic::AtomicFileWriter;
use image::{ImageBuffer, Rgb};
use std::io::Cursor;
use std::time::Duration;
use tempfile::tempdir;
use url::Url;

fn generate_test_jpeg(width: u32, height: u32) -> Vec<u8> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(width, height, |_, _| Rgb([128, 128, 128]));
    let mut bytes = Cursor::new(Vec::new());
    img.write_to(&mut bytes, image::ImageFormat::Jpeg)
        .expect("encode jpeg");
    bytes.into_inner()
}

/// 1. Prove that page 1 (and every page) is accounted for and never skipped or omitted.
#[test]
fn regression_page_001_not_skipped() {
    let page_count = 5;
    let mut pages = Vec::new();
    for i in 1..=page_count {
        let mut p = PageDescriptor::new(i);
        p.candidates.push(AssetCandidate {
            priority: 1,
            url: format!("https://ps.calameoassets.com/key/p{i}.jpg"),
            asset_type: AssetType::ImageJpeg,
            geometry: Some(PageGeometry::new(800, 1200)),
            headers: Vec::new(),
        });
        pages.push(p);
    }

    let publication = Publication {
        provider: "calameo".to_string(),
        canonical_url: "https://www.calameo.com/read/0061133461a5012e8961a".to_string(),
        publication_id: "0061133461a5012e8961a".to_string(),
        title: "Completeness Test".to_string(),
        author: None,
        description: None,
        page_count,
        thumbnail_url: None,
        geometry: None,
        direct_pdf_url: None,
        pages,
    };

    assert!(publication.validate_completeness().is_ok());
    assert_eq!(publication.pages[0].index, 1, "Page 1 must not be skipped");
    assert_eq!(publication.pages.len(), 5);

    // Simulate page 1 missing (pages 2..=5)
    let mut bad_pages = publication.pages.clone();
    bad_pages.remove(0);
    let mut bad_pub = publication.clone();
    bad_pub.pages = bad_pages;
    assert!(
        bad_pub.validate_completeness().is_err(),
        "Publication missing page 1 must fail completeness validation"
    );
}

/// 2. Prove that 0-based or disordered page numbering is strictly caught and rejected.
#[test]
fn regression_manifest_001_zero_based_order() {
    let mut pages = Vec::new();
    // Intentionally 0-based indexing: 0, 1, 2
    for i in 0..3 {
        let mut p = PageDescriptor::new(i);
        p.candidates.push(AssetCandidate {
            priority: 1,
            url: format!("https://ps.calameoassets.com/key/p{i}.jpg"),
            asset_type: AssetType::ImageJpeg,
            geometry: None,
            headers: Vec::new(),
        });
        pages.push(p);
    }

    let publication = Publication {
        provider: "calameo".to_string(),
        canonical_url: "https://www.calameo.com/read/0061133461a5012e8961a".to_string(),
        publication_id: "0061133461a5012e8961a".to_string(),
        title: "Zero-based order test".to_string(),
        author: None,
        description: None,
        page_count: 3,
        thumbnail_url: None,
        geometry: None,
        direct_pdf_url: None,
        pages,
    };

    let result = publication.validate_completeness();
    assert!(
        result.is_err(),
        "0-based indexing must fail validation (1-based [1..=N] is mandatory)"
    );
    let err = result.unwrap_err();
    assert!(err.contains("expected page index 1, got 0"));
}

/// 3. Prove that an HTTP 200 response containing an HTML or JSON error page is flagged as corrupt.
#[test]
fn regression_http_001_html_200_not_image() {
    let temp = tempdir().expect("tempdir");
    let html_path = temp.path().join("fake_image.jpg");
    std::fs::write(
        &html_path,
        b"<!DOCTYPE html><html><body><h1>404 Not Found</h1></body></html>",
    )
    .expect("write html");

    let result = inspect_and_validate_asset(&html_path, 1);
    assert!(result.is_err());
    match result.unwrap_err() {
        DocDownloaderError::PageCorrupt { page_index, reason } => {
            assert_eq!(page_index, 1);
            assert!(reason.contains("HTML/JSON error document disguised as an image"));
        }
        other => panic!("Expected PageCorrupt, got: {other:?}"),
    }

    let json_path = temp.path().join("fake_image_json.jpg");
    std::fs::write(&json_path, b"{\"status\": \"error\", \"code\": 403}").expect("write json");
    let json_result = inspect_and_validate_asset(&json_path, 2);
    assert!(json_result.is_err());
    assert!(matches!(
        json_result.unwrap_err(),
        DocDownloaderError::PageCorrupt { .. }
    ));
}

/// 4. Prove that HTTP 429 Retry-After parsing correctly extracts seconds.
#[test]
fn regression_retry_001_429_retry_after() {
    assert_eq!(parse_retry_after("12"), Some(Duration::from_secs(12)));
    assert_eq!(parse_retry_after("0"), Some(Duration::from_secs(0)));
    assert_eq!(parse_retry_after("120"), Some(Duration::from_secs(120)));
    assert_eq!(parse_retry_after("invalid_header"), None);
}

/// 5. Prove that landscape pages maintain their exact aspect ratio without distortion or rotation.
#[test]
fn regression_pdf_001_landscape_ratio() {
    let temp = tempdir().expect("tempdir");
    let pdf_path = temp.path().join("landscape.pdf");
    let page_path = temp.path().join("page_landscape.jpg");

    // 16:9 ratio: 1600x900
    let img_data = generate_test_jpeg(1600, 900);
    std::fs::write(&page_path, &img_data).expect("write img");

    let publication = Publication {
        provider: "test".to_string(),
        canonical_url: "https://example.com/landscape".to_string(),
        publication_id: "landscape".to_string(),
        title: "Landscape Document".to_string(),
        author: None,
        description: None,
        page_count: 1,
        thumbnail_url: None,
        geometry: Some(PageGeometry::new(1600, 900)),
        direct_pdf_url: None,
        pages: Vec::new(),
    };

    let builder = PdfBuilder::new(&publication);
    let pages = vec![CompletedPageAsset {
        page_index: 1,
        relative_path: "page_landscape.jpg".to_string(),
        sha256: "hash".to_string(),
        byte_size: img_data.len() as u64,
        width: 1600,
        height: 900,
        asset_type: AssetType::ImageJpeg,
    }];

    builder
        .build(&pdf_path, temp.path(), &pages)
        .expect("build pdf");
    assert!(pdf_path.exists());

    validate_pdf_document(&pdf_path, 1).expect("pdf validation");

    // Inspect MediaBox in generated PDF directly
    let doc = lopdf::Document::load(&pdf_path).expect("load pdf");
    let pages_dict = doc.get_pages();
    let (_, page_id) = pages_dict.iter().next().unwrap();
    let page_obj = doc.get_dictionary(*page_id).expect("page dict");
    let media_box = page_obj
        .get(b"MediaBox")
        .and_then(|o| o.as_array())
        .expect("mediabox");

    let w = match &media_box[2] {
        lopdf::Object::Real(r) => *r as f64,
        lopdf::Object::Integer(i) => *i as f64,
        _ => 0.0,
    };
    let h = match &media_box[3] {
        lopdf::Object::Real(r) => *r as f64,
        lopdf::Object::Integer(i) => *i as f64,
        _ => 0.0,
    };

    assert_eq!(w, 1600.0);
    assert_eq!(h, 900.0);
    let aspect_ratio = w / h;
    assert!(
        (aspect_ratio - (16.0 / 9.0)).abs() < 1e-4,
        "Generated PDF page must preserve exact 16:9 source aspect ratio"
    );
}

/// 6. Prove that partial/interrupted PDF files are not exposed at the destination path.
#[test]
fn regression_fs_001_partial_output_not_exposed() {
    let temp = tempdir().expect("tempdir");
    let target_pdf = temp.path().join("final_document.pdf");

    // Start an atomic write but abort / drop it without committing
    {
        let mut writer = AtomicFileWriter::new(&target_pdf).expect("create atomic writer");
        writer
            .write_all(b"%PDF-partial-corrupt-data")
            .expect("write bytes");
        // writer drops here without commit()
    }

    assert!(
        !target_pdf.exists(),
        "Final PDF destination must NOT exist if writer was not committed"
    );

    // Verify that the temporary .part file was cleaned up on drop
    let parent_entries = std::fs::read_dir(temp.path()).expect("read dir");
    for entry in parent_entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.ends_with(".part"),
            "Leftover .part file must not remain after uncommitted drop: {name}"
        );
    }
}

/// 7. Prove that SSRF targets and redirects to forbidden hosts are blocked.
#[test]
fn regression_security_001_redirect_ssrf_blocked() {
    let forbidden = [
        "http://127.0.0.1/admin",
        "http://127.0.0.1:8080/metrics",
        "http://localhost/secret",
        "http://10.0.0.1/private",
        "http://172.16.0.1/internal",
        "http://192.168.1.1/gateway",
        "http://169.254.169.254/latest/meta-data/",
        "http://[::1]/status",
        "file:///C:/Windows/System32/drivers/etc/hosts",
    ];

    for u in forbidden {
        let parsed = Url::parse(u).expect("valid url syntax");
        let res = validate_url_security(&parsed);
        assert!(res.is_err(), "Expected SSRF security check to block '{u}'");
    }
}
