use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER, USER_AGENT};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

use crate::core::error::DocDownloaderError;
use crate::network::retry::{RetryPolicy, parse_retry_after};
use crate::network::security::{SecureDnsResolver, validate_url_security};
use crate::storage::atomic::AtomicFileWriter;

/// Default User-Agent string used for transparent identification.
pub const DEFAULT_USER_AGENT: &str = concat!(
    "DocDownloader/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/Cylae/DocDownloader)"
);

/// Maximum size allowed for a single page download to guard against memory or disk exhaustion (e.g. 50 MB).
pub const MAX_PAGE_BYTE_LIMIT: u64 = 50 * 1024 * 1024;

/// Production HTTP client wrapped with connection pooling, timeouts, and SSRF redirect checks.
#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    retry_policy: Arc<RetryPolicy>,
    allow_local_mock: bool,
}

impl HttpClient {
    pub fn new(
        connect_timeout: Duration,
        request_timeout: Duration,
    ) -> Result<Self, DocDownloaderError> {
        let mut default_headers = HeaderMap::new();
        default_headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));

        let redirect_policy = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.error("Too many redirects (maximum 5 hops allowed)");
            }
            // Validate the target URL against SSRF rules on every redirect hop
            let target_url = attempt.url();
            if let Err(e) = validate_url_security(target_url) {
                return attempt.error(format!("Security policy rejected redirect: {e}"));
            }
            attempt.follow()
        });

        let client = reqwest::Client::builder()
            .default_headers(default_headers)
            .connect_timeout(connect_timeout)
            .timeout(request_timeout)
            .redirect(redirect_policy)
            .dns_resolver(Arc::new(SecureDnsResolver))
            .pool_max_idle_per_host(10)
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .build()
            .map_err(|e| DocDownloaderError::InternalInvariantViolation {
                reason: format!("Failed to build HTTP client: {e}"),
            })?;

        Ok(Self {
            inner: client,
            retry_policy: Arc::new(RetryPolicy::default()),
            allow_local_mock: false,
        })
    }

    pub fn default_client() -> Result<Self, DocDownloaderError> {
        Self::new(Duration::from_secs(10), Duration::from_secs(45))
    }

    /// Creates a test-only HTTP client that permits loopback connections for local WireMock tests.
    pub fn new_test_client() -> Result<Self, DocDownloaderError> {
        let mut default_headers = HeaderMap::new();
        default_headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));

        let client = reqwest::Client::builder()
            .default_headers(default_headers)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| DocDownloaderError::InternalInvariantViolation {
                reason: format!("Failed to build test HTTP client: {e}"),
            })?;

        Ok(Self {
            inner: client,
            retry_policy: Arc::new(RetryPolicy {
                max_retries: 2,
                initial_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(50),
                jitter_factor: 0.1,
            }),
            allow_local_mock: true,
        })
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = Arc::new(policy);
        self
    }

    pub fn inner(&self) -> &reqwest::Client {
        &self.inner
    }

    pub fn is_local_mock_allowed(&self) -> bool {
        self.allow_local_mock
    }

    /// Validates the URL and sends an HTTP GET request with retries, respecting 429 and transient errors.
    pub async fn get_with_retry(
        &self,
        url_str: &str,
        extra_headers: Option<&[(String, String)]>,
    ) -> Result<reqwest::Response, DocDownloaderError> {
        let parsed_url = Url::parse(url_str).map_err(|e| DocDownloaderError::InvalidUrl {
            url: url_str.to_string(),
            reason: e.to_string(),
        })?;

        if !self.allow_local_mock {
            validate_url_security(&parsed_url)?;
        }

        let mut attempt = 0;
        loop {
            attempt += 1;
            let mut req = self.inner.get(parsed_url.clone());
            if let Some(headers) = extra_headers {
                for (k, v) in headers {
                    if let (Ok(hk), Ok(hv)) = (
                        reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                        HeaderValue::from_str(v),
                    ) {
                        req = req.header(hk, hv);
                    }
                }
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp);
                    }

                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let retry_after = resp
                            .headers()
                            .get(RETRY_AFTER)
                            .and_then(|h| h.to_str().ok())
                            .and_then(parse_retry_after);

                        if attempt <= self.retry_policy.max_retries {
                            let delay = self.retry_policy.delay_for_attempt(attempt, retry_after);
                            tokio::time::sleep(delay).await;
                            continue;
                        }

                        return Err(DocDownloaderError::RateLimited {
                            retry_after_secs: retry_after.map(|d| d.as_secs()),
                            reason: format!("HTTP 429 Too Many Requests after {attempt} attempts"),
                        });
                    }

                    if RetryPolicy::is_status_retryable(status.as_u16())
                        && attempt <= self.retry_policy.max_retries
                    {
                        let delay = self.retry_policy.delay_for_attempt(attempt, None);
                        tokio::time::sleep(delay).await;
                        continue;
                    }

                    // Permanent failure
                    let status_code = status.as_u16();
                    if status_code == 404 {
                        return Err(DocDownloaderError::PublicationNotFound {
                            id: url_str.to_string(),
                            reason: "HTTP 404 Not Found".to_string(),
                        });
                    }
                    if status_code == 401 || status_code == 403 {
                        return Err(DocDownloaderError::AccessRestricted {
                            id: url_str.to_string(),
                            reason: format!("HTTP {status_code} Access Denied"),
                        });
                    }

                    return Err(DocDownloaderError::PageUnavailable {
                        page_index: 0,
                        reason: format!("HTTP error status {status_code}"),
                        status: Some(status_code),
                    });
                }
                Err(err) => {
                    if err.is_timeout() {
                        if attempt <= self.retry_policy.max_retries {
                            let delay = self.retry_policy.delay_for_attempt(attempt, None);
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        return Err(DocDownloaderError::NetworkTimeout {
                            url: url_str.to_string(),
                            elapsed_secs: 45,
                        });
                    }

                    if attempt <= self.retry_policy.max_retries {
                        let delay = self.retry_policy.delay_for_attempt(attempt, None);
                        tokio::time::sleep(delay).await;
                        continue;
                    }

                    return Err(DocDownloaderError::InternalInvariantViolation {
                        reason: format!("Network request failed: {err}"),
                    });
                }
            }
        }
    }

    /// Streams an HTTP response directly into an atomic file, calculating SHA-256 and enforcing byte limits.
    pub async fn stream_to_atomic_file(
        &self,
        url_str: &str,
        extra_headers: Option<&[(String, String)]>,
        atomic_writer: &mut AtomicFileWriter,
        max_bytes: u64,
    ) -> Result<(u64, String), DocDownloaderError> {
        let mut resp = self.get_with_retry(url_str, extra_headers).await?;

        // Guard against absurd Content-Length before downloading
        if let Some(content_len) = resp.content_length()
            && content_len > max_bytes
        {
            return Err(DocDownloaderError::PageCorrupt {
                page_index: 0,
                reason: format!(
                    "Content-Length {content_len} exceeds maximum safety limit of {max_bytes} bytes"
                ),
            });
        }

        let mut hasher = Sha256::new();
        let mut total_bytes: u64 = 0;

        while let Some(chunk) =
            resp.chunk()
                .await
                .map_err(|e| DocDownloaderError::FileSystemError {
                    path: atomic_writer.temp_path().to_path_buf(),
                    reason: format!("Failed to read stream chunk: {e}"),
                })?
        {
            total_bytes += chunk.len() as u64;
            if total_bytes > max_bytes {
                return Err(DocDownloaderError::PageCorrupt {
                    page_index: 0,
                    reason: format!("Streamed bytes exceeded maximum safety limit of {max_bytes}"),
                });
            }

            hasher.update(&chunk);
            atomic_writer.write_all(&chunk)?;
        }

        if total_bytes == 0 {
            return Err(DocDownloaderError::PageCorrupt {
                page_index: 0,
                reason: "Downloaded asset is 0 bytes".to_string(),
            });
        }

        let hash = hex::encode(hasher.finalize());
        Ok((total_bytes, hash))
    }
}
