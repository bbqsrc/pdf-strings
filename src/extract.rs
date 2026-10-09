#[cfg(target_os = "macos")]
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

use lopdf::{Dictionary, Document};

use crate::error::OutputError;
use crate::output::BoundingBoxOutput;
use crate::processor::Processor;
use crate::types::{MediaBox, TextOutput, TextSpan};
use crate::utils::{get, get_inherited};

/// Controls whether OCR is used as a fallback for pages that produce no
/// text spans from content-stream extraction.
///
/// On macOS, OCR runs via the Vision framework (`VNRecognizeTextRequest`),
/// the same backend used by Preview's Live Text. On other platforms,
/// `Auto` is equivalent to `Off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OcrPolicy {
    /// Never run OCR.
    Off,
    /// Run OCR only on pages whose content-stream extraction yielded no
    /// spans (scanned/image-only pages). Default on macOS.
    #[cfg_attr(target_os = "macos", default)]
    Auto,
}

#[cfg(not(target_os = "macos"))]
impl Default for OcrPolicy {
    fn default() -> Self {
        OcrPolicy::Off
    }
}

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
#[derive(Debug, Clone)]
pub struct PdfExtractorBuilder {
    password: Option<String>,
    ocr: OcrPolicy,
    ocr_languages: Vec<String>,
    deskew: bool,
    ocr_dump_dir: Option<std::path::PathBuf>,
}

impl Default for PdfExtractorBuilder {
    fn default() -> Self {
        Self {
            password: None,
            ocr: OcrPolicy::default(),
            ocr_languages: vec!["en-US".to_string()],
            // Document-segmentation tracks the paper outline, not text
            // baselines — on fax scans with bent/torn corners that's a
            // misleading signal. 600-DPI rendering already handles most
            // legibility issues, so deskew is opt-in.
            deskew: false,
            ocr_dump_dir: None,
        }
    }
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

    /// Configure OCR fallback behaviour for image-only pages.
    ///
    /// Defaults to [`OcrPolicy::Auto`] on macOS (using the Vision framework)
    /// and [`OcrPolicy::Off`] on other platforms.
    pub fn ocr(mut self, policy: OcrPolicy) -> Self {
        self.ocr = policy;
        self
    }

    /// Set the recognition languages passed to the OCR backend.
    ///
    /// An empty list enables Vision's automatic language detection. The
    /// default is `["en-US"]`. On non-macOS platforms this is ignored.
    pub fn ocr_languages<I, S>(mut self, languages: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.ocr_languages = languages.into_iter().map(Into::into).collect();
        self
    }

    /// Enable or disable automatic deskewing of scanned pages before OCR.
    ///
    /// Uses Apple's `VNDetectHorizonRequest` to detect rotation and
    /// applies a corrective transform before running text recognition.
    /// Defaults to `true` on macOS, `false` elsewhere. Ignored when
    /// [`OcrPolicy::Off`] is in effect.
    pub fn deskew(mut self, enable: bool) -> Self {
        self.deskew = enable;
        self
    }

    /// If set, writes the final (post-deskew) grayscale bitmap for every
    /// page sent to Vision OCR into the given directory as
    /// `page-<N>.pgm`. Useful for inspecting exactly what the OCR model
    /// sees. macOS only.
    pub fn ocr_dump_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.ocr_dump_dir = Some(dir.into());
        self
    }

    /// Build the extractor configuration.
    pub fn build(self) -> PdfExtractor {
        PdfExtractor {
            password: self.password,
            ocr: self.ocr,
            ocr_languages: self.ocr_languages,
            deskew: self.deskew,
            ocr_dump_dir: self.ocr_dump_dir,
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
#[derive(Debug, Clone)]
pub struct PdfExtractor {
    password: Option<String>,
    ocr: OcrPolicy,
    ocr_languages: Vec<String>,
    deskew: bool,
    ocr_dump_dir: Option<std::path::PathBuf>,
}

impl Default for PdfExtractor {
    fn default() -> Self {
        PdfExtractorBuilder::default().build()
    }
}

impl PdfExtractor {
    /// Create a builder for configuring extraction options.
    pub fn builder() -> PdfExtractorBuilder {
        PdfExtractorBuilder::new()
    }

    /// Extract text from a PDF file at the given path.
    pub fn from_path<P: AsRef<Path>>(self, path: P) -> Result<TextOutput, OutputError> {
        let bytes = std::fs::read(path)?;
        self.from_bytes(&bytes)
    }

    /// Extract text from a PDF in memory.
    pub fn from_bytes(self, bytes: &[u8]) -> Result<TextOutput, OutputError> {
        let mut doc = Document::load_mem(bytes)?;
        self.extract_from_document(&mut doc, bytes)
    }

    /// Extract text from a PDF reader.
    pub fn from_reader<R: Read>(self, mut reader: R) -> Result<TextOutput, OutputError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        self.from_bytes(&bytes)
    }

    fn extract_from_document(
        self,
        doc: &mut Document,
        pdf_bytes: &[u8],
    ) -> Result<TextOutput, OutputError> {
        if doc.is_encrypted() {
            if let Some(password) = &self.password {
                doc.decrypt(password)?;
            } else {
                doc.decrypt("")?;
            }
        }

        let mut lines = extract_structured_from_doc(doc)?;

        if self.ocr == OcrPolicy::Auto {
            lines = maybe_apply_ocr(
                doc,
                pdf_bytes,
                &self.ocr_languages,
                self.deskew,
                self.ocr_dump_dir.as_deref(),
                lines,
            );
        }

        Ok(TextOutput::from(lines))
    }
}

/// Identify pages that produced zero content-stream spans and run OCR on
/// them (macOS only). Returns a re-grouped `Vec<Vec<TextSpan>>`.
fn maybe_apply_ocr(
    doc: &Document,
    pdf_bytes: &[u8],
    ocr_languages: &[String],
    deskew: bool,
    dump_dir: Option<&std::path::Path>,
    lines: Vec<Vec<TextSpan>>,
) -> Vec<Vec<TextSpan>> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (doc, pdf_bytes, ocr_languages, deskew, dump_dir);
        return lines;
    }
    #[cfg(target_os = "macos")]
    {
        let covered: HashSet<u32> = lines.iter().flatten().map(|s| s.page_num).collect();
        let empty_pages: Vec<u32> = doc
            .get_pages()
            .keys()
            .copied()
            .filter(|p| !covered.contains(p))
            .collect();
        if empty_pages.is_empty() {
            return lines;
        }
        let ocr_lines =
            match crate::ocr::run(pdf_bytes, &empty_pages, ocr_languages, deskew, dump_dir) {
                Ok(v) => v,
                Err(_) => return lines,
            };
        if ocr_lines.is_empty() {
            return lines;
        }
        // Vision observations are already line-grouped, so append them
        // as-is and only sort the outer line list by page + y-top. This
        // avoids the y-threshold heuristic breaking on rotated/rasterised
        // pages where Vision's bbox heights are anti-alias-inflated.
        let mut merged: Vec<Vec<TextSpan>> = lines;
        merged.extend(ocr_lines);
        merged.sort_by(|a, b| {
            let (pa, pb) = (
                a.first().map(|s| s.page_num).unwrap_or(0),
                b.first().map(|s| s.page_num).unwrap_or(0),
            );
            match pa.cmp(&pb) {
                std::cmp::Ordering::Equal => {
                    let ya = a.first().map(|s| s.bbox.t).unwrap_or(0.0);
                    let yb = b.first().map(|s| s.bbox.t).unwrap_or(0.0);
                    ya.total_cmp(&yb)
                }
                other => other,
            }
        });
        merged
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
