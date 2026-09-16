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
