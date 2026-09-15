use lopdf::Document;
use std::path::Path;

use crate::core::error::DocDownloaderError;

/// Inspects and programmatically validates a generated PDF document.
pub fn validate_pdf_document(
    pdf_path: &Path,
    expected_pages: u32,
) -> Result<(), DocDownloaderError> {
    let metadata =
        std::fs::metadata(pdf_path).map_err(|e| DocDownloaderError::FileSystemError {
            path: pdf_path.to_path_buf(),
            reason: format!("Failed to read generated PDF metadata: {e}"),
        })?;

    if metadata.len() < 100 {
        return Err(DocDownloaderError::PdfValidationFailed {
            reason: format!(
                "PDF file size ({} bytes) is unreasonably small to contain a valid document",
                metadata.len()
            ),
        });
    }

    // Parse the PDF structure independently
    let doc = Document::load(pdf_path).map_err(|e| DocDownloaderError::PdfValidationFailed {
        reason: format!("Failed to parse PDF structure: {e}"),
    })?;

    // Validate page count
    let page_count = doc.get_pages().len() as u32;
    if page_count != expected_pages {
        return Err(DocDownloaderError::PdfValidationFailed {
            reason: format!(
                "PDF page count mismatch: expected {expected_pages} pages, found {page_count}"
            ),
        });
    }

    // Inspect individual pages
    let pages = doc.get_pages();
    for (page_num, page_id) in pages {
        let page_dict =
            doc.get_dictionary(page_id)
                .map_err(|e| DocDownloaderError::PdfValidationFailed {
                    reason: format!("Page {page_num} missing valid dictionary: {e}"),
                })?;

        // Validate MediaBox dimensions
        let media_box = page_dict
            .get(b"MediaBox")
            .and_then(|obj| obj.as_array())
            .map_err(|e| DocDownloaderError::PdfValidationFailed {
                reason: format!("Page {page_num} missing or malformed MediaBox: {e}"),
            })?;

        if media_box.len() != 4 {
            return Err(DocDownloaderError::PdfValidationFailed {
                reason: format!(
                    "Page {page_num} MediaBox has {} coordinates instead of 4",
                    media_box.len()
                ),
            });
        }

        let width = extract_coordinate(&media_box[2]) - extract_coordinate(&media_box[0]);
        let height = extract_coordinate(&media_box[3]) - extract_coordinate(&media_box[1]);

        if width <= 0.0 || height <= 0.0 {
            return Err(DocDownloaderError::PdfValidationFailed {
                reason: format!("Page {page_num} has non-positive geometry ({width}x{height})"),
            });
        }

        // Verify page has contents stream
        if !page_dict.has(b"Contents") {
            return Err(DocDownloaderError::PdfValidationFailed {
                reason: format!("Page {page_num} lacks a Contents stream"),
            });
        }
    }

    Ok(())
}

fn extract_coordinate(obj: &lopdf::Object) -> f64 {
    match obj {
        lopdf::Object::Integer(i) => *i as f64,
        lopdf::Object::Real(f) => *f as f64,
        _ => 0.0,
    }
}
