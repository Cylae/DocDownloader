use docdownloader::core::document::AssetType;
use docdownloader::core::job::CompletedPageAsset;
use docdownloader::core::Publication;
use docdownloader::pdf::PdfBuilder;
use std::time::Instant;

fn main() {
    println!("=== DocDownloader Synthetic Benchmarks ===");

    let temp_dir = tempfile::tempdir().expect("Failed to create temporary dir");
    let cache_dir = temp_dir.path().join("cache");
    let pages_dir = cache_dir.join("pages");
    std::fs::create_dir_all(&pages_dir).expect("Failed to create pages dir");

    // Generate a synthetic test JPEG image (100x150)
    let img = image::RgbImage::new(100, 150);
    let sample_jpeg_path = pages_dir.join("page_0001.jpg");
    img.save_with_format(&sample_jpeg_path, image::ImageFormat::Jpeg)
        .expect("Failed to save synthetic JPEG");

    let sample_jpeg_len = std::fs::metadata(&sample_jpeg_path).unwrap().len();

    // Run benchmarks for 10, 100, 500 pages
    for count in [10, 100, 500] {
        let mut completed_pages = Vec::with_capacity(count);
        for i in 1..=count {
            let page_filename = format!("page_{i:04}.jpg");
            let page_path = pages_dir.join(&page_filename);
            if !page_path.exists() {
                std::fs::copy(&sample_jpeg_path, &page_path)
                    .expect("Failed to copy synthetic page");
            }
            completed_pages.push(CompletedPageAsset {
                page_index: i as u32,
                relative_path: format!("pages/{page_filename}"),
                sha256: "fakehash".to_string(),
                byte_size: sample_jpeg_len,
                width: 100,
                height: 150,
                asset_type: AssetType::ImageJpeg,
            });
        }

        let publication = Publication {
            provider: "synthetic".to_string(),
            canonical_url: "https://example.com/synth".to_string(),
            publication_id: "synth".to_string(),
            title: format!("Synthetic Publication ({count} pages)"),
            author: Some("Benchmark".to_string()),
            description: None,
            page_count: count as u32,
            thumbnail_url: None,
            geometry: None,
            direct_pdf_url: None,
            pages: Vec::new(),
        };

        let builder = PdfBuilder::new(&publication);
        let out_pdf = temp_dir.path().join(format!("output_{count}.pdf"));

        let start = Instant::now();
        let final_path = builder
            .build(&out_pdf, &cache_dir, &completed_pages)
            .expect("Failed to build benchmark PDF");
        let elapsed = start.elapsed();

        let pdf_size = std::fs::metadata(&final_path).unwrap().len();
        println!(
            "-> {count:3} pages: {:?} | Generated PDF: {:.2} KB ({:.2} pages/sec)",
            elapsed,
            pdf_size as f64 / 1024.0,
            count as f64 / elapsed.as_secs_f64()
        );
    }

    println!("=== Benchmarks Completed Successfully ===");
}
