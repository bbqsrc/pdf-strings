use std::io::Read;
use std::path::Path;

use lopdf::{Dictionary, Document};

use crate::error::OutputError;
use crate::output::BoundingBoxOutput;
use crate::processor::Processor;
use crate::types::{MediaBox, TextOutput, TextSpan};
use crate::utils::{get, get_inherited};

fn extract_structured_from_doc(doc: &Document) -> Result<Vec<Vec<TextSpan>>, OutputError> {
    let mut output = BoundingBoxOutput::new();
    let empty_resources = Dictionary::new();
    let pages = doc.get_pages();
    let mut p = Processor::new();

    for dict in pages {
        let page_num = dict.0;
        let object_id = dict.1;
        let page_dict = doc.get_object(object_id).unwrap().as_dict().unwrap();
        let resources = get_inherited(doc, page_dict, b"Resources").unwrap_or(&empty_resources);
        let media_box: Vec<f32> = get_inherited(doc, page_dict, b"MediaBox").expect("MediaBox");
        let media_box = MediaBox {
            llx: media_box[0],
            lly: media_box[1],
            urx: media_box[2],
            ury: media_box[3],
        };
        let art_box =
            get::<Option<Vec<f32>>>(&doc, page_dict, b"ArtBox").map(|x| (x[0], x[1], x[2], x[3]));

        output.begin_page(page_num, &media_box, art_box)?;
        p.process_stream(
            &doc,
            doc.get_page_content(object_id).unwrap(),
            resources,
            &media_box,
            &mut output,
            page_num,
        )?;
        output.end_page()?;
    }

    Ok(output.into_lines())
}

/// Builder for configuring PDF extraction options.
///
/// # Examples
///
/// ```no_run
/// use pdf_extract::PdfExtractor;
///
/// // With password
/// let output = PdfExtractor::builder()
///     .password("secret")
///     .build()
///     .from_path("encrypted.pdf")?;
/// # Ok::<(), pdf_extract::OutputError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct PdfExtractorBuilder {
    password: Option<String>,
}

impl PdfExtractorBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the password for encrypted PDFs.
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Build the extractor configuration.
    pub fn build(self) -> PdfExtractor {
        PdfExtractor {
            password: self.password,
        }
    }
}

/// PDF text extractor with configuration options.
///
/// # Examples
///
/// ```no_run
/// use pdf_extract::PdfExtractor;
///
/// // Simple extraction
/// let output = PdfExtractor::default().from_path("file.pdf")?;
/// println!("{}", output);
///
/// // Pretty formatted output
/// println!("{}", output.to_string_pretty());
///
/// // Access structured data
/// for line in output.lines() {
///     for span in line {
///         println!("Text: {} at position {:?}", span.text, span.bbox);
///     }
/// }
/// # Ok::<(), pdf_extract::OutputError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct PdfExtractor {
    password: Option<String>,
}

impl PdfExtractor {
    /// Create a builder for configuring extraction options.
    pub fn builder() -> PdfExtractorBuilder {
        PdfExtractorBuilder::new()
    }

    /// Extract text from a PDF file at the given path.
    pub fn from_path<P: AsRef<Path>>(self, path: P) -> Result<TextOutput, OutputError> {
        let mut doc = Document::load(path)?;
        self.extract_from_document(&mut doc)
    }

    /// Extract text from a PDF in memory.
    pub fn from_bytes(self, bytes: &[u8]) -> Result<TextOutput, OutputError> {
        let mut doc = Document::load_mem(bytes)?;
        self.extract_from_document(&mut doc)
    }

    /// Extract text from a PDF reader.
    pub fn from_reader<R: Read>(self, mut reader: R) -> Result<TextOutput, OutputError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        self.from_bytes(&bytes)
    }

    fn extract_from_document(self, doc: &mut Document) -> Result<TextOutput, OutputError> {
        if doc.is_encrypted() {
            if let Some(password) = &self.password {
                doc.decrypt(password)?;
            } else {
                doc.decrypt("")?;
            }
        }

        let lines = extract_structured_from_doc(doc)?;
        Ok(TextOutput::from(lines))
    }
}

/// Extract text from a document at the given path. Dispatches by file
/// extension: `.ps` is parsed as dvips-emitted PostScript; everything
/// else is parsed as PDF.
pub fn from_path<P: AsRef<Path>>(path: P) -> Result<TextOutput, OutputError> {
    let path_ref = path.as_ref();
    let is_ps = path_ref
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("ps"))
        .unwrap_or(false);
    if is_ps {
        from_ps_path(path_ref)
    } else {
        PdfExtractor::default().from_path(path_ref)
    }
}

/// Extract text from a dvips-emitted PostScript file at the given path.
///
/// Unlike the PDF path, this does not perform OCR — the PostScript
/// content carries authoritative glyph codes under the TeX OT1 encoding
/// which are mapped directly to Unicode. Non-dvips PostScript is
/// rejected with an error.
pub fn from_ps_path<P: AsRef<Path>>(path: P) -> Result<TextOutput, OutputError> {
    let bytes = std::fs::read(path)?;
    from_ps_bytes(&bytes)
}

/// Extract text from dvips-emitted PostScript in memory.
pub fn from_ps_bytes(bytes: &[u8]) -> Result<TextOutput, OutputError> {
    let lines = crate::ps::extract_lines(bytes)?;
    Ok(TextOutput::from(lines))
}

/// Extract text from dvips-emitted PostScript via a reader.
pub fn from_ps_reader<R: Read>(mut reader: R) -> Result<TextOutput, OutputError> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    from_ps_bytes(&bytes)
}

/// Extract text from a PDF in memory using default settings.
///
/// This is a convenience function equivalent to `PdfExtractor::default().from_bytes(bytes)`.
///
/// # Examples
///
/// ```no_run
/// let bytes = std::fs::read("file.pdf")?;
/// let output = pdf_extract::from_bytes(&bytes)?;
/// println!("{}", output);
/// # Ok::<(), pdf_extract::OutputError>(())
/// ```
pub fn from_bytes(bytes: &[u8]) -> Result<TextOutput, OutputError> {
    PdfExtractor::default().from_bytes(bytes)
}

/// Extract text from a PDF reader using default settings.
///
/// This is a convenience function equivalent to `PdfExtractor::default().from_reader(reader)`.
///
/// # Examples
///
/// ```no_run
/// use std::fs::File;
///
/// let file = File::open("file.pdf")?;
/// let output = pdf_extract::from_reader(file)?;
/// println!("{}", output);
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn from_reader<R: Read>(reader: R) -> Result<TextOutput, OutputError> {
    PdfExtractor::default().from_reader(reader)
}
