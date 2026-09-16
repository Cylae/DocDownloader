use serde::{Deserialize, Serialize};
use std::path::Path;
use url::Url;

use crate::core::error::DocDownloaderError;

/// Sanitized diagnostic telemetry bundle for troubleshooting and bug reporting.
/// Strictly excludes credentials, cookies, tokens, and document page bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticBundle {
    pub tool_name: String,
    pub tool_version: String,
    pub os: String,
    pub timestamp_utc: String,
    pub provider: String,
    pub canonical_url_sanitized: String,
    pub stage_reached: String,
    pub discovered_page_count: Option<u32>,
    pub selected_strategy: Option<String>,
    pub direct_pdf_available: bool,
    pub http_status_codes: Vec<u16>,
    pub retries_attempted: u32,
    pub error_reason: Option<String>,
}

impl DiagnosticBundle {
    pub fn new(provider: &str, raw_url: &Url) -> Self {
        let sanitized = sanitize_diagnostic_url(raw_url);
        let timestamp_unix_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            tool_name: "DocDownloader".to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            timestamp_utc: format!("{timestamp_unix_secs}"),
            provider: provider.to_string(),
            canonical_url_sanitized: sanitized,
            stage_reached: "Init".to_string(),
            discovered_page_count: None,
            selected_strategy: None,
            direct_pdf_available: false,
            http_status_codes: Vec::new(),
            retries_attempted: 0,
            error_reason: None,
        }
    }

    pub fn set_stage(&mut self, stage: &str) {
        self.stage_reached = stage.to_string();
    }

    pub fn record_http_status(&mut self, status: u16) {
        if !self.http_status_codes.contains(&status) {
            self.http_status_codes.push(status);
        }
    }

    pub fn set_error(&mut self, err: &DocDownloaderError) {
        self.error_reason = Some(err.to_string());
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), DocDownloaderError> {
        let json = self
            .to_json()
            .map_err(|e| DocDownloaderError::InternalInvariantViolation {
                reason: format!("Failed to serialize diagnostic bundle: {e}"),
            })?;
        std::fs::write(path, json).map_err(|e| DocDownloaderError::FileSystemError {
            path: path.to_path_buf(),
            reason: format!("Failed to write diagnostic bundle: {e}"),
        })?;
        Ok(())
    }
}

/// Strips sensitive authentication, session, or signature tokens from URLs before logging.
pub fn sanitize_diagnostic_url(url: &Url) -> String {
    let mut clean = url.clone();
    let safe_pairs: Vec<(String, String)> = clean
        .query_pairs()
        .filter_map(|(k, v)| {
            let lower = k.to_ascii_lowercase();
            // Disallow known security or session parameters
            if lower.contains("auth")
                || lower.contains("token")
                || lower.contains("sig")
                || lower.contains("secret")
                || lower.contains("key")
                || lower.contains("session")
                || lower.contains("cookie")
            {
                None
            } else {
                Some((k.to_string(), v.to_string()))
            }
        })
        .collect();

    clean.set_query(None);
    if !safe_pairs.is_empty() {
        let mut query_builder = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in safe_pairs {
            query_builder.append_pair(&k, &v);
        }
        clean.set_query(Some(&query_builder.finish()));
    }

    clean.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_diagnostic_url_strips_sensitive_query_params() {
        let sensitive = Url::parse("https://www.calameo.com/read/0061133461a5012e8961a?authid=secret123&bkcode=0061133461a5012e8961a&signature=deadbeef&token=xyz&page=3").unwrap();
        let cleaned = sanitize_diagnostic_url(&sensitive);
        assert!(!cleaned.contains("secret123"));
        assert!(!cleaned.contains("deadbeef"));
        assert!(!cleaned.contains("xyz"));
        assert!(cleaned.contains("bkcode=0061133461a5012e8961a"));
        assert!(cleaned.contains("page=3"));
    }

    #[test]
    fn test_diagnostic_bundle_json_serialization() {
        let url = Url::parse("https://www.calameo.com/read/0061133461a5012e8961a").unwrap();
        let mut bundle = DiagnosticBundle::new("calameo", &url);
        bundle.set_stage("ResolvingMetadata");
        bundle.discovered_page_count = Some(42);
        bundle.record_http_status(200);

        let json = bundle.to_json().unwrap();
        assert!(json.contains("\"tool_name\": \"DocDownloader\""));
        assert!(json.contains("\"provider\": \"calameo\""));
        assert!(json.contains("\"discovered_page_count\": 42"));
        assert!(json.contains("\"stage_reached\": \"ResolvingMetadata\""));
    }
}
