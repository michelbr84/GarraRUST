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
                .and_then(|v| v.as_str().ok())
                .map(|b| String::from_utf8_lossy(b).into_owned()),
            author: info_dict
                .and_then(|d| d.get(b"Author").ok())
                .and_then(|v| v.as_str().ok())
                .map(|b| String::from_utf8_lossy(b).into_owned()),
            subject: info_dict
                .and_then(|d| d.get(b"Subject").ok())
                .and_then(|v| v.as_str().ok())
                .map(|b| String::from_utf8_lossy(b).into_owned()),
            creator: info_dict
                .and_then(|d| d.get(b"Creator").ok())
                .and_then(|v| v.as_str().ok())
                .map(|b| String::from_utf8_lossy(b).into_owned()),
            producer: info_dict
                .and_then(|d| d.get(b"Producer").ok())
                .and_then(|v| v.as_str().ok())
                .map(|b| String::from_utf8_lossy(b).into_owned()),
            creation_date: info_dict
                .and_then(|d| d.get(b"CreationDate").ok())
                .and_then(|v| v.as_str().ok())
                .map(|b| String::from_utf8_lossy(b).into_owned()),
            modification_date: info_dict
                .and_then(|d| d.get(b"ModDate").ok())
                .and_then(|v| v.as_str().ok())
                .map(|b| String::from_utf8_lossy(b).into_owned()),
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
                && !page_text.trim().is_empty()
            {
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

    /// Texto desenhado pelo gerador de fixture, e o marcador que as
    /// assercoes de extracao procuram.
    const MARKER: &str = "Garra smoke";

    /// Grava bytes de PDF num arquivo temporario e devolve o caminho.
    ///
    /// Substitui o antigo `create_test_pdf`, que escrevia bytes de PDF na mao
    /// com offsets de `xref` errados. Aqueles bytes nao carregavam em nenhuma
    /// versao do lopdf, e era isso — e nao "version drift" — que mantinha cinco
    /// testes deste modulo `#[ignore]`d desde 2026-04-15. Gerar a fixture pelo
    /// proprio writer do lopdf mantem ela correta a cada bump da dependencia.
    fn write_pdf(tmp_dir: &TempDir, bytes: &[u8]) -> std::path::PathBuf {
        let pdf_path = tmp_dir.path().join("test.pdf");
        let mut file = File::create(&pdf_path).expect("temp dir must be writable");
        file.write_all(bytes).expect("fixture must be written");
        pdf_path
    }

    /// Build a minimal, structurally valid single-page PDF using lopdf's own
    /// writer.
    ///
    /// Toda fixture de PDF deste modulo sai daqui, e nao de bytes escritos na
    /// mao: o writer mantem `xref` e offsets corretos para qualquer versao do
    /// lopdf que estiver pinada, entao a fixture continua carregavel a cada
    /// bump da dependencia.
    ///
    /// `info` vira o dicionario `/Info` do trailer, que e de onde
    /// `extract_metadata` le titulo e autor. `None` produz um PDF sem `/Info`,
    /// que e o caso em que todos os campos de metadado devem sair `None`.
    fn build_minimal_pdf_with_info(info: Option<(&str, &str)>) -> Vec<u8> {
        use lopdf::content::{Content, Operation};
        use lopdf::{Object, Stream, dictionary};

        let mut doc = lopdf::Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![100.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal(MARKER)]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(
            dictionary! {},
            content.encode().expect("content stream must encode"),
        ));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        if let Some((title, author)) = info {
            let info_id = doc.add_object(dictionary! {
                "Title" => Object::string_literal(title),
                "Author" => Object::string_literal(author),
            });
            doc.trailer.set("Info", info_id);
        }

        let mut buf = Vec::new();
        doc.save_to(&mut buf)
            .expect("lopdf must serialize the document");
        buf
    }

    /// A fixture do caso comum: um PDF de uma pagina, sem `/Info`.
    fn build_minimal_pdf() -> Vec<u8> {
        build_minimal_pdf_with_info(None)
    }

    /// Runtime guard for lopdf dependency bumps.
    ///
    /// Round-trips um documento por `Document::save_to` -> `load_mem` ->
    /// `extract_text`, que e a superficie que um bump de parser pode quebrar em
    /// silencio. Adicionado no bump 0.42 -> 0.44 (plan 0356); os demais testes
    /// de PDF deste modulo, antes `#[ignore]`d, hoje exercitam o mesmo caminho
    /// a partir de arquivo.
    #[test]
    fn test_lopdf_roundtrip_smoke() {
        let bytes = build_minimal_pdf();
        assert!(bytes.starts_with(b"%PDF-"), "writer produced non-PDF bytes");

        let parsed = PdfProcessor::new()
            .extract_text_from_bytes(&bytes)
            .expect("lopdf must be able to parse a document it just wrote");

        assert_eq!(parsed.page_count, 1);
        assert!(
            parsed.text.contains("Garra smoke"),
            "extracted text was {:?}",
            parsed.text
        );
    }

    #[test]
    fn test_pdf_processor_new() {
        let processor = PdfProcessor::new();
        let _ = processor;
    }

    #[test]
    fn test_extract_text_from_test_pdf() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = write_pdf(&tmp_dir, &build_minimal_pdf());

        let processor = PdfProcessor::new();
        let result = processor.extract_text(&pdf_path);

        assert!(result.is_ok());
        let doc = result.unwrap();
        assert!(
            doc.text.contains(MARKER),
            "extracted text was {:?}",
            doc.text
        );
        assert_eq!(doc.page_count, 1);
    }

    #[test]
    fn test_extract_metadata() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = write_pdf(&tmp_dir, &build_minimal_pdf());

        let processor = PdfProcessor::new();
        let doc = processor
            .extract_text(&pdf_path)
            .expect("PDF gerado pelo writer deve carregar");

        // Sem `/Info` no trailer, todo campo de metadado sai `None`. A
        // assercao antiga aqui era `title.is_none() || title.is_some()` — uma
        // tautologia, verdadeira ate para um parser completamente quebrado.
        assert_eq!(doc.metadata.title, None);
        assert_eq!(doc.metadata.author, None);
    }

    /// Com `/Info` no trailer, `extract_metadata` tem que ler de la.
    ///
    /// E o par do teste acima: um exercita o caminho ausente, o outro o
    /// presente. Nenhum dos dois existia de verdade enquanto a fixture nao
    /// carregava.
    #[test]
    fn test_extract_metadata_reads_info_dictionary() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = write_pdf(
            &tmp_dir,
            &build_minimal_pdf_with_info(Some(("Relatorio Garra", "Equipe GarraIA"))),
        );

        let processor = PdfProcessor::new();
        let doc = processor
            .extract_text(&pdf_path)
            .expect("PDF com /Info deve carregar");

        assert_eq!(doc.metadata.title.as_deref(), Some("Relatorio Garra"));
        assert_eq!(doc.metadata.author.as_deref(), Some("Equipe GarraIA"));
        assert_eq!(doc.metadata.subject, None);
    }

    #[test]
    fn test_get_page_count() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = write_pdf(&tmp_dir, &build_minimal_pdf());

        let processor = PdfProcessor::new();
        let count = processor.get_page_count(&pdf_path);

        assert!(count.is_ok());
        assert_eq!(count.unwrap(), 1);
    }

    #[test]
    fn test_extract_page_range() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = write_pdf(&tmp_dir, &build_minimal_pdf());

        let processor = PdfProcessor::new();
        let result = processor.extract_page_range(&pdf_path, 1, 1);

        assert!(result.is_ok());
        let doc = result.unwrap();
        assert!(
            doc.text.contains(MARKER),
            "extracted text was {:?}",
            doc.text
        );
        assert_eq!(doc.page_count, 1);
    }

    /// Faixa invalida tem que ser recusada pela validacao de faixa.
    ///
    /// Este teste ja passava antes, mas **pelo motivo errado**: a fixture
    /// escrita na mao nao carregava, entao o `Err` vinha do
    /// `Document::load` e a checagem de `start_page`/`end_page` nunca era
    /// alcancada. Com uma fixture que carrega, o erro passa a ser o da
    /// validacao de verdade.
    #[test]
    fn test_extract_page_range_invalid() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = write_pdf(&tmp_dir, &build_minimal_pdf());

        let processor = PdfProcessor::new();
        let result = processor.extract_page_range(&pdf_path, 2, 5);

        assert!(result.is_err());
    }

    #[test]
    fn test_extract_text_from_bytes() {
        let tmp_dir = TempDir::new().unwrap();
        let pdf_path = write_pdf(&tmp_dir, &build_minimal_pdf());

        let bytes = std::fs::read(&pdf_path).unwrap();

        let processor = PdfProcessor::new();
        let result = processor.extract_text_from_bytes(&bytes);

        assert!(result.is_ok());
        let doc = result.unwrap();
        assert!(
            doc.text.contains(MARKER),
            "extracted text was {:?}",
            doc.text
        );
    }
}
