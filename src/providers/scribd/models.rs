use serde::{Deserialize, Serialize};

/// Deserialization model for Scribd embedded metadata blocks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScribdDocMetadata {
    pub id: Option<serde_json::Value>,
    pub title: Option<String>,
    pub author: Option<String>,
    #[serde(rename = "pageCount")]
    pub page_count: Option<u32>,
    #[serde(rename = "isPrivate")]
    pub is_private: Option<bool>,
    pub access: Option<String>,
    pub secret_password: Option<bool>,
}
