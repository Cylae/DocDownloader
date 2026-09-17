pub mod models;
pub mod parser;

use async_trait::async_trait;
use url::Url;

use crate::core::document::Publication;
use crate::core::error::DocDownloaderError;
use crate::network::client::HttpClient;
use crate::providers::PublicationProvider;

/// Adapter for SlideShare presentation platform.
pub struct SlideShareProvider;

impl Default for SlideShareProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SlideShareProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PublicationProvider for SlideShareProvider {
    fn name(&self) -> &'static str {
        "slideshare"
    }

    fn display_name(&self) -> &'static str {
        "SlideShare"
    }

    fn can_handle(&self, url: &Url) -> bool {
        let host = match url.host_str() {
            Some(h) => h.to_ascii_lowercase(),
            None => return false,
        };

        let is_slideshare_domain = host == "slideshare.net"
            || host.ends_with(".slideshare.net")
            || host == "slideshare.test"
            || host.ends_with(".slideshare.test");
        if !is_slideshare_domain {
            return false;
        }

        self.extract_id(url).is_ok()
    }

    fn extract_id(&self, url: &Url) -> Result<String, DocDownloaderError> {
        let segments: Vec<&str> = url
            .path_segments()
            .map(|s| s.filter(|seg| !seg.is_empty()).collect())
            .unwrap_or_default();

        if segments.is_empty() {
            return Err(DocDownloaderError::InvalidUrl {
                url: url.to_string(),
                reason: "Empty path in SlideShare URL".to_string(),
            });
        }

        // Ignore utility routes
        let first = segments[0].to_ascii_lowercase();
        if first == "explore"
            || first == "features"
            || first == "about"
            || first == "terms"
            || first == "privacy"
        {
            return Err(DocDownloaderError::InvalidUrl {
                url: url.to_string(),
                reason: format!("Non-presentation SlideShare route: /{first}"),
            });
        }

        // Handle /slideshow/embed_code/key/{key} or /slideshow/{slug}/{id}
        if first == "slideshow" && segments.len() >= 2 {
            return Ok(segments[1..].join("/").to_ascii_lowercase());
        }

        // Standard: /{username}/{presentation_slug}
        if segments.len() >= 2 {
            let username = segments[0].to_ascii_lowercase();
            let slug = segments[1].to_ascii_lowercase();
            return Ok(format!("{username}/{slug}"));
        }

        Err(DocDownloaderError::InvalidUrl {
            url: url.to_string(),
            reason: "Could not extract presentation slug from SlideShare URL (expected /<username>/<slug>)".to_string(),
        })
    }

    async fn resolve(
        &self,
        client: &HttpClient,
        url: &Url,
    ) -> Result<Publication, DocDownloaderError> {
        let presentation_id = self.extract_id(url)?;
        let canonical_url = url
            .as_str()
            .split('#')
            .next()
            .unwrap_or(url.as_str())
            .split('?')
            .next()
            .unwrap_or(url.as_str())
            .to_string();

        // Attempt structured oEmbed API first
        match parser::resolve_via_oembed(client, &canonical_url, &presentation_id).await {
            Ok(pub_doc) => Ok(pub_doc),
            Err(DocDownloaderError::AccessRestricted { id, reason }) => {
                Err(DocDownloaderError::AccessRestricted { id, reason })
            }
            Err(err) => {
                tracing::warn!(
                    "Primary SlideShare oEmbed resolution failed ({err}); attempting HTML fallback"
                );
                parser::resolve_via_html_fallback(client, &canonical_url, &presentation_id).await
            }
        }
    }
}
