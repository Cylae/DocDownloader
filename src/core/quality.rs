use std::fmt;

use crate::core::document::{AssetType, Publication};
use crate::core::job::CompletedPageAsset;

/// Represents a contiguous range of pages sharing identical resolution and asset type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualitySegment {
    pub start_page: u32,
    pub end_page: u32,
    pub width: u32,
    pub height: u32,
    pub asset_type: AssetType,
    pub is_fallback: bool,
}

impl QualitySegment {
    pub fn page_range_string(&self) -> String {
        if self.start_page == self.end_page {
            format!("{}", self.start_page)
        } else {
            format!("{}–{}", self.start_page, self.end_page)
        }
    }
}

/// Aggregated publication quality breakdown across all pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityReport {
    pub segments: Vec<QualitySegment>,
    pub has_fallbacks: bool,
    pub fallback_count: u32,
}

impl QualityReport {
    /// Generates a quality report from a resolved Publication descriptor before download.
    pub fn from_publication(publication: &Publication) -> Self {
        let mut segments: Vec<QualitySegment> = Vec::new();
        let mut fallback_count = 0;

        for page in &publication.pages {
            let best = page.best_candidate();
            let (w, h) = best
                .and_then(|c| c.geometry)
                .map(|g| (g.width, g.height))
                .unwrap_or((0, 0));
            let asset_type = best.map(|c| c.asset_type).unwrap_or(AssetType::Thumbnail);
            let is_fallback = best.map(|c| c.priority > 1).unwrap_or(true);

            if is_fallback {
                fallback_count += 1;
            }

            if let Some(last) = segments.last_mut()
                && last.width == w
                && last.height == h
                && last.asset_type == asset_type
                && last.is_fallback == is_fallback
                && last.end_page + 1 == page.index
            {
                last.end_page = page.index;
                continue;
            }

            segments.push(QualitySegment {
                start_page: page.index,
                end_page: page.index,
                width: w,
                height: h,
                asset_type,
                is_fallback,
            });
        }

        Self {
            segments,
            has_fallbacks: fallback_count > 0,
            fallback_count,
        }
    }

    /// Generates a quality report from actual acquired and validated completed pages.
    pub fn from_completed_pages(pages: &[CompletedPageAsset]) -> Self {
        let mut segments: Vec<QualitySegment> = Vec::new();
        let mut fallback_count = 0;

        for page in pages {
            let is_fallback = page.asset_type == AssetType::Thumbnail;
            if is_fallback {
                fallback_count += 1;
            }

            if let Some(last) = segments.last_mut()
                && last.width == page.width
                && last.height == page.height
                && last.asset_type == page.asset_type
                && last.is_fallback == is_fallback
                && last.end_page + 1 == page.page_index
            {
                last.end_page = page.page_index;
                continue;
            }

            segments.push(QualitySegment {
                start_page: page.page_index,
                end_page: page.page_index,
                width: page.width,
                height: page.height,
                asset_type: page.asset_type,
                is_fallback,
            });
        }

        Self {
            segments,
            has_fallbacks: fallback_count > 0,
            fallback_count,
        }
    }

    /// Returns a multi-line formatted summary table of page quality ranges.
    pub fn format_report(&self) -> String {
        let mut out = String::new();
        out.push_str("Page Quality Breakdown:\n");
        for seg in &self.segments {
            let range = seg.page_range_string();
            let fallback_suffix = if seg.is_fallback { " (fallback)" } else { "" };
            if seg.width > 0 && seg.height > 0 {
                out.push_str(&format!(
                    "  {: <8} {:>4}×{:<4} {:?}{}\n",
                    range, seg.width, seg.height, seg.asset_type, fallback_suffix
                ));
            } else {
                out.push_str(&format!(
                    "  {: <8} [auto/unspecified geometry] {:?}{}\n",
                    range, seg.asset_type, fallback_suffix
                ));
            }
        }
        if self.has_fallbacks {
            out.push_str(&format!(
                "  Notice: {} page(s) utilized fallback asset resolution.\n",
                self.fallback_count
            ));
        }
        out
    }
}

impl fmt::Display for QualityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_report())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_segments_grouping() {
        let pages = vec![
            CompletedPageAsset {
                page_index: 1,
                relative_path: "p1.jpg".to_string(),
                sha256: "h1".to_string(),
                byte_size: 100,
                width: 1000,
                height: 1500,
                asset_type: AssetType::ImageJpeg,
            },
            CompletedPageAsset {
                page_index: 2,
                relative_path: "p2.jpg".to_string(),
                sha256: "h2".to_string(),
                byte_size: 100,
                width: 1000,
                height: 1500,
                asset_type: AssetType::ImageJpeg,
            },
            CompletedPageAsset {
                page_index: 3,
                relative_path: "p3.jpg".to_string(),
                sha256: "h3".to_string(),
                byte_size: 50,
                width: 500,
                height: 750,
                asset_type: AssetType::Thumbnail,
            },
            CompletedPageAsset {
                page_index: 4,
                relative_path: "p4.jpg".to_string(),
                sha256: "h4".to_string(),
                byte_size: 100,
                width: 1000,
                height: 1500,
                asset_type: AssetType::ImageJpeg,
            },
        ];

        let report = QualityReport::from_completed_pages(&pages);
        assert_eq!(report.segments.len(), 3);
        assert_eq!(report.segments[0].page_range_string(), "1–2");
        assert_eq!(report.segments[1].page_range_string(), "3");
        assert!(report.segments[1].is_fallback);
        assert_eq!(report.segments[2].page_range_string(), "4");
        assert!(report.has_fallbacks);
        assert_eq!(report.fallback_count, 1);
    }
}
