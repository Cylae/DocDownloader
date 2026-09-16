use docdownloader::core::document::{AssetType, Publication};
use docdownloader::core::job::CompletedPageAsset;
use docdownloader::pdf::builder::PdfBuilder;
use docdownloader::pdf::validator::validate_pdf_document;
use image::{ImageBuffer, Rgb};
use std::io::Cursor;
use tempfile::tempdir;

fn generate_test_jpeg(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(width, height, |_, _| Rgb([r, g, b]));
    let mut bytes = Cursor::new(Vec::new());
    img.write_to(&mut bytes, image::ImageFormat::Jpeg)
        .expect("encode jpeg");
    bytes.into_inner()
}

#[test]
fn test_pdf_single_and_multi_page_assembly() {
    let temp = tempdir().expect("tempdir");
    let out_pdf = temp.path().join("test_output.pdf");

    let p1 = temp.path().join("page_1.jpg");
    let p2 = temp.path().join("page_2.jpg");

    // Page 1: Portrait 800x1200
    let img1_data = generate_test_jpeg(800, 1200, 200, 50, 50);
    std::fs::write(&p1, &img1_data).expect("write p1");

    // Page 2: Landscape 1200x800
    let img2_data = generate_test_jpeg(1200, 800, 50, 150, 200);
    std::fs::write(&p2, &img2_data).expect("write p2");

    let publication = Publication {
        provider: "test".to_string(),
        canonical_url: "https://example.com/doc".to_string(),
        publication_id: "doc123".to_string(),
        title: "Test PDF Title".to_string(),
        author: Some("Test Author".to_string()),
        description: Some("Test Subject".to_string()),
        page_count: 2,
        thumbnail_url: None,
        geometry: None,
        direct_pdf_url: None,
        pages: Vec::new(),
    };

    let builder = PdfBuilder::new(&publication);

    let completed_pages = vec![
        CompletedPageAsset {
            page_index: 1,
            relative_path: "page_1.jpg".to_string(),
            sha256: "dummy1".to_string(),
            byte_size: img1_data.len() as u64,
            width: 800,
            height: 1200,
            asset_type: AssetType::ImageJpeg,
        },
        CompletedPageAsset {
            page_index: 2,
            relative_path: "page_2.jpg".to_string(),
            sha256: "dummy2".to_string(),
            byte_size: img2_data.len() as u64,
            width: 1200,
            height: 800,
            asset_type: AssetType::ImageJpeg,
        },
    ];

    builder
        .build(&out_pdf, temp.path(), &completed_pages)
        .expect("assemble pdf");
    assert!(out_pdf.exists(), "Output PDF must exist");

    // Independently validate PDF structure and page count
    validate_pdf_document(&out_pdf, 2).expect("validation must succeed");
}

#[test]
fn test_pdf_validation_fails_on_page_count_mismatch() {
    let temp = tempdir().expect("tempdir");
    let out_pdf = temp.path().join("single_page.pdf");
    let p1 = temp.path().join("p1.jpg");

    let img_data = generate_test_jpeg(600, 800, 100, 100, 100);
    std::fs::write(&p1, &img_data).expect("write p1");

    let publication = Publication {
        provider: "test".to_string(),
        canonical_url: "https://example.com/single".to_string(),
        publication_id: "single".to_string(),
        title: "Single Page".to_string(),
        author: None,
        description: None,
        page_count: 1,
        thumbnail_url: None,
        geometry: None,
        direct_pdf_url: None,
        pages: Vec::new(),
    };

    let builder = PdfBuilder::new(&publication);
    let completed_pages = vec![CompletedPageAsset {
        page_index: 1,
        relative_path: "p1.jpg".to_string(),
        sha256: "dummy".to_string(),
        byte_size: img_data.len() as u64,
        width: 600,
        height: 800,
        asset_type: AssetType::ImageJpeg,
    }];

    builder
        .build(&out_pdf, temp.path(), &completed_pages)
        .expect("assemble pdf");

    // Expecting 5 pages when only 1 exists must fail validation
    let validation_result = validate_pdf_document(&out_pdf, 5);
    assert!(
        validation_result.is_err(),
        "Validation must fail if actual page count does not match expected"
    );
}
