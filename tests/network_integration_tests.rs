use docdownloader::core::error::DocDownloaderError;
use docdownloader::network::client::{HttpClient, MAX_PAGE_BYTE_LIMIT};
use docdownloader::storage::atomic::AtomicFileWriter;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_wiremock_200_stream_to_atomic_file() {
    let mock_server = MockServer::start().await;

    let payload = b"MOCK_JPEG_PAYLOAD_VALID_STREAM_CONTENT";
    Mock::given(method("GET"))
        .and(path("/page1.jpg"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(payload.to_vec())
                .append_header("Content-Type", "image/jpeg"),
        )
        .mount(&mock_server)
        .await;

    let client = HttpClient::new_test_client().expect("client");
    let temp = tempdir().expect("tempdir");
    let target = temp.path().join("downloaded.jpg");
    let mut writer = AtomicFileWriter::new(&target).expect("writer");

    let url = format!("{}/page1.jpg", mock_server.uri());
    let (bytes, hash) = client
        .stream_to_atomic_file(&url, None, &mut writer, MAX_PAGE_BYTE_LIMIT)
        .await
        .expect("download stream");

    assert_eq!(bytes, payload.len() as u64);
    assert!(!hash.is_empty());
    let committed = writer.commit().expect("commit");
    assert!(committed.exists());
    assert_eq!(std::fs::read(&committed).unwrap(), payload);
}

#[tokio::test]
async fn test_wiremock_429_retry_after() {
    let mock_server = MockServer::start().await;

    // First request returns 429 with Retry-After: 0, second request succeeds
    Mock::given(method("GET"))
        .and(path("/throttled.jpg"))
        .respond_with(
            ResponseTemplate::new(429)
                .append_header("Retry-After", "0")
                .set_body_string("Rate limited"),
        )
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/throttled.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"RECOVERED_PAGE_DATA".to_vec()))
        .mount(&mock_server)
        .await;

    let client = HttpClient::new_test_client().expect("client");
    let url = format!("{}/throttled.jpg", mock_server.uri());

    let resp = client
        .get_with_retry(&url, None)
        .await
        .expect("should recover after 429");
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_wiremock_500_transient_retry_success() {
    let mock_server = MockServer::start().await;

    // Fail once with 500, then return 200
    Mock::given(method("GET"))
        .and(path("/transient.jpg"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/transient.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"SUCCESS_AFTER_RETRY".to_vec()))
        .mount(&mock_server)
        .await;

    let client = HttpClient::new_test_client().expect("client");
    let url = format!("{}/transient.jpg", mock_server.uri());

    let resp = client
        .get_with_retry(&url, None)
        .await
        .expect("should succeed after retry");
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_wiremock_permanent_404_fails_immediately() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/notfound.jpg"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let client = HttpClient::new_test_client().expect("client");
    let url = format!("{}/notfound.jpg", mock_server.uri());

    let err = client
        .get_with_retry(&url, None)
        .await
        .expect_err("404 must fail");
    assert!(
        matches!(err, DocDownloaderError::PublicationNotFound { .. }),
        "Expected PublicationNotFound error, got: {err:?}"
    );
}

#[tokio::test]
async fn test_wiremock_permanent_403_access_restricted() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/private.jpg"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&mock_server)
        .await;

    let client = HttpClient::new_test_client().expect("client");
    let url = format!("{}/private.jpg", mock_server.uri());

    let err = client
        .get_with_retry(&url, None)
        .await
        .expect_err("403 must fail");
    assert!(
        matches!(err, DocDownloaderError::AccessRestricted { .. }),
        "Expected AccessRestricted error, got: {err:?}"
    );
}

#[tokio::test]
async fn test_wiremock_zero_bytes_rejected() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/empty.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(Vec::new()))
        .mount(&mock_server)
        .await;

    let client = HttpClient::new_test_client().expect("client");
    let temp = tempdir().expect("tempdir");
    let target = temp.path().join("empty.jpg");
    let mut writer = AtomicFileWriter::new(&target).expect("writer");

    let url = format!("{}/empty.jpg", mock_server.uri());
    let res = client
        .stream_to_atomic_file(&url, None, &mut writer, MAX_PAGE_BYTE_LIMIT)
        .await;

    assert!(res.is_err());
    match res.unwrap_err() {
        DocDownloaderError::PageCorrupt { reason, .. } => {
            assert!(reason.contains("0 bytes"));
        }
        other => panic!("Expected PageCorrupt, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_wiremock_content_length_exceeded_safety_limit() {
    let mock_server = MockServer::start().await;

    // Body of 2048 bytes
    let body = vec![0u8; 2048];
    Mock::given(method("GET"))
        .and(path("/huge.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
        .mount(&mock_server)
        .await;

    let client = HttpClient::new_test_client().expect("client");
    let temp = tempdir().expect("tempdir");
    let target = temp.path().join("huge.jpg");
    let mut writer = AtomicFileWriter::new(&target).expect("writer");

    // Setting max_bytes = 1000 ensures Content-Length 2048 exceeds the limit
    let url = format!("{}/huge.jpg", mock_server.uri());
    let res = client
        .stream_to_atomic_file(&url, None, &mut writer, 1000)
        .await;

    assert!(res.is_err());
    match res.unwrap_err() {
        DocDownloaderError::PageCorrupt { reason, .. } => {
            assert!(reason.contains("exceeds maximum safety limit"));
        }
        other => panic!("Expected PageCorrupt, got: {other:?}"),
    }
}
