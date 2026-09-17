use docdownloader::network::security::validate_url_security;
use docdownloader::storage::sanitize::{safe_output_path, sanitize_filename};
use std::path::Path;
use url::Url;

#[test]
fn test_ssrf_blocks_localhost() {
    let urls = [
        "http://localhost/secret",
        "http://localhost:8080/admin",
        "http://127.0.0.1/",
        "http://127.0.0.1:8000/api",
        "http://127.1.2.3/",
        "http://[::1]/",
        "http://[0:0:0:0:0:0:0:1]/",
    ];

    for u in urls {
        let parsed = Url::parse(u).expect("valid url syntax");
        let res = validate_url_security(&parsed);
        assert!(
            res.is_err(),
            "Expected URL '{u}' to be blocked by SSRF policy"
        );
    }
}

#[test]
fn test_ssrf_blocks_private_networks() {
    let urls = [
        "http://10.0.0.1/resource",
        "http://10.255.255.254/",
        "http://172.16.0.1/private",
        "http://172.31.255.255/",
        "http://192.168.1.1/router",
        "http://192.168.0.100:8080/",
        "http://169.254.169.254/latest/meta-data/", // AWS/Cloud metadata
    ];

    for u in urls {
        let parsed = Url::parse(u).expect("valid url syntax");
        let res = validate_url_security(&parsed);
        assert!(res.is_err(), "Expected private IP URL '{u}' to be rejected");
    }
}

#[test]
fn test_ssrf_blocks_non_http_schemes() {
    let urls = [
        "file:///etc/passwd",
        "file:///C:/Windows/win.ini",
        "ftp://ftp.example.com/file",
        "gopher://example.com/",
        "ws://example.com/",
    ];

    for u in urls {
        let parsed = Url::parse(u).expect("valid url syntax");
        let res = validate_url_security(&parsed);
        assert!(res.is_err(), "Expected scheme in URL '{u}' to be rejected");
    }
}

#[test]
fn test_path_traversal_sanitization() {
    assert_eq!(sanitize_filename("../../etc/passwd"), "etc_passwd");
    assert_eq!(
        sanitize_filename("..\\..\\Windows\\System32\\cmd.exe"),
        "Windows_System32_cmd.exe"
    );
    assert_eq!(sanitize_filename("/root/.ssh/id_rsa"), "root_.ssh_id_rsa");
    assert_eq!(sanitize_filename("C:\\autoexec.bat"), "C_autoexec.bat");
    assert_eq!(sanitize_filename("CON.txt"), "doc_CON.txt");
    assert_eq!(sanitize_filename("NUL"), "doc_NUL");
    assert_eq!(sanitize_filename("AUX"), "doc_AUX");
    assert_eq!(sanitize_filename("COM1.pdf"), "doc_COM1.pdf");
    assert_eq!(sanitize_filename("   "), "document");
    assert_eq!(sanitize_filename(""), "document");
}

#[test]
fn test_safe_output_path_confinement() {
    let out_dir = Path::new("test_output");
    let safe_path = safe_output_path(out_dir, "../../../malicious_file");
    assert_eq!(safe_path, out_dir.join("malicious_file.pdf"));

    let win_traversal = safe_output_path(out_dir, "..\\..\\secret");
    assert_eq!(win_traversal, out_dir.join("secret.pdf"));
}

#[test]
fn test_provider_domain_spoofing_rejected() {
    use docdownloader::providers::PublicationProvider;
    use docdownloader::providers::calameo::CalameoProvider;
    use docdownloader::providers::issuu::IssuuProvider;
    use docdownloader::providers::scribd::ScribdProvider;
    use docdownloader::providers::slideshare::SlideShareProvider;

    let calameo = CalameoProvider::new();
    let issuu = IssuuProvider::new();
    let scribd = ScribdProvider::new();
    let slideshare = SlideShareProvider::new();

    let calameo_spoofs = [
        "https://attacker-calameo.com/read/0061133461a5012e8961a",
        "https://calameo.com.attacker.com/read/0061133461a5012e8961a",
        "https://fakecalameo.com/books/0061133461a5012e8961a",
        "https://not-calameo.com/read/0061133461a5012e8961a",
    ];
    for u in calameo_spoofs {
        let url = Url::parse(u).unwrap();
        assert!(
            !calameo.can_handle(&url),
            "Calameo must reject spoofed domain: {u}"
        );
    }

    let issuu_spoofs = [
        "https://attacker-issuu.com/user/docs/slug",
        "https://issuu.com.evil.org/user/docs/slug",
        "https://fakeissuu.com/user/docs/slug",
    ];
    for u in issuu_spoofs {
        let url = Url::parse(u).unwrap();
        assert!(
            !issuu.can_handle(&url),
            "Issuu must reject spoofed domain: {u}"
        );
    }

    let scribd_spoofs = [
        "https://attacker-scribd.com/document/123456789/Title",
        "https://scribd.com.malicious.com/document/123456789/Title",
        "https://fakescribd.com/document/123456789/Title",
    ];
    for u in scribd_spoofs {
        let url = Url::parse(u).unwrap();
        assert!(
            !scribd.can_handle(&url),
            "Scribd must reject spoofed domain: {u}"
        );
    }

    let slideshare_spoofs = [
        "https://attacker-slideshare.net/author/deck",
        "https://slideshare.net.evil.com/author/deck",
        "https://fakeslideshare.net/author/deck",
    ];
    for u in slideshare_spoofs {
        let url = Url::parse(u).unwrap();
        assert!(
            !slideshare.can_handle(&url),
            "SlideShare must reject spoofed domain: {u}"
        );
    }
}

#[tokio::test]
async fn test_engine_inspect_blocks_ssrf() {
    use docdownloader::core::engine::DownloadEngine;
    use docdownloader::network::client::HttpClient;
    use docdownloader::providers::ProviderRegistry;
    use docdownloader::storage::cache::CacheManager;
    use std::sync::Arc;

    let client = HttpClient::default_client().expect("client");
    let registry = Arc::new(ProviderRegistry::new());
    let cache = CacheManager::new(std::env::temp_dir().join("test_sec_cache"));
    let engine = DownloadEngine::new(client, registry, cache, 2, false);

    let malicious_urls = [
        "http://127.0.0.1/latest/meta-data/",
        "http://localhost:8080/admin",
        "http://169.254.169.254/latest/meta-data/",
        "http://10.0.0.1/internal",
        "http://192.168.1.1/router",
        "file:///etc/passwd",
    ];

    for u in malicious_urls {
        let url = Url::parse(u).expect("valid url syntax");
        let result = engine.inspect(&url).await;
        assert!(
            result.is_err(),
            "engine.inspect() must reject SSRF target '{u}'"
        );
    }
}

#[test]
fn test_provider_parser_pathological_inputs() {
    use docdownloader::providers::issuu::parser::parse_issuu_reader_html;
    use docdownloader::providers::scribd::parser::parse_scribd_embed_html;
    use docdownloader::providers::slideshare::parser::parse_slideshare_html;

    let huge = "A".repeat(100_000);
    let pathological_inputs: &[&str] = &[
        "",
        "   ",
        "<html><body><<<>>>",
        "<!DOCTYPE html><html><head><title>",
        "{\"props\": null}",
        &huge,
    ];

    for input in pathological_inputs {
        // Issuu HTML parser should return an error without panicking
        let issuu_res =
            parse_issuu_reader_html(input, "user", "doc", "https://issuu.com/user/docs/doc");
        assert!(issuu_res.is_err());

        // Scribd HTML parser should return an error without panicking
        let scribd_res =
            parse_scribd_embed_html(input, "12345", "https://www.scribd.com/document/12345");
        assert!(scribd_res.is_err());

        // SlideShare HTML parser should return an error without panicking
        let slideshare_res =
            parse_slideshare_html(input, "https://www.slideshare.net/user/deck", "user/deck");
        assert!(slideshare_res.is_err());
    }
}
