use serde::{Deserialize, Serialize};

/// Deserialization model for Issuu reader JSON manifest (reader3_4.json).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IssuuReaderManifest {
    pub document: Option<IssuuDocumentData>,
    #[serde(rename = "publicationId")]
    pub publication_id: Option<String>,
    #[serde(rename = "revisionId")]
    pub revision_id: Option<serde_json::Value>,
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "pageCount")]
    pub page_count: Option<u32>,
    pub pages: Option<Vec<IssuuPageData>>,
    #[serde(rename = "coverUrl")]
    pub cover_url: Option<String>,
    #[serde(rename = "isPrivate")]
    pub is_private: Option<bool>,
}

impl IssuuReaderManifest {
    /// Resolves effective publication ID from top-level or nested document.
    pub fn effective_publication_id(&self) -> Option<&str> {
        self.publication_id
            .as_deref()
            .or_else(|| self.document.as_ref().and_then(|d| d.publication_id.as_deref()))
    }

    /// Resolves effective revision ID as string.
    pub fn effective_revision_id(&self) -> Option<String> {
        let val = self
            .revision_id
            .as_ref()
            .or_else(|| self.document.as_ref().and_then(|d| d.revision_id.as_ref()));

        match val {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            _ => None,
        }
    }

    /// Resolves effective title.
    pub fn effective_title(&self) -> Option<&str> {
        self.title
            .as_deref()
            .or_else(|| self.document.as_ref().and_then(|d| d.title.as_deref()))
    }

    /// Resolves effective page count.
    pub fn effective_page_count(&self) -> u32 {
        self.page_count
            .or_else(|| self.document.as_ref().and_then(|d| d.page_count))
            .or_else(|| self.effective_pages().map(|p| p.len() as u32))
            .unwrap_or(0)
    }

    /// Resolves effective page slice.
    pub fn effective_pages(&self) -> Option<&[IssuuPageData]> {
        if let Some(ref p) = self.pages {
            Some(p.as_slice())
        } else if let Some(ref d) = self.document {
            d.pages.as_deref()
        } else {
            None
        }
    }

    /// Checks if publication is marked private.
    pub fn is_private_access(&self) -> bool {
        self.is_private.unwrap_or(false)
            || self
                .document
                .as_ref()
                .and_then(|d| d.is_private)
                .unwrap_or(false)
    }
}

/// Document-level metadata in Issuu manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IssuuDocumentData {
    #[serde(rename = "publicationId")]
    pub publication_id: Option<String>,
    #[serde(rename = "revisionId")]
    pub revision_id: Option<serde_json::Value>,
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "pageCount")]
    pub page_count: Option<u32>,
    pub pages: Option<Vec<IssuuPageData>>,
    #[serde(rename = "coverUrl")]
    pub cover_url: Option<String>,
    #[serde(rename = "isPrivate")]
    pub is_private: Option<bool>,
}

/// Individual page descriptor in Issuu reader manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IssuuPageData {
    #[serde(rename = "pageNumber")]
    pub page_number: Option<u32>,
    #[serde(rename = "imageUri")]
    pub image_uri: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}
