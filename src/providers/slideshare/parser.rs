use regex::Regex;

use crate::core::document::{AssetCandidate, AssetType, PageDescriptor, Publication};
use crate::core::error::DocDownloaderError;
use crate::network::client::HttpClient;
use crate::providers::slideshare::models::SlideShareOEmbedResponse;

const OEMBED_ENDPOINT: &str = "https://www.slideshare.net/api/oembed/2";

/// Resolves SlideShare presentation using the official oEmbed API.
pub async fn resolve_via_oembed(
    client: &HttpClient,
    canonical_url: &str,
    presentation_id: &str,
) -> Result<Publication, DocDownloaderError> {
    let oembed_url = format!("{OEMBED_ENDPOINT}?url={canonical_url}&format=json");
    let headers = vec![("Referer".to_string(), canonical_url.to_string())];

    let resp = match client.get_with_retry(&oembed_url, Some(&headers)).await {
        Ok(r) => r,
        Err(DocDownloaderError::AccessRestricted { id: _, reason }) => {
            return Err(DocDownloaderError::AccessRestricted {
                id: presentation_id.to_string(),
                reason,
            });
        }
        Err(DocDownloaderError::PublicationNotFound { id: _, reason }) => {
            return Err(DocDownloaderError::PublicationNotFound {
                id: presentation_id.to_string(),
                reason,
            });
        }
        Err(e) => return Err(e),
    };

    let body_bytes = resp
        .bytes()
        .await
        .map_err(|_e| DocDownloaderError::NetworkTimeout {
            url: oembed_url.clone(),
            elapsed_secs: 45,
        })?;

    let parsed: SlideShareOEmbedResponse =
        serde_json::from_slice(&body_bytes).map_err(|e| DocDownloaderError::InvalidMetadata {
            id: presentation_id.to_string(),
            reason: format!("Failed to parse SlideShare oEmbed JSON: {e}"),
        })?;

    parse_slideshare_oembed(&parsed, canonical_url, presentation_id)
}

/// Helper to generate candidate resolutions from a SlideShare slide image URL.
fn generate_slide_candidates(
    base_url: &str,
    idx: u32,
    canonical_url: &str,
) -> Vec<AssetCandidate> {
    // Regex matching any slide number and resolution in the filename, e.g. -1-638.jpg or -1-2048.jpg
    let pattern = Regex::new(r#"-\d+-(?:638|1024|2048)\.jpg"#).unwrap();
    let clean_url = base_url.split('?').next().unwrap_or(base_url);

    let mut candidates = Vec::with_capacity(3);

    if pattern.is_match(clean_url) {
        // Priority 1: High definition 2048px
        let url_2048 = pattern
            .replace(clean_url, format!("-{idx}-2048.jpg"))
            .to_string();
        candidates.push(AssetCandidate {
            priority: 1,
            url: url_2048,
            asset_type: AssetType::ImageJpeg,
            geometry: None,
            headers: vec![("Referer".to_string(), canonical_url.to_string())],
        });

        // Priority 2: Standard high resolution 1024px
        let url_1024 = pattern
            .replace(clean_url, format!("-{idx}-1024.jpg"))
            .to_string();
        candidates.push(AssetCandidate {
            priority: 2,
            url: url_1024,
            asset_type: AssetType::ImageJpeg,
            geometry: None,
            headers: vec![("Referer".to_string(), canonical_url.to_string())],
        });

        // Priority 3: Base preview 638px
        let url_638 = pattern
            .replace(clean_url, format!("-{idx}-638.jpg"))
            .to_string();
        candidates.push(AssetCandidate {
            priority: 3,
            url: url_638,
            asset_type: AssetType::ImageJpeg,
            geometry: None,
            headers: vec![("Referer".to_string(), canonical_url.to_string())],
        });
    } else {
        // Fallback: direct candidate
        candidates.push(AssetCandidate {
            priority: 1,
            url: clean_url.to_string(),
            asset_type: AssetType::ImageJpeg,
            geometry: None,
            headers: vec![("Referer".to_string(), canonical_url.to_string())],
        });
    }

    candidates
}

/// Pure parser extracting publication domain model from SlideShare oEmbed data.
pub fn parse_slideshare_oembed(
    resp: &SlideShareOEmbedResponse,
    canonical_url: &str,
    presentation_id: &str,
) -> Result<Publication, DocDownloaderError> {
    let page_count = resp.total_slides.unwrap_or(0);
    if page_count == 0 {
        return Err(DocDownloaderError::PageListInvalid {
            id: presentation_id.to_string(),
            reason: "SlideShare oEmbed reported 0 total slides".to_string(),
        });
    }

    let title = resp
        .title
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(presentation_id);

    let thumb_url = resp.effective_thumbnail();
    let slideshow_id = resp.effective_slideshow_id();

    let mut pages = Vec::with_capacity(page_count as usize);

    for idx in 1..=page_count {
        let candidates = if let Some(thumb) = thumb_url {
            generate_slide_candidates(thumb, idx, canonical_url)
        } else if let Some(ref sid) = slideshow_id {
            vec![
                AssetCandidate {
                    priority: 1,
                    url: format!("https://image.slidesharecdn.com/{sid}/95/slide-{idx}-2048.jpg"),
                    asset_type: AssetType::ImageJpeg,
                    geometry: None,
                    headers: vec![("Referer".to_string(), canonical_url.to_string())],
                },
                AssetCandidate {
                    priority: 2,
                    url: format!("https://image.slidesharecdn.com/{sid}/95/slide-{idx}-1024.jpg"),
                    asset_type: AssetType::ImageJpeg,
                    geometry: None,
                    headers: vec![("Referer".to_string(), canonical_url.to_string())],
                },
                AssetCandidate {
                    priority: 3,
                    url: format!("https://image.slidesharecdn.com/{sid}/95/slide-{idx}-638.jpg"),
                    asset_type: AssetType::ImageJpeg,
                    geometry: None,
                    headers: vec![("Referer".to_string(), canonical_url.to_string())],
                },
            ]
        } else {
            return Err(DocDownloaderError::InvalidMetadata {
                id: presentation_id.to_string(),
                reason: "SlideShare oEmbed missing both thumbnail and slideshow_id".to_string(),
            });
        };

        pages.push(PageDescriptor {
            index: idx,
            geometry: None,
            candidates,
        });
    }

    let publication = Publication {
        provider: "slideshare".to_string(),
        canonical_url: canonical_url.to_string(),
        publication_id: presentation_id.to_string(),
        title: title.to_string(),
        author: resp.author_name.clone(),
        description: None,
        page_count,
        thumbnail_url: thumb_url.map(|s| s.to_string()),
        geometry: None,
        direct_pdf_url: None,
        pages,
    };

    publication
        .validate_completeness()
        .map_err(|e| DocDownloaderError::PageListInvalid {
            id: presentation_id.to_string(),
            reason: e,
        })?;

    Ok(publication)
}

/// Fallback resolution method by fetching and parsing the public SlideShare presentation HTML.
pub async fn resolve_via_html_fallback(
    client: &HttpClient,
    canonical_url: &str,
    presentation_id: &str,
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
    parse_slideshare_html(&html, canonical_url, presentation_id)
}

/// Pure parser extracting publication model from SlideShare public HTML.
pub fn parse_slideshare_html(
    html: &str,
    canonical_url: &str,
    presentation_id: &str,
) -> Result<Publication, DocDownloaderError> {
    // Check private presentation indication
    if html.contains("This presentation is private")
        || html.contains("\"is_private\":true")
        || html.contains("\"privacy\":\"private\"")
    {
        return Err(DocDownloaderError::AccessRestricted {
            id: presentation_id.to_string(),
            reason: "SlideShare page indicates presentation is private".to_string(),
        });
    }

    // Check Next.js __NEXT_DATA__ script if present
    if let Ok(next_data_re) = Regex::new(r#"<script id=["']__NEXT_DATA__["'][^>]*>(.*?)</script>"#)
        && let Some(caps) = next_data_re.captures(html)
        && let Some(json_str) = caps.get(1)
        && let Ok(next_val) = serde_json::from_str::<serde_json::Value>(json_str.as_str())
        && let Some(slideshow) = next_val.pointer("/props/pageProps/slideshow")
    {
        let next_title = slideshow
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or(presentation_id)
            .trim();
        let total_slides = slideshow
            .get("totalSlides")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        if total_slides > 0 {
            let author = slideshow
                .pointer("/user/name")
                .or_else(|| slideshow.get("username"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let description = slideshow
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let thumbnail_url = slideshow
                .get("thumbnail")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let slides_obj = slideshow.get("slides");
            let host = slides_obj
                .and_then(|s| s.get("host"))
                .and_then(|v| v.as_str())
                .unwrap_or("https://image.slidesharecdn.com");
            let image_loc = slides_obj
                .and_then(|s| s.get("imageLocation"))
                .and_then(|v| v.as_str());
            let slide_title = slides_obj
                .and_then(|s| s.get("title"))
                .and_then(|v| v.as_str());

            let mut sizes: Vec<(u32, u32)> = Vec::new();
            if let Some(arr) = slides_obj.and_then(|s| s.get("imageSizes")).and_then(|v| v.as_array()) {
                for item in arr {
                    let width = item.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let quality = item.get("quality").and_then(|v| v.as_u64()).unwrap_or(85) as u32;
                    if width > 0 {
                        sizes.push((width, quality));
                    }
                }
            }
            sizes.sort_by(|a, b| b.0.cmp(&a.0));
            if sizes.is_empty() {
                sizes = vec![(2048, 75), (1024, 85), (638, 85)];
            }

            if let (Some(loc), Some(stitle)) = (image_loc, slide_title) {
                let mut pages = Vec::with_capacity(total_slides as usize);
                for idx in 1..=total_slides {
                    let mut candidates = Vec::with_capacity(sizes.len());
                    for (prio, (w, q)) in sizes.iter().enumerate() {
                        candidates.push(AssetCandidate {
                            priority: (prio + 1) as u32,
                            url: format!("{host}/{loc}/{q}/{stitle}-{idx}-{w}.jpg"),
                            asset_type: AssetType::ImageJpeg,
                            geometry: None,
                            headers: vec![("Referer".to_string(), canonical_url.to_string())],
                        });
                    }
                    pages.push(PageDescriptor {
                        index: idx,
                        geometry: None,
                        candidates,
                    });
                }

                let pub_doc = Publication {
                    provider: "slideshare".to_string(),
                    canonical_url: canonical_url.to_string(),
                    publication_id: presentation_id.to_string(),
                    title: next_title.to_string(),
                    author,
                    description,
                    page_count: total_slides,
                    thumbnail_url,
                    geometry: None,
                    direct_pdf_url: None,
                    pages,
                };

                if pub_doc.validate_completeness().is_ok() {
                    return Ok(pub_doc);
                }
            }
        }
    }

    // Extract title: <meta property="og:title" content="..."> or <title>
    let title_re = Regex::new(r#"<meta\s+property=["']og:title["']\s+content=["'](.*?)["']"#)
        .map_err(|e| DocDownloaderError::InternalInvariantViolation {
            reason: format!("Failed to compile title regex: {e}"),
        })?;
    let title = title_re
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().replace(" | SlideShare", "").trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| presentation_id.to_string());

    // Extract total slides: itemprop="numberOfPages" content="(\d+)" or data-total-slides="(\d+)"
    let total_re = Regex::new(
        r#"(?:itemprop=["']numberOfPages["']\s+content=["'](\d+)["'])|(?:data-total-slides=["'](\d+)["'])|(?:"total_slides":\s*(\d+))"#,
    )
    .map_err(|e| DocDownloaderError::InternalInvariantViolation {
        reason: format!("Failed to compile total slides regex: {e}"),
    })?;

    let page_count = total_re
        .captures(html)
        .and_then(|c| {
            c.get(1)
                .or_else(|| c.get(2))
                .or_else(|| c.get(3))
                .and_then(|m| m.as_str().parse::<u32>().ok())
        })
        .unwrap_or(0);

    // Extract slide image base template from HTML:
    // Look for slidesharecdn.com image URL with pattern -1-638.jpg or -1-2048.jpg or data-full="..."
    let cdn_img_re = Regex::new(
        r#"https?://[^"'\s]+(?:slidesharecdn\.com|sscdn\.co)[^"'\s]+/95/[^"'\s]+-(?:\d+)-(?:638|1024|2048)\.jpg"#,
    )
    .map_err(|e| DocDownloaderError::InternalInvariantViolation {
        reason: format!("Failed to compile cdn_img_re: {e}"),
    })?;

    let template_url = cdn_img_re.captures(html).and_then(|c| c.get(0)).map(|m| m.as_str());

    // Extract slideshow ID if available: data-slideshow-id="(\d+)" or "slideshow_id":\s*"?(\d+)"?
    let sid_re = Regex::new(r#"(?:data-slideshow-id=["'](\d+)["'])|(?:"slideshow_id":\s*"?(\d+)"?)"#)
        .map_err(|e| DocDownloaderError::InternalInvariantViolation {
            reason: format!("Failed to compile sid regex: {e}"),
        })?;
    let slideshow_id = sid_re
        .captures(html)
        .and_then(|c| c.get(1).or_else(|| c.get(2)))
        .map(|m| m.as_str().to_string());

    if page_count == 0 {
        return Err(DocDownloaderError::InvalidMetadata {
            id: presentation_id.to_string(),
            reason: "Could not determine slide count from SlideShare presentation HTML".to_string(),
        });
    }

    let mut pages = Vec::with_capacity(page_count as usize);

    for idx in 1..=page_count {
        let candidates = if let Some(tmpl) = template_url {
            generate_slide_candidates(tmpl, idx, canonical_url)
        } else if let Some(ref sid) = slideshow_id {
            vec![
                AssetCandidate {
                    priority: 1,
                    url: format!("https://image.slidesharecdn.com/{sid}/95/slide-{idx}-2048.jpg"),
                    asset_type: AssetType::ImageJpeg,
                    geometry: None,
                    headers: vec![("Referer".to_string(), canonical_url.to_string())],
                },
                AssetCandidate {
                    priority: 2,
                    url: format!("https://image.slidesharecdn.com/{sid}/95/slide-{idx}-1024.jpg"),
                    asset_type: AssetType::ImageJpeg,
                    geometry: None,
                    headers: vec![("Referer".to_string(), canonical_url.to_string())],
                },
                AssetCandidate {
                    priority: 3,
                    url: format!("https://image.slidesharecdn.com/{sid}/95/slide-{idx}-638.jpg"),
                    asset_type: AssetType::ImageJpeg,
                    geometry: None,
                    headers: vec![("Referer".to_string(), canonical_url.to_string())],
                },
            ]
        } else {
            return Err(DocDownloaderError::InvalidMetadata {
                id: presentation_id.to_string(),
                reason: "Could not determine slide image assets from SlideShare presentation HTML"
                    .to_string(),
            });
        };

        pages.push(PageDescriptor {
            index: idx,
            geometry: None,
            candidates,
        });
    }

    let thumbnail_url = template_url.map(|s| s.to_string());

    let publication = Publication {
        provider: "slideshare".to_string(),
        canonical_url: canonical_url.to_string(),
        publication_id: presentation_id.to_string(),
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
            id: presentation_id.to_string(),
            reason: e,
        })?;

    Ok(publication)
}
