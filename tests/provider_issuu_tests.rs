use docdownloader::providers::PublicationProvider;
use docdownloader::providers::issuu::IssuuProvider;
use docdownloader::providers::issuu::models::IssuuReaderManifest;
use docdownloader::providers::issuu::parser::{
    parse_issuu_reader_html, parse_issuu_reader_manifest,
};
use url::Url;

#[test]
fn test_issuu_provider_detection() {
    let provider = IssuuProvider::new();

    let valid_urls = [
        "https://issuu.com/company/docs/annual_report_2025",
        "https://www.issuu.com/company/docs/annual_report_2025",
        "https://issuu.com/company/docs/annual_report_2025?fr=sMWYyNjY0NzM0ODg",
        "https://e.issuu.com/embed.html?d=annual_report_2025&u=company",
        "https://e.issuu.com/anonymous-embed.html?u=company&d=annual_report_2025",
        "http://issuu.com/company/docs/annual_report_2025",
    ];

    for u in valid_urls {
        let url = Url::parse(u).expect("valid url");
        assert!(
            provider.can_handle(&url),
            "Expected IssuuProvider to handle '{u}'"
        );
        let id = provider.extract_id(&url).expect("extract id");
        assert_eq!(id, "company/annual_report_2025");
    }

    let invalid_urls = [
        "https://www.calameo.com/read/0061133461a5012e8961a",
        "https://www.slideshare.net/tech/presentation",
        "https://www.scribd.com/document/123456/sample",
        "https://issuu.com/",
        "https://issuu.com/pricing",
        "https://issuu.com/company", // Missing /docs/slug
    ];

    for u in invalid_urls {
        let url = Url::parse(u).expect("valid url");
        assert!(
            !provider.can_handle(&url),
            "Expected IssuuProvider to reject '{u}'"
        );
    }
}

#[test]
fn test_issuu_manifest_json_parsing() {
    let json_data = include_str!("fixtures/issuu_manifest_valid.json");
    let manifest: IssuuReaderManifest =
        serde_json::from_str(json_data).expect("deserialize fixture");

    let canonical_url = "https://issuu.com/acme/docs/quarterly_report";
    let publication =
        parse_issuu_reader_manifest(&manifest, "acme", "quarterly_report", canonical_url)
            .expect("parse manifest");

    assert_eq!(publication.provider, "issuu");
    assert_eq!(publication.publication_id, "acme/quarterly_report");
    assert_eq!(publication.title, "Sample Issuu Quarterly Report 2026");
    assert_eq!(publication.author.as_deref(), Some("acme"));
    assert_eq!(publication.page_count, 4);
    assert_eq!(publication.pages.len(), 4);

    // Verify page descriptors and candidate ordering
    for (idx, page) in publication.pages.iter().enumerate() {
        assert_eq!(page.index, (idx + 1) as u32);
        assert!(!page.candidates.is_empty());

        // Check geometry preservation
        assert_eq!(page.geometry.unwrap().width, 1200);
        assert_eq!(page.geometry.unwrap().height, 1600);

        // Check candidate priorities
        let best = page.best_candidate().unwrap();
        assert_eq!(best.priority, 0);
        assert!(best.url.contains(&format!("page_{}.jpg", idx + 1)));
    }
}

#[test]
fn test_issuu_restricted_access_rejection() {
    let json_data = include_str!("fixtures/issuu_manifest_private.json");
    let manifest: IssuuReaderManifest =
        serde_json::from_str(json_data).expect("deserialize fixture");

    let canonical_url = "https://issuu.com/acme/docs/confidential";
    let result = parse_issuu_reader_manifest(&manifest, "acme", "confidential", canonical_url);

    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        matches!(
            err,
            docdownloader::core::error::DocDownloaderError::AccessRestricted { .. }
        ),
        "Expected AccessRestricted error, got: {err:?}"
    );
}

#[test]
fn test_issuu_reader_fallback_html_parsing() {
    let html_content = include_str!("fixtures/issuu_reader_fallback.html");
    let canonical_url = "https://issuu.com/magazines/docs/sample_mag";

    let publication =
        parse_issuu_reader_html(html_content, "magazines", "sample_mag", canonical_url)
            .expect("parse html reader");

    assert_eq!(publication.provider, "issuu");
    assert_eq!(publication.publication_id, "magazines/sample_mag");
    assert_eq!(publication.title, "Sample Issuu Fallback Magazine Title");
    assert_eq!(publication.page_count, 6);
    assert_eq!(publication.pages.len(), 6);

    for (idx, page) in publication.pages.iter().enumerate() {
        assert_eq!(page.index, (idx + 1) as u32);
        assert!(!page.candidates.is_empty());
        // Verify canonical CDN candidate is generated
        assert!(page.candidates.iter().any(|c| {
            c.url
                .contains("image.isu.pub/240822153012-1234567890abcdef1234567890abcdef")
        }));
    }
}
