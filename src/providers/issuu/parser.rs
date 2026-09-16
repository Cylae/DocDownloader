use regex::Regex;

use crate::core::document::{AssetCandidate, AssetType, PageDescriptor, PageGeometry, Publication};
use crate::core::error::DocDownloaderError;
use crate::network::client::HttpClient;
use crate::providers::issuu::models::IssuuReaderManifest;

/// Resolves Issuu publication using the official reader3 manifest endpoint.
pub async fn resolve_via_reader_manifest(
    client: &HttpClient,
    username: &str,
    doc_slug: &str,
    canonical_url: &str,
) -> Result<Publication, DocDownloaderError> {
    let manifest_url = format!("https://reader3.isu.pub/{username}/{doc_slug}/reader3_4.json");
    let headers = vec![("Referer".to_string(), canonical_url.to_string())];

    let resp = match client.get_with_retry(&manifest_url, Some(&headers)).await {
        Ok(r) => r,
        Err(DocDownloaderError::AccessRestricted { id: _, reason }) => {
            return Err(DocDownloaderError::AccessRestricted {
                id: format!("{username}/{doc_slug}"),
                reason,
            });
        }
        Err(DocDownloaderError::PublicationNotFound { id: _, reason }) => {
            return Err(DocDownloaderError::PublicationNotFound {
                id: format!("{username}/{doc_slug}"),
                reason,
            });
        }
        Err(e) => return Err(e),
    };

    let body_bytes = resp
        .bytes()
        .await
        .map_err(|_e| DocDownloaderError::NetworkTimeout {
            url: manifest_url.clone(),
            elapsed_secs: 45,
        })?;

    let parsed: IssuuReaderManifest =
        serde_json::from_slice(&body_bytes).map_err(|e| DocDownloaderError::InvalidMetadata {
            id: format!("{username}/{doc_slug}"),
            reason: format!("Failed to parse Issuu reader manifest JSON: {e}"),
        })?;

    parse_issuu_reader_manifest(&parsed, username, doc_slug, canonical_url)
}

/// Pure parser extracting publication domain model from IssuuReaderManifest.
pub fn parse_issuu_reader_manifest(
    manifest: &IssuuReaderManifest,
    username: &str,
    doc_slug: &str,
    canonical_url: &str,
) -> Result<Publication, DocDownloaderError> {
    let publication_id = format!("{username}/{doc_slug}");

    if manifest.is_private_access() {
        return Err(DocDownloaderError::AccessRestricted {
            id: publication_id,
            reason: "Issuu reader manifest reports publication access is private".to_string(),
        });
    }

    let page_count = manifest.effective_page_count();
    if page_count == 0 {
        return Err(DocDownloaderError::PageListInvalid {
            id: publication_id,
            reason: "Issuu reader manifest declares 0 pages".to_string(),
        });
    }

    let title = manifest
        .effective_title()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(doc_slug);

    let pub_hash = manifest.effective_publication_id();
    let rev_id = manifest.effective_revision_id();

    let pages_slice = manifest.effective_pages();
    let mut pages = Vec::with_capacity(page_count as usize);

    for idx in 1..=page_count {
        let mut candidates = Vec::new();
        let page_data = pages_slice.and_then(|p| {
            p.iter()
                .find(|d| d.page_number == Some(idx))
                .or_else(|| p.get((idx - 1) as usize))
        });

        let page_geometry = page_data.and_then(|d| match (d.width, d.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => Some(PageGeometry::new(w, h)),
            _ => None,
        });

        // Priority 0: Image URI declared directly in manifest page data
        if let Some(data) = page_data
            && let Some(ref uri) = data.image_uri
            && !uri.is_empty()
        {
            let full_url = if uri.starts_with("http://") || uri.starts_with("https://") {
                uri.clone()
            } else if uri.starts_with("//") {
                format!("https:{uri}")
            } else {
                format!("https://reader3.isu.pub/{username}/{doc_slug}/{uri}")
            };

            candidates.push(AssetCandidate {
                priority: 0,
                url: full_url,
                asset_type: AssetType::ImageJpeg,
                geometry: page_geometry,
                headers: vec![("Referer".to_string(), canonical_url.to_string())],
            });
        }

        // Priority 1: Canonical CDN format: https://image.isu.pub/{revision_id}-{publication_id}/jpg/page_{idx}.jpg
        if let (Some(r), Some(p)) = (rev_id.as_ref(), pub_hash) {
            candidates.push(AssetCandidate {
                priority: 1,
                url: format!("https://image.isu.pub/{r}-{p}/jpg/page_{idx}.jpg"),
                asset_type: AssetType::ImageJpeg,
                geometry: page_geometry,
                headers: vec![("Referer".to_string(), canonical_url.to_string())],
            });
        }

        // Priority 2: Direct hash CDN: https://image.isu.pub/{publication_id}/jpg/page_{idx}.jpg
        if let Some(p) = pub_hash {
            candidates.push(AssetCandidate {
                priority: 2,
                url: format!("https://image.isu.pub/{p}/jpg/page_{idx}.jpg"),
                asset_type: AssetType::ImageJpeg,
                geometry: page_geometry,
                headers: vec![("Referer".to_string(), canonical_url.to_string())],
            });

            // Priority 3: Legacy image domain fallback
            candidates.push(AssetCandidate {
                priority: 3,
                url: format!("https://image.issuu.com/{p}/jpg/page_{idx}.jpg"),
                asset_type: AssetType::ImageJpeg,
                geometry: page_geometry,
                headers: vec![("Referer".to_string(), canonical_url.to_string())],
            });
        }

        // Priority 4: Reader host asset fallback
        candidates.push(AssetCandidate {
            priority: 4,
            url: format!("https://reader3.isu.pub/{username}/{doc_slug}/page_{idx}.jpg"),
            asset_type: AssetType::Thumbnail,
            geometry: page_geometry,
            headers: vec![("Referer".to_string(), canonical_url.to_string())],
        });

        pages.push(PageDescriptor {
            index: idx,
            geometry: page_geometry,
            candidates,
        });
    }

    let first_geometry = pages.first().and_then(|p| p.geometry);
    let cover_url = manifest
        .cover_url
        .clone()
        .or_else(|| pages.first()?.candidates.first().map(|c| c.url.clone()));

    let publication = Publication {
        provider: "issuu".to_string(),
        canonical_url: canonical_url.to_string(),
        publication_id,
        title: title.to_string(),
        author: Some(username.to_string()),
        description: manifest.description.clone(),
        page_count,
        thumbnail_url: cover_url,
        geometry: first_geometry,
        direct_pdf_url: None,
        pages,
    };

    publication
        .validate_completeness()
        .map_err(|e| DocDownloaderError::PageListInvalid {
            id: format!("{username}/{doc_slug}"),
            reason: e,
        })?;

    Ok(publication)
}

/// Fallback resolution method by fetching and parsing the public Issuu publication HTML.
pub async fn resolve_via_html_fallback(
    client: &HttpClient,
    username: &str,
    doc_slug: &str,
    canonical_url: &str,
) -> Result<Publication, DocDownloaderError> {
    let reader_url = format!("https://issuu.com/{username}/docs/{doc_slug}");
    let resp = client.get_with_retry(&reader_url, None).await?;
    let body_bytes = resp
        .bytes()
        .await
        .map_err(|_e| DocDownloaderError::NetworkTimeout {
            url: reader_url.clone(),
            elapsed_secs: 45,
        })?;
    let html = String::from_utf8_lossy(&body_bytes);
    parse_issuu_reader_html(&html, username, doc_slug, canonical_url)
}

/// Pure parser extracting publication model from public reader HTML.
pub fn parse_issuu_reader_html(
    html: &str,
    username: &str,
    doc_slug: &str,
    canonical_url: &str,
) -> Result<Publication, DocDownloaderError> {
    let publication_id = format!("{username}/{doc_slug}");

    // Check for explicit access restricted indicators
    if html.contains("This document is private")
        || html.contains("\"isPrivate\":true")
        || html.contains("\"access\":\"private\"")
        || html.contains("isAccessRestricted\\\":true")
    {
        return Err(DocDownloaderError::AccessRestricted {
            id: publication_id,
            reason: "Issuu page indicates document access is private or restricted".to_string(),
        });
    }

    // Extract title: <meta property="og:title" content="..."> or <title>
    let title_re = Regex::new(r#"<meta\s+property=["']og:title["']\s+content=["'](.*?)["']"#)
        .map_err(|e| DocDownloaderError::InternalInvariantViolation {
            reason: format!("Failed to compile title regex: {e}"),
        })?;

    let title = title_re
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| doc_slug.to_string());

    // Extract page count: "pageCount":123 or pageCount\":123 or Length: 123 pages
    let page_count_re = Regex::new(
        r#"(?:"pageCount"|pageCount\\"):(\d+)|(?:Length:\s*(\d+)\s*pages?)|(?:"totalPages":(\d+))"#,
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
                .and_then(|m| m.as_str().parse::<u32>().ok())
        })
        .unwrap_or(0);

    if page_count == 0 {
        return Err(DocDownloaderError::InvalidMetadata {
            id: publication_id,
            reason: "Could not determine page count from Issuu reader HTML".to_string(),
        });
    }

    // Extract publicationId (hash) & revisionId
    let pub_id_re = Regex::new(r#"(?:"publicationId"|publicationId\\"):\s*\\?"([a-zA-Z0-9_-]+)\\?""#)
        .map_err(|e| DocDownloaderError::InternalInvariantViolation {
            reason: format!("Failed to compile publicationId regex: {e}"),
        })?;
    let pub_hash = pub_id_re
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    let rev_id_re = Regex::new(r#"(?:"revisionId"|revisionId\\"):\s*\\?"?(\d+)\\?"?"#).map_err(
        |e| DocDownloaderError::InternalInvariantViolation {
            reason: format!("Failed to compile revisionId regex: {e}"),
        },
    )?;
    let rev_id = rev_id_re
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    // Extract direct CDN prefix if an image.isu.pub URL is present in the HTML
    let direct_cdn_prefix = Regex::new(r#"https?://image\.isu\.pub/([0-9a-fA-F-]+)/jpg/"#)
        .ok()
        .and_then(|re| re.captures(html))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    let mut pages = Vec::with_capacity(page_count as usize);

    for idx in 1..=page_count {
        let mut candidates = Vec::new();

        if let Some(ref prefix) = direct_cdn_prefix {
            candidates.push(AssetCandidate {
                priority: 0,
                url: format!("https://image.isu.pub/{prefix}/jpg/page_{idx}.jpg"),
                asset_type: AssetType::ImageJpeg,
                geometry: None,
                headers: vec![("Referer".to_string(), canonical_url.to_string())],
            });
        }

        if let (Some(r), Some(p)) = (rev_id.as_ref(), pub_hash.as_ref()) {
            candidates.push(AssetCandidate {
                priority: 1,
                url: format!("https://image.isu.pub/{r}-{p}/jpg/page_{idx}.jpg"),
                asset_type: AssetType::ImageJpeg,
                geometry: None,
                headers: vec![("Referer".to_string(), canonical_url.to_string())],
            });
        }

        if let Some(p) = pub_hash.as_ref() {
            candidates.push(AssetCandidate {
                priority: 2,
                url: format!("https://image.isu.pub/{p}/jpg/page_{idx}.jpg"),
                asset_type: AssetType::ImageJpeg,
                geometry: None,
                headers: vec![("Referer".to_string(), canonical_url.to_string())],
            });

            candidates.push(AssetCandidate {
                priority: 3,
                url: format!("https://image.issuu.com/{p}/jpg/page_{idx}.jpg"),
                asset_type: AssetType::ImageJpeg,
                geometry: None,
                headers: vec![("Referer".to_string(), canonical_url.to_string())],
            });
        }

        candidates.push(AssetCandidate {
            priority: 4,
            url: format!("https://reader3.isu.pub/{username}/{doc_slug}/page_{idx}.jpg"),
            asset_type: AssetType::Thumbnail,
            geometry: None,
            headers: vec![("Referer".to_string(), canonical_url.to_string())],
        });

        pages.push(PageDescriptor {
            index: idx,
            geometry: None,
            candidates,
        });
    }

    let thumbnail_url = pages.first().and_then(|p| p.candidates.first()).map(|c| c.url.clone());

    let publication = Publication {
        provider: "issuu".to_string(),
        canonical_url: canonical_url.to_string(),
        publication_id,
        title,
        author: Some(username.to_string()),
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
            id: format!("{username}/{doc_slug}"),
            reason: e,
        })?;

    Ok(publication)
}
