use async_trait::async_trait;
use url::Url;

use crate::core::document::Publication;
use crate::core::error::DocDownloaderError;
use crate::network::client::HttpClient;

pub mod calameo;

/// Contract that every publication platform adapter must satisfy.
#[async_trait]
pub trait PublicationProvider: Send + Sync {
    /// Unique provider identifier (e.g. "calameo").
    fn name(&self) -> &'static str;

    /// Human-friendly display name (e.g. "Calaméo").
    fn display_name(&self) -> &'static str;

    /// Checks if this provider handles the given target URL.
    fn can_handle(&self, url: &Url) -> bool;

    /// Extracts normalized publication ID from the URL.
    fn extract_id(&self, url: &Url) -> Result<String, DocDownloaderError>;

    /// Resolves the publication metadata and ordered page manifest.
    async fn resolve(
        &self,
        client: &HttpClient,
        url: &Url,
    ) -> Result<Publication, DocDownloaderError>;
}

/// Registry managing all supported publication providers.
pub struct ProviderRegistry {
    providers: Vec<Box<dyn PublicationProvider>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            providers: Vec::new(),
        };
        registry.register(Box::new(calameo::CalameoProvider::new()));
        registry
    }

    pub fn register(&mut self, provider: Box<dyn PublicationProvider>) {
        self.providers.push(provider);
    }

    pub fn find_provider(&self, url: &Url) -> Option<&dyn PublicationProvider> {
        self.providers
            .iter()
            .find(|p| p.can_handle(url))
            .map(|b| b.as_ref())
    }

    pub async fn resolve(
        &self,
        client: &HttpClient,
        url: &Url,
    ) -> Result<Publication, DocDownloaderError> {
        let provider =
            self.find_provider(url)
                .ok_or_else(|| DocDownloaderError::UnsupportedProvider {
                    url: url.to_string(),
                })?;

        provider.resolve(client, url).await
    }
}
