use docdownloader::providers::scribd::parser::parse_scribd_embed_html;
use docdownloader::providers::scribd::ScribdProvider;
use docdownloader::providers::PublicationProvider;
use url::Url;

#[test]
fn test_scribd_provider_detection() {
    let provider = ScribdProvider::new();

    let valid_urls = [
        "https://www.scribd.com/document/123456789/Engineering-Whitepaper",
        "https://scribd.com/document/123456789/Engineering-Whitepaper",
        "https://www.scribd.com/doc/123456789/Engineering-Whitepaper",
        "https://www.scribd.com/presentation/123456789/Engineering-Whitepaper",
        "https://www.scribd.com/embeds/123456789/content?start_page=1&view_mode=scroll",
        "https://es.scribd.com/document/123456789/Engineering-Whitepaper",
    ];

    for u in valid_urls {
        let url = Url::parse(u).expect("valid url");
        assert!(
            provider.can_handle(&url),
            "Expected ScribdProvider to handle '{u}'"
        );
        let id = provider.extract_id(&url).expect("extract id");
        assert_eq!(id, "123456789");
    }

    let invalid_urls = [
        "https://www.calameo.com/read/0061133461a5012e8961a",
        "https://issuu.com/user/docs/doc",
        "https://www.slideshare.net/author/deck",
        "https://www.scribd.com/about",
        "https://www.scribd.com/terms",
        "https://google.com/",
    ];

    for u in invalid_urls {
        let url = Url::parse(u).expect("valid url");
        assert!(
            !provider.can_handle(&url),
            "Expected ScribdProvider to reject '{u}'"
        );
    }
}

#[test]
fn test_scribd_embed_html_parsing() {
    let html_content = include_str!("fixtures/scribd_embed_valid.html");
    let canonical_url = "https://www.scribd.com/document/123456789";

    let publication = parse_scribd_embed_html(html_content, "123456789", canonical_url)
        .expect("parse scribd embed html");

    assert_eq!(publication.provider, "scribd");
    assert_eq!(publication.publication_id, "123456789");
    assert_eq!(publication.title, "Sample Scribd Engineering Whitepaper");
    assert_eq!(publication.page_count, 3);
    assert_eq!(publication.pages.len(), 3);

    for (idx, page) in publication.pages.iter().enumerate() {
        let page_num = idx + 1;
        assert_eq!(page.index, page_num as u32);
        assert!(!page.candidates.is_empty());

        // Check that page candidate from absimg is discovered as priority 0
        let best = page.best_candidate().unwrap();
        assert_eq!(best.priority, 0);
        assert!(best.url.contains(&format!("pages/{page_num}.jpg")));
    }
}

#[test]
fn test_scribd_restricted_access_rejection() {
    let html_content = include_str!("fixtures/scribd_embed_private.html");
    let canonical_url = "https://www.scribd.com/document/99999999";

    let result = parse_scribd_embed_html(html_content, "99999999", canonical_url);
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
