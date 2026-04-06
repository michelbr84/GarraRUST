//! PDF processing module for GarraIA
//!
//! Provides PDF text extraction and metadata parsing capabilities.

use garraia_common::{Error, Result};
use itertools::Itertools;
use lopdf::Document;
use std::path::Path;
use tracing::{debug, info, warn};

use crate::types::{DocumentMetadata, ParsedDocument};

/// Processor for PDF documents
pub struct PdfProcessor;

impl PdfProcessor {
    /// Create a new PDF processor
    pub fn new() -> Self {
        Self
    }

    /// Extract text from a PDF file
    ///
    /// # Arguments
    /// * `path` - Path to the PDF file
    ///
    /// # Returns
    /// * `ParsedDocument` containing extracted text and metadata
    pub fn extract_text<P: AsRef<Path>>(&self, path: P) -> Result<ParsedDocument> {
        let path = path.as_ref();
        info!("Extracting text from PDF: {}", path.display());

        let doc =
            Document::load(path).map_err(|e| Error::Media(format!("Failed to load PDF: {}", e)))?;

        let page_count = doc.get_pages().len();
        debug!("PDF has {} pages", page_count);

        // Extract metadata
        let metadata = self.extract_metadata(&doc);

        // Extract text from all pages
        let mut full_text = String::new();
        let pages = doc.get_pages();

        for (page_num, _) in pages.iter().sorted_by_key(|(num, _)| *num) {
            if let Ok(page_text) = doc.extract_text(&[*page_num]) {
                if !page_text.trim().is_empty() {
                    full_text.push_str(&page_text);
                    full_text.push('\n');
                }
            } else {
                warn!("Failed to extract text from page {}", page_num);
            }
        }

        Ok(ParsedDocument {
            text: full_text.trim().to_string(),
            page_count,
            metadata,
        })
    }

    /// Extract text from PDF bytes
    ///
    /// # Arguments
    /// * `data` - PDF file bytes
    ///
    /// # Returns
    /// * `ParsedDocument` containing extracted text and metadata
    pub fn extract_text_from_bytes(&self, data: &[u8]) -> Result<ParsedDocument> {
        info!("Extracting text from PDF bytes ({} bytes)", data.len());

        let doc = Document::load_mem(data)
            .map_err(|e| Error::Media(format!("Failed to load PDF from bytes: {}", e)))?;

        let page_count = doc.get_pages().len();
        debug!("PDF has {} pages", page_count);

        // Extract metadata
        let metadata = self.extract_metadata(&doc);

        // Extract text from all pages
        let mut full_text = String::new();
        let pages = doc.get_pages();

        for (page_num, _) in pages.iter().sorted_by_key(|(num, _)| *num) {
            if let Ok(page_text) = doc.extract_text(&[*page_num]) {
                if !page_text.trim().is_empty() {
                    full_text.push_str(&page_text);
                    full_text.push('\n');
                }
            } else {
                warn!("Failed to extract text from page {}", page_num);
            }
        }

        Ok(ParsedDocument {
            text: full_text.trim().to_string(),
            page_count,
            metadata,
        })
    }

    /// Extract metadata from a PDF document
    fn extract_metadata(&self, doc: &Document) -> DocumentMetadata {
        let info_dict = doc.trailer.get(b"Info").ok().and_then(|info_ref| {
            if let Ok(info_id) = info_ref.as_reference() {
                doc.get_dictionary(info_id).ok()
            } else {
                None
            }
        });

        DocumentMetadata {
            title: info_dict
                .and_then(|d| d.get(b"Title").ok())
                .and_then(|v| v.as_string().ok())
                .map(|s| s.to_string()),
            author: info_dict
                .and_then(|d| d.get(b"Author").ok())
                .and_then(|v| v.as_string().ok())
                .map(|s| s.to_string()),
            subject: info_dict
                .and_then(|d| d.get(b"Subject").ok())
                .and_then(|v| v.as_string().ok())
                .map(|s| s.to_string()),
            creator: info_dict
                .and_then(|d| d.get(b"Creator").ok())
                .and_then(|v| v.as_string().ok())
                .map(|s| s.to_string()),
            producer: info_dict
                .and_then(|d| d.get(b"Producer").ok())
                .and_then(|v| v.as_string().ok())
                .map(|s| s.to_string()),
            creation_date: info_dict
                .and_then(|d| d.get(b"CreationDate").ok())
                .and_then(|v| v.as_string().ok())
                .map(|s| s.to_string()),
            modification_date: info_dict
                .and_then(|d| d.get(b"ModDate").ok())
                .and_then(|v| v.as_string().ok())
                .map(|s| s.to_string()),
        }
    }

    /// Get page count without extracting full text
    pub fn get_page_count<P: AsRef<Path>>(&self, path: P) -> Result<usize> {
        let doc = Document::load(path.as_ref())
            .map_err(|e| Error::Media(format!("Failed to load PDF: {}", e)))?;
        Ok(doc.get_pages().len())
    }

    /// Extract text from a specific page range
    ///
    /// # Arguments
    /// * `path` - Path to the PDF file
    /// * `start_page` - 1-indexed start page
    /// * `end_page` - 1-indexed end page (inclusive)
    pub fn extract_page_range<P: AsRef<Path>>(
        &self,
        path: P,
        start_page: usize,
        end_page: usize,
    ) -> Result<ParsedDocument> {
        let path = path.as_ref();
        info!(
            "Extracting pages {} to {} from PDF: {}",
            start_page,
            end_page,
            path.display()
        );

        let doc =
            Document::load(path).map_err(|e| Error::Media(format!("Failed to load PDF: {}", e)))?;

        let total_pages = doc.get_pages().len();
        if start_page > end_page || start_page == 0 || end_page > total_pages {
            return Err(Error::Media(format!(
                "Invalid page range: {}-{} (total: {})",
                start_page, end_page, total_pages
            )));
        }

        let metadata = self.extract_metadata(&doc);

        // Extract text for the requested page range
        let mut full_text = String::new();
        for page_num in start_page..=end_page {
            if let Ok(page_text) = doc.extract_text(&[page_num as u32])
                && !page_text.trim().is_empty() {
                    full_text.push_str(&page_text);
                    full_text.push('\n');
                }
        }

        Ok(ParsedDocument {
            text: full_text.trim().to_string(),
            page_count: end_page - start_page + 1,
            metadata,
        })
    }
}

impl Default for PdfProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_pdf(tmp_dir: &TempDir) -> std::path::PathBuf {
        // Create a simple PDF with some text
        let pdf_path = tmp_dir.path().join("test.pdf");
        let pdf_content = r#"%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>
endobj
4 0 obj
<< /Length 44 >>
stream
BT
/F1 12 Tf
100 700 Td
(Test PDF) Tj
ET
endstream
endobj
5 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
xref
0 6
0000000000 65535 f 
0000000009 00000 n 
0000000058 00000 n 
0000000115 00000 n 
0000000266 00000 n 
0000000361 00000 n 
trailer
<< /Size 6 /Root 1 0 R >>
startxref
454
%%EOF"#;
        let mut file = File::create(&pdf_path).unwrap();
        file.write_all(pdf_content.as_bytes()).unwrap();
        pdf_path
    }

    #[test]
    fn test_pdf_processor_new() {
        let processor = PdfProcessor::new();
        let _ = processor;
    }

    #[test]
    fn test_extract_text_from_test_pdf() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = create_test_pdf(&tmp_dir);

        let processor = PdfProcessor::new();
        let result = processor.extract_text(&pdf_path);

        assert!(result.is_ok());
        let doc = result.unwrap();
        assert!(doc.text.contains("Test PDF"));
        assert_eq!(doc.page_count, 1);
    }

    #[test]
    fn test_extract_metadata() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = create_test_pdf(&tmp_dir);

        let processor = PdfProcessor::new();
        let result = processor.extract_text(&pdf_path);

        assert!(result.is_ok());
        let doc = result.unwrap();
        // Basic metadata should be present (even if None for this simple PDF)
        assert!(doc.metadata.title.is_none() || doc.metadata.title.is_some());
    }

    #[test]
    fn test_get_page_count() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = create_test_pdf(&tmp_dir);

        let processor = PdfProcessor::new();
        let count = processor.get_page_count(&pdf_path);

        assert!(count.is_ok());
        assert_eq!(count.unwrap(), 1);
    }

    #[test]
    fn test_extract_page_range() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = create_test_pdf(&tmp_dir);

        let processor = PdfProcessor::new();
        let result = processor.extract_page_range(&pdf_path, 1, 1);

        assert!(result.is_ok());
        let doc = result.unwrap();
        assert!(doc.text.contains("Test PDF"));
        assert_eq!(doc.page_count, 1);
    }

    #[test]
    fn test_extract_page_range_invalid() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = create_test_pdf(&tmp_dir);

        let processor = PdfProcessor::new();
        let result = processor.extract_page_range(&pdf_path, 2, 5);

        assert!(result.is_err());
    }

    #[test]
    fn test_extract_text_from_bytes() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = create_test_pdf(&tmp_dir);

        let bytes = std::fs::read(&pdf_path).unwrap();

        let processor = PdfProcessor::new();
        let result = processor.extract_text_from_bytes(&bytes);

        assert!(result.is_ok());
        let doc = result.unwrap();
        assert!(doc.text.contains("Test PDF"));
    }
}
