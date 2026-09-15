pub mod models;
pub mod parser;
pub mod signature;

use async_trait::async_trait;
use regex::Regex;
use url::Url;

use crate::core::document::Publication;
use crate::core::error::DocDownloaderError;
use crate::network::client::HttpClient;
use crate::providers::PublicationProvider;

/// Adapter for Calaméo interactive flipbook publications.
pub struct CalameoProvider {
    url_regex: Regex,
    bkcode_regex: Regex,
}

impl Default for CalameoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CalameoProvider {
    pub fn new() -> Self {
        Self {
            // Matches calameo.com domain with read/ or books/ paths or bkcode query parameter
            url_regex: Regex::new(
                r#"(?i)calameo(?:\.test|\.com)/(?:read|books)/([0-9a-fA-F]{21})"#,
            )
            .unwrap(),
            bkcode_regex: Regex::new(r#"^[0-9a-fA-F]{21}$"#).unwrap(),
        }
    }
}

#[async_trait]
impl PublicationProvider for CalameoProvider {
    fn name(&self) -> &'static str {
        "calameo"
    }

    fn display_name(&self) -> &'static str {
        "Calaméo"
    }

    fn can_handle(&self, url: &Url) -> bool {
        let host = match url.host_str() {
            Some(h) => h.to_ascii_lowercase(),
            None => return false,
        };

        if !host.contains("calameo.com") && !host.contains("calameo.test") {
            return false;
        }

        // Check path matching: /read/<bkcode> or /books/<bkcode>
        if self.url_regex.is_match(url.as_str()) {
            return true;
        }

        // Check query parameter matching: ?bkcode=<bkcode>
        for (k, v) in url.query_pairs() {
            if k == "bkcode" && self.bkcode_regex.is_match(&v) {
                return true;
            }
        }

        false
    }

    fn extract_id(&self, url: &Url) -> Result<String, DocDownloaderError> {
        // 1. Check path regex
        if let Some(caps) = self.url_regex.captures(url.as_str()) {
            if let Some(m) = caps.get(1) {
                return Ok(m.as_str().to_ascii_lowercase());
            }
        }

        // 2. Check query parameter `bkcode`
        for (k, v) in url.query_pairs() {
            if k == "bkcode" && self.bkcode_regex.is_match(&v) {
                return Ok(v.to_ascii_lowercase());
            }
        }

        Err(DocDownloaderError::InvalidUrl {
            url: url.to_string(),
            reason: "Could not extract 21-hex character publication ID from Calaméo URL".to_string(),
        })
    }

    async fn resolve(
        &self,
        client: &HttpClient,
        url: &Url,
    ) -> Result<Publication, DocDownloaderError> {
        let publication_id = self.extract_id(url)?;
        let canonical_url = format!("https://www.calameo.com/read/{publication_id}");

        // Attempt structured book API first
        match parser::resolve_via_book_api(client, &publication_id, &canonical_url).await {
            Ok(pub_doc) => Ok(pub_doc),
            Err(DocDownloaderError::AccessRestricted { id, reason }) => {
                Err(DocDownloaderError::AccessRestricted { id, reason })
            }
            Err(DocDownloaderError::PublicationNotFound { id, reason }) => {
                Err(DocDownloaderError::PublicationNotFound { id, reason })
            }
            Err(err) => {
                tracing::warn!(
                    "Primary Calaméo book API resolution failed ({err}); attempting HTML fallback"
                );
                // Attempt HTML fallback
                parser::resolve_via_html_fallback(client, &publication_id, &canonical_url).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_handle_various_calameo_urls() {
        let provider = CalameoProvider::new();

        let read_url = Url::parse("https://www.calameo.com/read/0061133461a5012e8961a").unwrap();
        assert!(provider.can_handle(&read_url));
        assert_eq!(provider.extract_id(&read_url).unwrap(), "0061133461a5012e8961a");

        let books_url = Url::parse("https://en.calameo.com/books/0061133461A5012E8961A?page=2").unwrap();
        assert!(provider.can_handle(&books_url));
        assert_eq!(provider.extract_id(&books_url).unwrap(), "0061133461a5012e8961a");

        let viewer_url = Url::parse("https://v.calameo.com/?bkcode=0061133461a5012e8961a&authid=").unwrap();
        assert!(provider.can_handle(&viewer_url));
        assert_eq!(provider.extract_id(&viewer_url).unwrap(), "0061133461a5012e8961a");

        let other_url = Url::parse("https://example.com/read/0061133461a5012e8961a").unwrap();
        assert!(!provider.can_handle(&other_url));
    }
}
