use regex::Regex;

use crate::core::document::{AssetCandidate, AssetType, PageDescriptor, Publication};
use crate::core::error::DocDownloaderError;
use crate::network::client::HttpClient;

const EMBED_BASE_URL: &str = "https://www.scribd.com/embeds";

/// Primary resolution method querying Scribd's public embed viewer HTML.
pub async fn resolve_via_embed(
    client: &HttpClient,
    doc_id: &str,
    canonical_url: &str,
) -> Result<Publication, DocDownloaderError> {
    let embed_url = format!("{EMBED_BASE_URL}/{doc_id}/content?start_page=1&view_mode=scroll");
    let headers = vec![(
        "Referer".to_string(),
        format!("https://www.scribd.com/document/{doc_id}"),
    )];

    let resp = match client.get_with_retry(&embed_url, Some(&headers)).await {
        Ok(r) => r,
        Err(DocDownloaderError::AccessRestricted { id: _, reason }) => {
            return Err(DocDownloaderError::AccessRestricted {
                id: doc_id.to_string(),
                reason,
            });
        }
        Err(DocDownloaderError::PublicationNotFound { id: _, reason }) => {
            return Err(DocDownloaderError::PublicationNotFound {
                id: doc_id.to_string(),
                reason,
            });
        }
        Err(e) => return Err(e),
    };

    let body_bytes = resp
        .bytes()
        .await
        .map_err(|_e| DocDownloaderError::NetworkTimeout {
            url: embed_url.clone(),
            elapsed_secs: 45,
        })?;

    let html = String::from_utf8_lossy(&body_bytes);
    parse_scribd_embed_html(&html, doc_id, canonical_url)
}

/// Pure parser extracting publication model from Scribd embed HTML.
pub fn parse_scribd_embed_html(
    html: &str,
    doc_id: &str,
    canonical_url: &str,
) -> Result<Publication, DocDownloaderError> {
    // Check access restrictions or removed document indicators
    if html.contains("This document is private")
        || html.contains("This document has been removed")
        || html.contains("document_removed")
        || html.contains("\"is_private\":true")
        || html.contains("\"access\":\"private\"")
        || html.contains("\"access\": \"private\"")
    {
        return Err(DocDownloaderError::AccessRestricted {
            id: doc_id.to_string(),
            reason: "Scribd document access is private, removed, or restricted".to_string(),
        });
    }

    // Extract title: <meta property="og:title" content="..."> or <title>...</title>
    let title_re = Regex::new(
        r#"(?:<meta\s+property=["']og:title["']\s+content=["'](.*?)["'])|(?:"title":\s*"([^"]+)")|(?:<title>(.*?)</title>)"#,
    )
    .map_err(|e| DocDownloaderError::InternalInvariantViolation {
        reason: format!("Failed to compile title regex: {e}"),
    })?;

    let title = title_re
        .captures(html)
        .and_then(|c| c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)))
        .map(|m| {
            m.as_str()
                .replace(" | Scribd", "")
                .replace(" - Scribd", "")
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty() && s != "Scribd")
        .unwrap_or_else(|| format!("Scribd_{doc_id}"));

    // Extract page count: data-e2e="total-pages", .pageCount, or "pageCount":\s*(\d+)
    let page_count_re = Regex::new(
        r#"(?:data-e2e=["']total-pages["'][^>]*>(\d+))|(?:"pageCount":\s*(\d+))|(?:"total_pages":\s*(\d+))|(?:"page_count":\s*(\d+))"#,
    )
    .map_err(|e| DocDownloaderError::InternalInvariantViolation {
        reason: format!("Failed to compile page count regex: {e}"),
    })?;

    let page_count = page_count_re
        .captures(html)
        .and_then(|c| {
            c.get(1)
                .or_else(|| c.get(2))
                .or_else(|| c.get(3))
                .or_else(|| c.get(4))
                .and_then(|m| m.as_str().parse::<u32>().ok())
        })
        .or_else(|| {
            // Count occurrence of outer_page divs in the DOM
            let outer_page_re = Regex::new(r#"class=["'][^"']*outer_page[^"']*["']"#).ok()?;
            let count = outer_page_re.find_iter(html).count() as u32;
            if count > 0 { Some(count) } else { None }
        })
        .unwrap_or(0);

    if page_count == 0 {
        return Err(DocDownloaderError::PageListInvalid {
            id: doc_id.to_string(),
            reason: "Could not determine page count from Scribd embed HTML".to_string(),
        });
    }

    // Extract content key from html.scribd(assets).com/{key}/
    let asset_key_re = Regex::new(r#"https?://html\.scribd(?:assets)?\.com/([a-zA-Z0-9_-]+)/"#)
        .map_err(|e| DocDownloaderError::InternalInvariantViolation {
            reason: format!("Failed to compile asset_key_re: {e}"),
        })?;
    let content_key = asset_key_re
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    // Extract token-based document CDN image template:
    // e.g. https://imgv2-1-f.scribdassets.com/img/document/448327126/original/009a8fbd77/1?v=1
    let thumb_host_re =
        Regex::new(r#"https?://([^"'\s/]+)/img/document/\d+/original/([a-zA-Z0-9_-]+)/"#).ok();
    let doc_token = thumb_host_re
        .as_ref()
        .and_then(|re| re.captures(html))
        .and_then(|c| match (c.get(1), c.get(2)) {
            (Some(h), Some(t)) => Some((h.as_str().to_string(), t.as_str().to_string())),
            _ => None,
        });

    let img_src_re = Regex::new(
        r#"<img[^>]*class=["'][^"']*absimg[^"']*["'][^>]*(?:src|orig|data-src)=["']([^"']+)["']"#,
    )
    .ok();

    let mut pages = Vec::with_capacity(page_count as usize);

    for idx in 1..=page_count {
        let mut candidates = Vec::new();
        let mut priority = 0;

        // Priority 0: Token-based high-res document image from CDN if present (verified live structure)
        if let Some((ref host, ref token)) = doc_token {
            candidates.push(AssetCandidate {
                priority,
                url: format!("https://{host}/img/document/{doc_id}/original/{token}/{idx}?v=1"),
                asset_type: AssetType::ImageJpeg,
                geometry: None,
                headers: vec![("Referer".to_string(), canonical_url.to_string())],
            });
            priority += 1;

            candidates.push(AssetCandidate {
                priority,
                url: format!("https://{host}/img/document/{doc_id}/original/{token}/{idx}"),
                asset_type: AssetType::ImageJpeg,
                geometry: None,
                headers: vec![("Referer".to_string(), canonical_url.to_string())],
            });
            priority += 1;
        }

        // Check if page element in HTML has direct absimg src/orig URL
        let page_div_pattern = format!(
            r#"(?:id=["'](?:outer_)?page_?{idx}["'][^>]*>)([\s\S]*?)(?:</div>\s*</div>|class=["'](?:outer_)?page)"#
        );
        if let Ok(re) = Regex::new(&page_div_pattern)
            && let Some(caps) = re.captures(html)
            && let Some(inner) = caps.get(1)
            && let Some(ref img_re) = img_src_re
            && let Some(img_caps) = img_re.captures(inner.as_str())
            && let Some(src) = img_caps.get(1)
        {
            let raw_src = src.as_str().to_string();
            if !raw_src.contains("loading") && !raw_src.is_empty() {
                candidates.push(AssetCandidate {
                    priority,
                    url: raw_src,
                    asset_type: AssetType::ImageJpeg,
                    geometry: None,
                    headers: vec![("Referer".to_string(), canonical_url.to_string())],
                });
                priority += 1;
            }
        }

        // Key-based asset if content key is detected
        if let Some(ref key) = content_key {
            candidates.push(AssetCandidate {
                priority,
                url: format!("https://html.scribdassets.com/{key}/images/{idx}.jpg"),
                asset_type: AssetType::ImageJpeg,
                geometry: None,
                headers: vec![("Referer".to_string(), canonical_url.to_string())],
            });
            priority += 1;

            candidates.push(AssetCandidate {
                priority,
                url: format!("https://html.scribdassets.com/{key}/pages/{idx}.jpg"),
                asset_type: AssetType::ImageJpeg,
                geometry: None,
                headers: vec![("Referer".to_string(), canonical_url.to_string())],
            });
            priority += 1;
        }

        // Standard document CDN fallback
        candidates.push(AssetCandidate {
            priority,
            url: format!(
                "https://imgv2-1-f.scribdassets.com/img/document/{doc_id}/original/{idx}.jpg"
            ),
            asset_type: AssetType::ImageJpeg,
            geometry: None,
            headers: vec![("Referer".to_string(), canonical_url.to_string())],
        });

        pages.push(PageDescriptor {
            index: idx,
            geometry: None,
            candidates,
        });
    }

    let thumbnail_url = if let Some((ref host, ref token)) = doc_token {
        Some(format!(
            "https://{host}/img/document/{doc_id}/original/{token}/1?v=1"
        ))
    } else {
        pages
            .first()
            .and_then(|p| p.candidates.first())
            .map(|c| c.url.clone())
    };

    let publication = Publication {
        provider: "scribd".to_string(),
        canonical_url: canonical_url.to_string(),
        publication_id: doc_id.to_string(),
        title,
        author: None,
        description: None,
        page_count,
        thumbnail_url,
        geometry: None,
        direct_pdf_url: None,
        pages,
    };

    publication
        .validate_completeness()
        .map_err(|e| DocDownloaderError::PageListInvalid {
            id: doc_id.to_string(),
            reason: e,
        })?;

    Ok(publication)
}

/// Fallback resolution method by fetching and parsing the public Scribd document page.
pub async fn resolve_via_doc_page(
    client: &HttpClient,
    doc_id: &str,
    canonical_url: &str,
) -> Result<Publication, DocDownloaderError> {
    let resp = client.get_with_retry(canonical_url, None).await?;
    let body_bytes = resp
        .bytes()
        .await
        .map_err(|_e| DocDownloaderError::NetworkTimeout {
            url: canonical_url.to_string(),
            elapsed_secs: 45,
        })?;
    let html = String::from_utf8_lossy(&body_bytes);
    parse_scribd_doc_html(&html, doc_id, canonical_url)
}

/// Pure parser extracting publication model from public Scribd document page HTML.
pub fn parse_scribd_doc_html(
    html: &str,
    doc_id: &str,
    canonical_url: &str,
) -> Result<Publication, DocDownloaderError> {
    parse_scribd_embed_html(html, doc_id, canonical_url)
}
