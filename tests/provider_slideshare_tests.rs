use docdownloader::providers::PublicationProvider;
use docdownloader::providers::slideshare::SlideShareProvider;
use docdownloader::providers::slideshare::models::SlideShareOEmbedResponse;
use docdownloader::providers::slideshare::parser::{
    parse_slideshare_html, parse_slideshare_oembed,
};
use url::Url;

#[test]
fn test_slideshare_provider_detection() {
    let provider = SlideShareProvider::new();

    let valid_urls = [
        "https://www.slideshare.net/techarchitect/cloud-native-design-patterns",
        "https://slideshare.net/techarchitect/cloud-native-design-patterns",
        "https://fr.slideshare.net/techarchitect/cloud-native-design-patterns",
        "https://www.slideshare.net/slideshow/embed_code/key/abcdef123",
        "https://www.slideshare.net/techarchitect/cloud-native-design-patterns?qid=123",
    ];

    for u in valid_urls {
        let url = Url::parse(u).expect("valid url");
        assert!(
            provider.can_handle(&url),
            "Expected SlideShareProvider to handle '{u}'"
        );
    }

    let id = provider
        .extract_id(
            &Url::parse("https://www.slideshare.net/techarchitect/cloud-native-design-patterns")
                .unwrap(),
        )
        .expect("extract id");
    assert_eq!(id, "techarchitect/cloud-native-design-patterns");

    let invalid_urls = [
        "https://www.calameo.com/read/0061133461a5012e8961a",
        "https://issuu.com/user/docs/doc",
        "https://www.scribd.com/document/123/doc",
        "https://www.slideshare.net/about",
        "https://www.slideshare.net/terms",
        "https://google.com/",
    ];

    for u in invalid_urls {
        let url = Url::parse(u).expect("valid url");
        assert!(
            !provider.can_handle(&url),
            "Expected SlideShareProvider to reject '{u}'"
        );
    }
}

#[test]
fn test_slideshare_oembed_parsing() {
    let json_data = include_str!("fixtures/slideshare_oembed_valid.json");
    let resp: SlideShareOEmbedResponse =
        serde_json::from_str(json_data).expect("deserialize fixture");

    let canonical_url = "https://www.slideshare.net/techarchitect/cloud-native-architecture";
    let publication = parse_slideshare_oembed(
        &resp,
        canonical_url,
        "techarchitect/cloud-native-architecture",
    )
    .expect("parse oembed");

    assert_eq!(publication.provider, "slideshare");
    assert_eq!(
        publication.publication_id,
        "techarchitect/cloud-native-architecture"
    );
    assert_eq!(
        publication.title,
        "Cloud Native Architecture Design Patterns"
    );
    assert_eq!(publication.author.as_deref(), Some("techarchitect"));
    assert_eq!(publication.page_count, 5);
    assert_eq!(publication.pages.len(), 5);

    // Verify slide resolution tiers
    for (idx, page) in publication.pages.iter().enumerate() {
        let slide_num = idx + 1;
        assert_eq!(page.index, slide_num as u32);
        assert_eq!(page.candidates.len(), 3);

        // Priority 1: 2048px
        assert_eq!(page.candidates[0].priority, 1);
        assert!(
            page.candidates[0]
                .url
                .contains(&format!("cloud-native-{slide_num}-2048.jpg"))
        );

        // Priority 2: 1024px
        assert_eq!(page.candidates[1].priority, 2);
        assert!(
            page.candidates[1]
                .url
                .contains(&format!("cloud-native-{slide_num}-1024.jpg"))
        );

        // Priority 3: 638px
        assert_eq!(page.candidates[2].priority, 3);
        assert!(
            page.candidates[2]
                .url
                .contains(&format!("cloud-native-{slide_num}-638.jpg"))
        );
    }
}

#[test]
fn test_slideshare_reader_fallback_html_parsing() {
    let html_content = include_str!("fixtures/slideshare_reader_fallback.html");
    let canonical_url = "https://www.slideshare.net/architect/dist-sys";

    let publication = parse_slideshare_html(html_content, canonical_url, "architect/dist-sys")
        .expect("parse fallback html");

    assert_eq!(publication.provider, "slideshare");
    assert_eq!(publication.publication_id, "architect/dist-sys");
    assert_eq!(publication.title, "Modern Distributed Systems");
    assert_eq!(publication.page_count, 6);
    assert_eq!(publication.pages.len(), 6);

    for (idx, page) in publication.pages.iter().enumerate() {
        let slide_num = idx + 1;
        assert_eq!(page.index, slide_num as u32);
        assert!(
            page.candidates
                .iter()
                .any(|c| c.url.contains(&format!("dist-sys-{slide_num}-2048.jpg")))
        );
    }
}

#[test]
fn test_slideshare_private_access_rejection() {
    let html = "<html><body><div class='error'>This presentation is private.</div></body></html>";
    let canonical_url = "https://www.slideshare.net/user/private-deck";

    let result = parse_slideshare_html(html, canonical_url, "user/private-deck");
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        matches!(
            err,
            docdownloader::core::error::DocDownloaderError::AccessRestricted { .. }
        ),
        "Expected AccessRestricted, got: {err:?}"
    );
}
