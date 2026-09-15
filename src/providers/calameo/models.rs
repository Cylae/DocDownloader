use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalameoResponse {
    pub status: String,
    pub id: Option<String>,
    pub content: Option<CalameoContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalameoContent {
    pub id: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mode: String,
    pub account: Option<CalameoAccount>,
    pub document: Option<CalameoDocument>,
    pub features: Option<CalameoFeatures>,
    pub domains: Option<CalameoDomains>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalameoAccount {
    pub id: Option<u64>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalameoDocument {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub pages: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalameoFeatures {
    pub download: Option<CalameoDownloadFeature>,
    pub urlsigning: Option<CalameoUrlSigningFeature>,
    pub subscribers: Option<CalameoSubscribersFeature>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalameoDownloadFeature {
    pub enabled: bool,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalameoUrlSigningFeature {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalameoSubscribersFeature {
    pub enabled: Option<bool>,
    pub access: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalameoDomains {
    pub pages: Option<String>,
    pub image: Option<String>,
    pub thumbnail: Option<String>,
    pub secured: Option<CalameoSecuredDomains>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalameoSecuredDomains {
    pub svg: Option<String>,
    pub image: Option<String>,
}
