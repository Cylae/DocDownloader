use docdownloader::providers::calameo::models::CalameoResponse;
use docdownloader::providers::calameo::parser::{
    parse_calameo_book_response, parse_calameo_reader_html,
};
use docdownloader::providers::calameo::signature::sign_calameo_asset_url;
use docdownloader::providers::calameo::CalameoProvider;
use docdownloader::providers::PublicationProvider;
use url::Url;

#[test]
fn test_calameo_provider_detection() {
    let provider = CalameoProvider::new();

    let valid_urls = [
        "https://www.calameo.com/read/0061133461a5012e8961a",
        "https://calameo.com/read/0061133461a5012e8961a",
        "https://www.calameo.com/books/0061133461a5012e8961a",
        "https://en.calameo.com/read/0061133461a5012e8961a?authid=abc123xyz",
        "http://www.calameo.com/read/0061133461a5012e8961a",
    ];

    for u in valid_urls {
        let url = Url::parse(u).expect("valid url");
        assert!(
            provider.can_handle(&url),
            "Expected CalameoProvider to handle '{u}'"
        );
    }

    let invalid_urls = [
        "https://issuu.com/example/docs/publication",
        "https://www.slideshare.net/example/presentation",
        "https://google.com/",
        "https://www.calameo.com/about", // Not a book or read URL
        "https://www.calameo.com/read/invalid_short_code",
    ];

    for u in invalid_urls {
        let url = Url::parse(u).expect("valid url");
        assert!(
            !provider.can_handle(&url),
            "Expected CalameoProvider to reject '{u}'"
        );
    }
}

#[test]
fn test_calameo_metadata_json_parsing() {
    let json_data = include_str!("fixtures/calameo_book_valid.json");
    let resp: CalameoResponse = serde_json::from_str(json_data).expect("deserialize fixture");

    let canonical_url = "https://www.calameo.com/read/0061133461a5012e8961a";
    let publication =
        parse_calameo_book_response(&resp, "0061133461a5012e8961a", canonical_url, None)
            .expect("parse metadata");

    assert_eq!(publication.provider, "calameo");
    assert_eq!(publication.publication_id, "0061133461a5012e8961a");
    assert_eq!(
        publication.title,
        "Synthetic Test Publication - Corporate Report 2025"
    );
    assert_eq!(publication.author.as_deref(), Some("Acme Corporation"));
    assert_eq!(publication.page_count, 4);
    assert_eq!(publication.pages.len(), 4);

    // Verify deterministic ordering of pages (1-based index)
    for (idx, page) in publication.pages.iter().enumerate() {
        assert_eq!(page.index, (idx + 1) as u32);
        assert!(
            !page.candidates.is_empty(),
            "Page must have candidate assets"
        );
        // Best quality should be prioritized first
        assert_eq!(page.candidates[0].geometry.as_ref().unwrap().width, 1125);
        assert_eq!(page.candidates[0].geometry.as_ref().unwrap().height, 1591);
    }
}

#[test]
fn test_calameo_restricted_access_rejection() {
    let json_data = include_str!("fixtures/calameo_book_private.json");
    let resp: CalameoResponse = serde_json::from_str(json_data).expect("deserialize fixture");

    let canonical_url = "https://www.calameo.com/read/0061133461a5012e8961a";
    let result = parse_calameo_book_response(&resp, "0061133461a5012e8961a", canonical_url, None);

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
fn test_calameo_reader_fallback_html_parsing() {
    let html_content = include_str!("fixtures/calameo_reader_fallback.html");
    let canonical_url = "https://www.calameo.com/read/0061133461a5012e8961a";

    let publication =
        parse_calameo_reader_html(html_content, "0061133461a5012e8961a", canonical_url)
            .expect("parse html reader");

    assert_eq!(publication.publication_id, "0061133461a5012e8961a");
    assert_eq!(publication.title, "Sample Fallback Publication Title");
    assert_eq!(publication.page_count, 8);
    assert_eq!(publication.pages.len(), 8);
}

#[test]
fn test_calameo_asset_url_signing() {
    let raw_url = "https://p.calameoassets.com/240822153012-0061133461a5012e8961a/p1.jpg";
    let token = "exp=1724345000~acl=/240822153012-0061133461a5012e8961a/*~hmac=abcdef0123456789";

    let signed = sign_calameo_asset_url(raw_url, Some(token));
    assert!(signed.contains("_token_="));
    assert!(signed.contains("hmac=abcdef0123456789"));
}
