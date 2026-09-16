pub mod models;
pub mod parser;

use async_trait::async_trait;
use regex::Regex;
use url::Url;

use crate::core::document::Publication;
use crate::core::error::DocDownloaderError;
use crate::network::client::HttpClient;
use crate::providers::PublicationProvider;

/// Adapter for Scribd document library and reader platform.
pub struct ScribdProvider;

impl Default for ScribdProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ScribdProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PublicationProvider for ScribdProvider {
    fn name(&self) -> &'static str {
        "scribd"
    }

    fn display_name(&self) -> &'static str {
        "Scribd"
    }

    fn can_handle(&self, url: &Url) -> bool {
        let host = match url.host_str() {
            Some(h) => h.to_ascii_lowercase(),
            None => return false,
        };

        if !host.contains("scribd.com") && !host.contains("scribd.test") {
            return false;
        }

        self.extract_id(url).is_ok()
    }

    fn extract_id(&self, url: &Url) -> Result<String, DocDownloaderError> {
        let path = url.path();

        // Pattern 1: /(?:document|doc|presentation|embeds|book)/(\d+)
        let re = Regex::new(r#"/(?:document|doc|presentation|embeds|book)/(\d+)"#).map_err(|e| {
            DocDownloaderError::InternalInvariantViolation {
                reason: format!("Failed to compile Scribd ID regex: {e}"),
            }
        })?;

        if let Some(caps) = re.captures(path)
            && let Some(id) = caps.get(1)
        {
            return Ok(id.as_str().to_string());
        }

        // Pattern 2: Look for numeric segment in path
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            if seg.len() >= 4 && seg.chars().all(|c| c.is_ascii_digit()) {
                return Ok(seg.to_string());
            }
        }

        Err(DocDownloaderError::InvalidUrl {
            url: url.to_string(),
            reason: "Could not extract numeric document ID from Scribd URL (expected /document/<id> or similar)".to_string(),
        })
    }

    async fn resolve(
        &self,
        client: &HttpClient,
        url: &Url,
    ) -> Result<Publication, DocDownloaderError> {
        let doc_id = self.extract_id(url)?;
        let canonical_url = format!("https://www.scribd.com/document/{doc_id}");

        // Attempt embed viewer first
        match parser::resolve_via_embed(client, &doc_id, &canonical_url).await {
            Ok(pub_doc) => Ok(pub_doc),
            Err(DocDownloaderError::AccessRestricted { id, reason }) => {
                Err(DocDownloaderError::AccessRestricted { id, reason })
            }
            Err(DocDownloaderError::PublicationNotFound { id, reason }) => {
                Err(DocDownloaderError::PublicationNotFound { id, reason })
            }
            Err(err) => {
                tracing::warn!(
                    "Primary Scribd embed resolution failed ({err}); attempting document page fallback"
                );
                parser::resolve_via_doc_page(client, &doc_id, &canonical_url).await
            }
        }
    }
}
