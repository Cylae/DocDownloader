use serde::{Deserialize, Serialize};

/// Deserialization model for SlideShare oEmbed API endpoint response.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlideShareOEmbedResponse {
    pub title: Option<String>,
    pub author_name: Option<String>,
    pub author_url: Option<String>,
    pub provider_name: Option<String>,
    pub provider_url: Option<String>,
    pub slideshow_id: Option<serde_json::Value>,
    pub total_slides: Option<u32>,
    pub thumbnail: Option<String>,
    pub thumbnail_url: Option<String>,
    pub html: Option<String>,
    pub version: Option<String>,
}

impl SlideShareOEmbedResponse {
    /// Resolves slideshow ID as string regardless of whether JSON returned int or string.
    pub fn effective_slideshow_id(&self) -> Option<String> {
        match self.slideshow_id.as_ref() {
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// Resolves best available thumbnail URL.
    pub fn effective_thumbnail(&self) -> Option<&str> {
        self.thumbnail
            .as_deref()
            .or(self.thumbnail_url.as_deref())
    }
}
