use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, Stream};
use std::path::{Path, PathBuf};

use crate::core::document::Publication;
use crate::core::error::DocDownloaderError;
use crate::core::job::CompletedPageAsset;
use crate::pdf::image::inspect_and_validate_asset;
use crate::pdf::validator::validate_pdf_document;
use crate::storage::atomic::AtomicFileWriter;

/// Assembles validated page image assets into a standards-compliant offline PDF document.
pub struct PdfBuilder {
    title: String,
    author: Option<String>,
    subject: Option<String>,
}

impl PdfBuilder {
    pub fn new(publication: &Publication) -> Self {
        Self {
            title: publication.title.clone(),
            author: publication.author.clone(),
            subject: publication.description.clone(),
        }
    }

    /// Reconstructs the PDF from the list of completed page assets in strict page order.
    /// Writes output atomically to `target_pdf_path` and validates the final document.
    pub fn build(
        &self,
        target_pdf_path: &Path,
        cache_base: &Path,
        pages: &[CompletedPageAsset],
    ) -> Result<PathBuf, DocDownloaderError> {
        if pages.is_empty() {
            return Err(DocDownloaderError::PdfGenerationFailed {
                reason: "Cannot build PDF: 0 pages provided".to_string(),
            });
        }

        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let mut page_ids = Vec::with_capacity(pages.len());

        for page in pages {
            let asset_path = cache_base.join(&page.relative_path);
            let metadata = inspect_and_validate_asset(&asset_path, page.page_index)?;

            let raw_bytes =
                std::fs::read(&asset_path).map_err(|e| DocDownloaderError::FileSystemError {
                    path: asset_path.clone(),
                    reason: format!("Failed to read cached page asset: {e}"),
                })?;

            let (w, h) = (metadata.width, metadata.height);

            // 1. Embed image as XObject
            let image_id = if metadata.is_direct_jpeg {
                // Direct JPEG embedding: Lossless, avoids re-encoding and heavy memory allocation
                let mut image_dict = Dictionary::new();
                image_dict.set("Type", Object::Name(b"XObject".to_vec()));
                image_dict.set("Subtype", Object::Name(b"Image".to_vec()));
                image_dict.set("Width", Object::Integer(w as i64));
                image_dict.set("Height", Object::Integer(h as i64));
                image_dict.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
                image_dict.set("BitsPerComponent", Object::Integer(8));
                image_dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));

                let stream = Stream::new(image_dict, raw_bytes);
                doc.add_object(stream)
            } else {
                // Non-JPEG format (PNG, WebP): decode and embed as standard RGB Flate stream
                let dyn_img = image::load_from_memory(&raw_bytes).map_err(|e| {
                    DocDownloaderError::PdfGenerationFailed {
                        reason: format!("Failed to decode image page {}: {e}", page.page_index),
                    }
                })?;
                let rgb = dyn_img.to_rgb8();
                let (img_w, img_h) = (rgb.width(), rgb.height());

                let mut image_dict = Dictionary::new();
                image_dict.set("Type", Object::Name(b"XObject".to_vec()));
                image_dict.set("Subtype", Object::Name(b"Image".to_vec()));
                image_dict.set("Width", Object::Integer(img_w as i64));
                image_dict.set("Height", Object::Integer(img_h as i64));
                image_dict.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
                image_dict.set("BitsPerComponent", Object::Integer(8));

                let stream = Stream::new(image_dict, rgb.into_raw());
                // lopdf's compress method will apply FlateDecode compression automatically
                let mut compressed_stream = stream;
                let _ = compressed_stream.compress();
                doc.add_object(compressed_stream)
            };

            // 2. Build Page Content stream drawing the XObject across the page dimensions
            let content = Content {
                operations: vec![
                    Operation::new("q", vec![]),
                    Operation::new(
                        "cm",
                        vec![
                            Object::Real(w as f32),
                            Object::Integer(0),
                            Object::Integer(0),
                            Object::Real(h as f32),
                            Object::Integer(0),
                            Object::Integer(0),
                        ],
                    ),
                    Operation::new("Do", vec![Object::Name(b"Im1".to_vec())]),
                    Operation::new("Q", vec![]),
                ],
            };

            let encoded_content =
                content
                    .encode()
                    .map_err(|e| DocDownloaderError::PdfGenerationFailed {
                        reason: format!(
                            "Failed to encode content stream for page {}: {e}",
                            page.page_index
                        ),
                    })?;

            let content_id = doc.add_object(Stream::new(Dictionary::new(), encoded_content));

            // 3. Assemble Page Dictionary
            let mut resources = Dictionary::new();
            let mut xobjects = Dictionary::new();
            xobjects.set("Im1", Object::Reference(image_id));
            resources.set("XObject", Object::Dictionary(xobjects));

            let mut page_dict = Dictionary::new();
            page_dict.set("Type", Object::Name(b"Page".to_vec()));
            page_dict.set("Parent", Object::Reference(pages_id));
            page_dict.set(
                "MediaBox",
                Object::Array(vec![
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Real(w as f32),
                    Object::Real(h as f32),
                ]),
            );
            page_dict.set("Contents", Object::Reference(content_id));
            page_dict.set("Resources", Object::Dictionary(resources));

            let page_id = doc.add_object(page_dict);
            page_ids.push(page_id);
        }

        // 4. Build Pages Root Dictionary
        let pages_dict = Dictionary::from_iter(vec![
            ("Type", Object::Name(b"Pages".to_vec())),
            ("Count", Object::Integer(page_ids.len() as i64)),
            (
                "Kids",
                Object::Array(page_ids.into_iter().map(Object::Reference).collect()),
            ),
        ]);
        doc.objects.insert(pages_id, Object::Dictionary(pages_dict));

        // 5. Build Catalog Dictionary
        let catalog_id = doc.add_object(Dictionary::from_iter(vec![
            ("Type", Object::Name(b"Catalog".to_vec())),
            ("Pages", Object::Reference(pages_id)),
        ]));
        doc.trailer.set("Root", Object::Reference(catalog_id));

        // 6. Populate Metadata Info Dictionary
        let mut info = Dictionary::new();
        info.set("Title", Object::string_literal(self.title.as_bytes()));
        if let Some(ref author) = self.author {
            info.set("Author", Object::string_literal(author.as_bytes()));
        }
        if let Some(ref subject) = self.subject {
            info.set("Subject", Object::string_literal(subject.as_bytes()));
        }
        info.set("Creator", Object::string_literal(b"DocDownloader"));
        info.set(
            "Producer",
            Object::string_literal(
                format!("DocDownloader v{}", env!("CARGO_PKG_VERSION")).as_bytes(),
            ),
        );
        let info_id = doc.add_object(info);
        doc.trailer.set("Info", Object::Reference(info_id));

        // 7. Atomic Write to Disk
        let mut atomic_writer = AtomicFileWriter::new(target_pdf_path)?;
        doc.save_to(&mut atomic_writer)
            .map_err(|e| DocDownloaderError::PdfGenerationFailed {
                reason: format!("Failed to serialize PDF: {e}"),
            })?;

        // 8. Validate Generated Document Structure before committing
        validate_pdf_document(atomic_writer.temp_path(), pages.len() as u32)?;

        // 9. Atomic Commit
        let final_path = atomic_writer.commit()?;
        Ok(final_path)
    }
}

impl std::io::Write for AtomicFileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.write_all(buf)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
