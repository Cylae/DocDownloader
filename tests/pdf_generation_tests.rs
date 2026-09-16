use docdownloader::core::document::{AssetType, Publication};
use docdownloader::core::job::CompletedPageAsset;
use docdownloader::pdf::builder::PdfBuilder;
use docdownloader::pdf::validator::validate_pdf_document;
use image::{ImageBuffer, Rgb};
use lopdf::Document;
use std::io::Cursor;
use std::path::Path;
use tempfile::tempdir;

fn generate_test_jpeg(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(width, height, |_, _| Rgb([r, g, b]));
    let mut bytes = Cursor::new(Vec::new());
    img.write_to(&mut bytes, image::ImageFormat::Jpeg)
        .expect("encode jpeg");
    bytes.into_inner()
}

fn generate_test_png(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(width, height, |_, _| Rgb([r, g, b]));
    let mut bytes = Cursor::new(Vec::new());
    img.write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode png");
    bytes.into_inner()
}

fn extract_page_dimensions(pdf_path: &Path) -> Vec<(f64, f64)> {
    let doc = Document::load(pdf_path).expect("load pdf");
    let pages = doc.get_pages();
    let mut dims = Vec::new();
    for (_, page_id) in pages {
        let dict = doc.get_dictionary(page_id).expect("page dict");
        let media_box = dict
            .get(b"MediaBox")
            .expect("mediabox")
            .as_array()
            .expect("array");
        let extract_coord = |obj: &lopdf::Object| match obj {
            lopdf::Object::Integer(i) => *i as f64,
            lopdf::Object::Real(f) => *f as f64,
            _ => 0.0,
        };
        let w = extract_coord(&media_box[2]) - extract_coord(&media_box[0]);
        let h = extract_coord(&media_box[3]) - extract_coord(&media_box[1]);
        dims.push((w, h));
    }
    dims
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

#[test]
fn test_pdf_mixed_orientation_and_exact_geometry_preservation() {
    let temp = tempdir().expect("tempdir");
    let out_pdf = temp.path().join("mixed_orient.pdf");

    let p1 = temp.path().join("portrait.jpg");
    let p2 = temp.path().join("landscape.jpg");

    // Portrait 720x1080 (aspect ratio 0.6667)
    let p1_data = generate_test_jpeg(720, 1080, 255, 0, 0);
    std::fs::write(&p1, &p1_data).expect("write p1");

    // Landscape 1440x900 (aspect ratio 1.6)
    let p2_data = generate_test_jpeg(1440, 900, 0, 255, 0);
    std::fs::write(&p2, &p2_data).expect("write p2");

    let publication = Publication {
        provider: "test".to_string(),
        canonical_url: "https://example.com/mixed".to_string(),
        publication_id: "mixed".to_string(),
        title: "Mixed Orientation Document".to_string(),
        author: None,
        description: None,
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
            relative_path: "portrait.jpg".to_string(),
            sha256: "p1".to_string(),
            byte_size: p1_data.len() as u64,
            width: 720,
            height: 1080,
            asset_type: AssetType::ImageJpeg,
        },
        CompletedPageAsset {
            page_index: 2,
            relative_path: "landscape.jpg".to_string(),
            sha256: "p2".to_string(),
            byte_size: p2_data.len() as u64,
            width: 1440,
            height: 900,
            asset_type: AssetType::ImageJpeg,
        },
    ];

    builder
        .build(&out_pdf, temp.path(), &completed_pages)
        .expect("assemble");
    validate_pdf_document(&out_pdf, 2).expect("valid");

    let dims = extract_page_dimensions(&out_pdf);
    assert_eq!(dims.len(), 2);
    // Verify Page 1 exact portrait dimensions
    assert!((dims[0].0 - 720.0).abs() < 0.01);
    assert!((dims[0].1 - 1080.0).abs() < 0.01);
    assert!(dims[0].1 > dims[0].0, "Page 1 must be portrait");

    // Verify Page 2 exact landscape dimensions
    assert!((dims[1].0 - 1440.0).abs() < 0.01);
    assert!((dims[1].1 - 900.0).abs() < 0.01);
    assert!(dims[1].0 > dims[1].1, "Page 2 must be landscape");
}

#[test]
fn test_pdf_mixed_formats_jpeg_and_png() {
    let temp = tempdir().expect("tempdir");
    let out_pdf = temp.path().join("mixed_formats.pdf");

    let p1_path = temp.path().join("p1.jpg");
    let p2_path = temp.path().join("p2.png");

    let jpeg_data = generate_test_jpeg(500, 700, 10, 20, 30);
    let png_data = generate_test_png(600, 800, 40, 50, 60);

    std::fs::write(&p1_path, &jpeg_data).expect("write jpeg");
    std::fs::write(&p2_path, &png_data).expect("write png");

    let publication = Publication {
        provider: "test".to_string(),
        canonical_url: "https://example.com/fmt".to_string(),
        publication_id: "fmt".to_string(),
        title: "Mixed Formats Doc".to_string(),
        author: None,
        description: None,
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
            relative_path: "p1.jpg".to_string(),
            sha256: "h1".to_string(),
            byte_size: jpeg_data.len() as u64,
            width: 500,
            height: 700,
            asset_type: AssetType::ImageJpeg,
        },
        CompletedPageAsset {
            page_index: 2,
            relative_path: "p2.png".to_string(),
            sha256: "h2".to_string(),
            byte_size: png_data.len() as u64,
            width: 600,
            height: 800,
            asset_type: AssetType::ImagePng,
        },
    ];

    builder
        .build(&out_pdf, temp.path(), &completed_pages)
        .expect("assemble");
    validate_pdf_document(&out_pdf, 2).expect("valid");

    let dims = extract_page_dimensions(&out_pdf);
    assert_eq!(dims[0], (500.0, 700.0));
    assert_eq!(dims[1], (600.0, 800.0));
}

#[test]
fn test_pdf_unicode_metadata_and_long_title() {
    let temp = tempdir().expect("tempdir");
    let out_pdf = temp.path().join("unicode.pdf");

    let p1 = temp.path().join("p1.jpg");
    let data = generate_test_jpeg(400, 600, 100, 100, 100);
    std::fs::write(&p1, &data).expect("write");

    let long_title = format!(
        "Rapport Annuel 2026 — 日本語 / 🚀 / Тест — {}",
        "A".repeat(500)
    );
    let publication = Publication {
        provider: "test".to_string(),
        canonical_url: "https://example.com/unicode".to_string(),
        publication_id: "uni1".to_string(),
        title: long_title,
        author: Some("著者: Jean-François d'Aix".to_string()),
        description: Some("Subject with emojis 🌟 and symbols ©®™".to_string()),
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
        sha256: "h1".to_string(),
        byte_size: data.len() as u64,
        width: 400,
        height: 600,
        asset_type: AssetType::ImageJpeg,
    }];

    builder
        .build(&out_pdf, temp.path(), &completed_pages)
        .expect("assemble");
    validate_pdf_document(&out_pdf, 1).expect("valid");
}

#[test]
fn test_pdf_large_scale_100_pages() {
    let temp = tempdir().expect("tempdir");
    let out_pdf = temp.path().join("large_100.pdf");

    let shared_page_data = generate_test_jpeg(300, 450, 120, 140, 160);
    let p_path = temp.path().join("shared.jpg");
    std::fs::write(&p_path, &shared_page_data).expect("write shared page");

    let count = 100;
    let mut completed_pages = Vec::with_capacity(count);
    for idx in 1..=count as u32 {
        completed_pages.push(CompletedPageAsset {
            page_index: idx,
            relative_path: "shared.jpg".to_string(),
            sha256: "shared_hash".to_string(),
            byte_size: shared_page_data.len() as u64,
            width: 300,
            height: 450,
            asset_type: AssetType::ImageJpeg,
        });
    }

    let publication = Publication {
        provider: "test".to_string(),
        canonical_url: "https://example.com/100p".to_string(),
        publication_id: "p100".to_string(),
        title: "100 Pages Publication".to_string(),
        author: Some("Scale Tester".to_string()),
        description: None,
        page_count: count as u32,
        thumbnail_url: None,
        geometry: None,
        direct_pdf_url: None,
        pages: Vec::new(),
    };

    let builder = PdfBuilder::new(&publication);
    builder
        .build(&out_pdf, temp.path(), &completed_pages)
        .expect("assemble 100 pages");
    validate_pdf_document(&out_pdf, count as u32).expect("must validate exactly 100 pages");

    let dims = extract_page_dimensions(&out_pdf);
    assert_eq!(dims.len(), 100);
    assert_eq!(dims[0], (300.0, 450.0));
    assert_eq!(dims[99], (300.0, 450.0));
}
