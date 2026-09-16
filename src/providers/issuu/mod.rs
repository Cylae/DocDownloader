pub mod models;
pub mod parser;

use async_trait::async_trait;
use url::Url;

use crate::core::document::Publication;
use crate::core::error::DocDownloaderError;
use crate::network::client::HttpClient;
use crate::providers::PublicationProvider;

/// Adapter for Issuu digital publishing platform.
pub struct IssuuProvider;

impl Default for IssuuProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl IssuuProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PublicationProvider for IssuuProvider {
    fn name(&self) -> &'static str {
        "issuu"
    }

    fn display_name(&self) -> &'static str {
        "Issuu"
    }

    fn can_handle(&self, url: &Url) -> bool {
        let host = match url.host_str() {
            Some(h) => h.to_ascii_lowercase(),
            None => return false,
        };

        if !host.contains("issuu.com") && !host.contains("issuu.test") {
            return false;
        }

        self.extract_id(url).is_ok()
    }

    fn extract_id(&self, url: &Url) -> Result<String, DocDownloaderError> {
        // Case 1: Embed query params (e.g. e.issuu.com/embed.html?d=slug&u=user)
        let mut u_param = None;
        let mut d_param = None;
        for (k, v) in url.query_pairs() {
            if k.eq_ignore_ascii_case("u") {
                u_param = Some(v.to_string());
            } else if k.eq_ignore_ascii_case("d") {
                d_param = Some(v.to_string());
            }
        }
        if let (Some(u), Some(d)) = (u_param, d_param) {
            if !u.is_empty() && !d.is_empty() {
                return Ok(format!("{}/{}", u.to_ascii_lowercase(), d.to_ascii_lowercase()));
            }
        }

        // Case 2: Path segments: /{username}/docs/{doc_slug}
        let segments: Vec<&str> = url
            .path_segments()
            .map(|s| s.filter(|seg| !seg.is_empty()).collect())
            .unwrap_or_default();

        for i in 0..segments.len() {
            if segments[i].eq_ignore_ascii_case("docs") && i > 0 && i + 1 < segments.len() {
                let user = segments[i - 1].to_ascii_lowercase();
                let slug = segments[i + 1].to_ascii_lowercase();
                return Ok(format!("{user}/{slug}"));
            }
        }

        Err(DocDownloaderError::InvalidUrl {
            url: url.to_string(),
            reason: "Could not extract username and document slug from Issuu URL (expected /<username>/docs/<slug>)".to_string(),
        })
    }

    async fn resolve(
        &self,
        client: &HttpClient,
        url: &Url,
    ) -> Result<Publication, DocDownloaderError> {
        let pub_id = self.extract_id(url)?;
        let mut parts = pub_id.splitn(2, '/');
        let username = parts.next().unwrap_or_default();
        let doc_slug = parts.next().unwrap_or_default();
        let canonical_url = format!("https://issuu.com/{username}/docs/{doc_slug}");

        // Attempt structured reader manifest first
        match parser::resolve_via_reader_manifest(client, username, doc_slug, &canonical_url).await {
            Ok(pub_doc) => Ok(pub_doc),
            Err(err) => {
                tracing::warn!(
                    "Primary Issuu reader manifest resolution failed ({err}); attempting HTML fallback"
                );
                parser::resolve_via_html_fallback(client, username, doc_slug, &canonical_url).await
            }
        }
    }
}
