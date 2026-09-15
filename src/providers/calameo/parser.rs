use std::time::{SystemTime, UNIX_EPOCH};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue};
use url::Url;

use crate::core::document::{AssetCandidate, AssetType, PageDescriptor, PageGeometry, Publication};
use crate::core::error::DocDownloaderError;
use crate::network::client::HttpClient;
use crate::providers::calameo::models::CalameoResponse;
use crate::providers::calameo::signature::CalameoSignature;

const BOOK_GET_ENDPOINT: &str = "https://d.calameo.com/pinwheel/viewer/book/get";
const CALAMEO_BUILD_ID: &str = "9639-2b0066";

/// Primary resolution method querying Calaméo's structured reader API endpoint.
pub async fn resolve_via_book_api(
    client: &HttpClient,
    publication_id: &str,
    canonical_url: &str,
) -> Result<Publication, DocDownloaderError> {
    let url = format!("{BOOK_GET_ENDPOINT}?bkcode={publication_id}");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let headers = vec![
        ("X-Calameo-Build-ID".to_string(), CALAMEO_BUILD_ID.to_string()),
        ("X-Calameo-Timestamp".to_string(), now.to_string()),
        ("Referer".to_string(), format!("https://v.calameo.com/?bkcode={publication_id}")),
    ];

    let resp = match client.get_with_retry(&url, Some(&headers)).await {
        Ok(r) => r,
        Err(DocDownloaderError::AccessRestricted { id: _, reason }) => {
            return Err(DocDownloaderError::AccessRestricted {
                id: publication_id.to_string(),
                reason,
            });
        }
        Err(DocDownloaderError::PublicationNotFound { id: _, reason }) => {
            return Err(DocDownloaderError::PublicationNotFound {
                id: publication_id.to_string(),
                reason,
            });
        }
        Err(e) => return Err(e),
    };

    let sig = CalameoSignature::from_headers(resp.headers());
    let body_bytes = resp.bytes().await.map_err(|e| DocDownloaderError::NetworkTimeout {
        url: url.clone(),
        elapsed_secs: 45,
    })?;

    let parsed: CalameoResponse = serde_json::from_slice(&body_bytes).map_err(|e| {
        DocDownloaderError::InvalidMetadata {
            id: publication_id.to_string(),
            reason: format!("Failed to parse Calaméo book JSON: {e}"),
        }
    })?;

    if parsed.status != "ok" {
        // Check for known error codes
        let err_text = String::from_utf8_lossy(&body_bytes);
        if err_text.contains("Unknown book") || err_text.contains("\"code\":101") {
            return Err(DocDownloaderError::PublicationNotFound {
                id: publication_id.to_string(),
                reason: "Book code not found on Calaméo".to_string(),
            });
        }
        if err_text.contains("Access denied") || err_text.contains("\"code\":403") {
            return Err(DocDownloaderError::AccessRestricted {
                id: publication_id.to_string(),
                reason: "Public access restricted by publisher".to_string(),
            });
        }
        return Err(DocDownloaderError::InvalidMetadata {
            id: publication_id.to_string(),
            reason: format!("Calaméo returned error status: {err_text}"),
        });
    }

    let content = parsed.content.ok_or_else(|| DocDownloaderError::InvalidMetadata {
        id: publication_id.to_string(),
        reason: "Calaméo API response missing content block".to_string(),
    })?;

    // Check if mode is private or subscriber-only
    if content.mode == "private" {
        return Err(DocDownloaderError::AccessRestricted {
            id: publication_id.to_string(),
            reason: "Publication mode is set to private".to_string(),
        });
    }
    if let Some(sub) = &content.features.as_ref().and_then(|f| f.subscribers.as_ref()) {
        if sub.enabled == Some(true) && sub.access == Some(false) {
            return Err(DocDownloaderError::AccessRestricted {
                id: publication_id.to_string(),
                reason: "Publication is restricted to subscribers".to_string(),
            });
        }
    }

    let doc = content.document.ok_or_else(|| DocDownloaderError::InvalidMetadata {
        id: publication_id.to_string(),
        reason: "Missing document specifications in Calaméo metadata".to_string(),
    })?;

    let page_count = doc.pages.unwrap_or(0);
    if page_count == 0 {
        return Err(DocDownloaderError::PageListInvalid {
            id: publication_id.to_string(),
            reason: "Calaméo reported 0 pages for publication".to_string(),
        });
    }

    let base_geometry = match (doc.width, doc.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => Some(PageGeometry::new(w, h)),
        _ => None,
    };

    let key = if content.key.is_empty() {
        publication_id.to_string()
    } else {
        content.key
    };

    // Determine domain paths
    let secured_image_domain = content
        .domains
        .as_ref()
        .and_then(|d| d.secured.as_ref())
        .and_then(|s| s.image.as_deref())
        .unwrap_or("https://ps.calameoassets.com/");

    let secured_svg_domain = content
        .domains
        .as_ref()
        .and_then(|d| d.secured.as_ref())
        .and_then(|s| s.svg.as_deref())
        .unwrap_or("https://ps.calameoassets.com/");

    let thumb_domain = content
        .domains
        .as_ref()
        .and_then(|d| d.thumbnail.as_deref())
        .unwrap_or("http://i.calameoassets.com/");

    // Check direct PDF download feature
    let direct_pdf_url = content
        .features
        .as_ref()
        .and_then(|f| f.download.as_ref())
        .filter(|d| d.enabled)
        .and_then(|d| d.url.clone());

    let mut pages = Vec::with_capacity(page_count as usize);

    for idx in 1..=page_count {
        let mut candidates = Vec::new();

        // 1. Direct PDF if enabled (priority 0)
        if let Some(ref pdf_url) = direct_pdf_url {
            candidates.push(AssetCandidate {
                priority: 0,
                url: pdf_url.clone(),
                asset_type: AssetType::DirectPdf,
                geometry: base_geometry,
                headers: Vec::new(),
            });
        }

        // 2. High-resolution JPEG page asset (priority 1)
        let raw_jpg_url = format!("{secured_image_domain}{key}/p{idx}.jpg");
        let signed_jpg_url = if let Some(ref s) = sig {
            s.sign_url(&raw_jpg_url)
        } else {
            raw_jpg_url
        };
        candidates.push(AssetCandidate {
            priority: 1,
            url: signed_jpg_url,
            asset_type: AssetType::ImageJpeg,
            geometry: base_geometry,
            headers: vec![("Referer".to_string(), canonical_url.to_string())],
        });

        // 3. Fallback SVG asset (priority 2)
        let raw_svg_url = format!("{secured_svg_domain}{key}/p{idx}.svgz");
        let signed_svg_url = if let Some(ref s) = sig {
            s.sign_url(&raw_svg_url)
        } else {
            raw_svg_url
        };
        candidates.push(AssetCandidate {
            priority: 2,
            url: signed_svg_url,
            asset_type: AssetType::VectorSvg,
            geometry: base_geometry,
            headers: vec![("Referer".to_string(), canonical_url.to_string())],
        });

        // 4. Fallback thumbnail (priority 3)
        let thumb_url = format!("{thumb_domain}{key}/p{idx}.jpg");
        candidates.push(AssetCandidate {
            priority: 3,
            url: thumb_url,
            asset_type: AssetType::Thumbnail,
            geometry: base_geometry,
            headers: vec![("Referer".to_string(), canonical_url.to_string())],
        });

        pages.push(PageDescriptor {
            index: idx,
            geometry: base_geometry,
            candidates,
        });
    }

    let publication = Publication {
        provider: "calameo".to_string(),
        canonical_url: canonical_url.to_string(),
        publication_id: publication_id.to_string(),
        title: if content.name.is_empty() {
            format!("Calameo_{publication_id}")
        } else {
            content.name
        },
        author: content.account.and_then(|a| a.name),
        description: None,
        page_count,
        thumbnail_url: Some(format!("{thumb_domain}{key}/p1.jpg")),
        geometry: base_geometry,
        direct_pdf_url,
        pages,
    };

    publication.validate_completeness().map_err(|e| {
        DocDownloaderError::PageListInvalid {
            id: publication_id.to_string(),
            reason: e,
        }
    })?;

    Ok(publication)
}

/// Fallback metadata extraction by parsing the public reader HTML.
pub async fn resolve_via_html_fallback(
    client: &HttpClient,
    publication_id: &str,
    canonical_url: &str,
) -> Result<Publication, DocDownloaderError> {
    let reader_url = format!("https://www.calameo.com/read/{publication_id}");
    let resp = client.get_with_retry(&reader_url, None).await?;
    let body_bytes = resp.bytes().await.map_err(|e| DocDownloaderError::NetworkTimeout {
        url: reader_url.clone(),
        elapsed_secs: 45,
    })?;
    let html = String::from_utf8_lossy(&body_bytes);

    // Check for private / access restricted indications in HTML
    if html.contains("This document is private") || html.contains("Ce document est privé") {
        return Err(DocDownloaderError::AccessRestricted {
            id: publication_id.to_string(),
            reason: "Reader page indicates publication is private".to_string(),
        });
    }

    // Extract title: <meta property="og:title" content="..."> or <title>...</title>
    let title_re = Regex::new(r#"<meta\s+property=["']og:title["']\s+content=["'](.*?)["']"#).unwrap();
    let title = title_re
        .captures(&html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| format!("Calameo_{publication_id}"));

    // Extract description & page count: Length:\s*(\d+)\s*pages?
    let length_re = Regex::new(r#"Length:\s*(\d+)\s*pages?"#).unwrap();
    let page_count = length_re
        .captures(&html)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .unwrap_or(0);

    if page_count == 0 {
        return Err(DocDownloaderError::InvalidMetadata {
            id: publication_id.to_string(),
            reason: "Could not determine page count from reader HTML".to_string(),
        });
    }

    // Extract asset key and token from image_src:
    // <link rel="image_src" href="https://ps.calameoassets.com/([0-9a-fA-F-]+)/p1.jpg(\?_token_=.*?)?"
    let img_re = Regex::new(r#"https://ps\.calameoassets\.com/([a-zA-Z0-9_-]+)/p1\.jpg(?:\?_token_=([^\s"'>]+))?"#).unwrap();
    let (key, token_opt) = if let Some(caps) = img_re.captures(&html) {
        let key = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_else(|| publication_id.to_string());
        let token = caps.get(2).map(|m| m.as_str().to_string());
        (key, token)
    } else {
        (publication_id.to_string(), None)
    };

    let mut pages = Vec::with_capacity(page_count as usize);
    for idx in 1..=page_count {
        let base_jpg = format!("https://ps.calameoassets.com/{key}/p{idx}.jpg");
        let signed_jpg = if let Some(ref tok) = token_opt {
            format!("{base_jpg}?_token_={tok}")
        } else {
            base_jpg
        };

        pages.push(PageDescriptor {
            index: idx,
            geometry: None,
            candidates: vec![
                AssetCandidate {
                    priority: 1,
                    url: signed_jpg,
                    asset_type: AssetType::ImageJpeg,
                    geometry: None,
                    headers: vec![("Referer".to_string(), canonical_url.to_string())],
                },
                AssetCandidate {
                    priority: 3,
                    url: format!("http://i.calameoassets.com/{key}/p{idx}.jpg"),
                    asset_type: AssetType::Thumbnail,
                    geometry: None,
                    headers: vec![("Referer".to_string(), canonical_url.to_string())],
                },
            ],
        });
    }

    let pub_doc = Publication {
        provider: "calameo".to_string(),
        canonical_url: canonical_url.to_string(),
        publication_id: publication_id.to_string(),
        title,
        author: None,
        description: None,
        page_count,
        thumbnail_url: Some(format!("http://i.calameoassets.com/{key}/p1.jpg")),
        geometry: None,
        direct_pdf_url: None,
        pages,
    };

    pub_doc.validate_completeness().map_err(|e| {
        DocDownloaderError::PageListInvalid {
            id: publication_id.to_string(),
            reason: e,
        }
    })?;

    Ok(pub_doc)
}
