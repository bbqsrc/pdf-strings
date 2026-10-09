//! Document-level metadata — the `/Info` dictionary and XMP fields — read from
//! the PDF structure without extracting any text. This is what a caller uses to
//! recognize or fingerprint a document by its producing system rather than its
//! (often noisy, sometimes unextractable) visible content.

use std::collections::HashSet;
use std::path::Path;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use sha2::{Digest, Sha256};

use crate::error::OutputError;
use crate::utils::get_inherited;

/// A PDF's document-level metadata gathered for fingerprinting.
#[derive(Debug, Clone, Default)]
pub struct PdfMetadata {
    /// The PDF version, e.g. `"1.7"`.
    pub version: String,
    /// The page count.
    pub pages: usize,
    /// The `/Info` dictionary entries in document order — Producer, Creator,
    /// Author, Title, CreationDate, and so on — with values decoded to text.
    pub info: Vec<(String, String)>,
    /// The raw XMP metadata packet (XML) verbatim, when the catalog carries one.
    pub xmp: Option<String>,
}

impl PdfMetadata {
    /// The first `/Info` value whose key matches `key` case-insensitively.
    #[must_use]
    pub fn info(&self, key: &str) -> Option<&str> {
        self.info
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.as_str())
    }
}

/// Reads a PDF's metadata from a file without extracting its text.
pub fn metadata_from_path<P: AsRef<Path>>(path: P) -> Result<PdfMetadata, OutputError> {
    metadata_from_bytes(&std::fs::read(path)?)
}

/// Reads a PDF's metadata from bytes in memory without extracting its text.
pub fn metadata_from_bytes(bytes: &[u8]) -> Result<PdfMetadata, OutputError> {
    let doc = Document::load_mem(bytes)?;
    Ok(PdfMetadata {
        version: doc.version.clone(),
        pages: doc.get_pages().len(),
        info: info_entries(&doc),
        xmp: xmp_xml(&doc),
    })
}

fn info_entries(doc: &Document) -> Vec<(String, String)> {
    doc.trailer
        .get(b"Info")
        .ok()
        .and_then(|object| resolve_dictionary(doc, object))
        .map(|dict| {
            dict.iter()
                .map(|(key, value)| {
                    (
                        String::from_utf8_lossy(key).into_owned(),
                        object_text(value),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Resolves a possibly-indirect object reference to a dictionary.
fn resolve_dictionary<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    match object {
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        Object::Dictionary(dict) => Some(dict),
        _ => None,
    }
}

/// Renders a metadata value to text, decoding PDF string encodings.
fn object_text(value: &Object) -> String {
    match value {
        Object::String(bytes, _) => decode_pdf_string(bytes),
        Object::Name(name) => String::from_utf8_lossy(name).into_owned(),
        Object::Integer(number) => number.to_string(),
        Object::Real(number) => number.to_string(),
        Object::Boolean(flag) => flag.to_string(),
        other => format!("{other:?}"),
    }
}

/// Decodes a PDF text string: UTF-16BE when it carries a byte-order mark,
/// otherwise PDFDocEncoding treated as Latin-1 — adequate for the ASCII-ish
/// Producer/Creator strings this is used for.
fn decode_pdf_string(bytes: &[u8]) -> String {
    if let Some(be) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = be
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&byte| byte as char).collect()
    }
}

/// The raw XMP metadata packet, when the catalog carries an XMP stream.
fn xmp_xml(doc: &Document) -> Option<String> {
    let id = match doc.catalog().ok()?.get(b"Metadata").ok()? {
        Object::Reference(id) => *id,
        _ => return None,
    };
    let Object::Stream(stream) = doc.get_object(id).ok()? else {
        return None;
    };
    let content = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    Some(String::from_utf8_lossy(&content).into_owned())
}

/// One raster image (an `/XObject /Subtype /Image`) embedded in a PDF, described
/// for fingerprinting by its dimensions, encoding filter, and a content hash of
/// its raw encoded bytes. A vendor's logo embeds identically across its
/// documents, so a recurring hash is a strong, hard-to-forge signature that
/// survives generic metadata and an unextractable text layer.
#[derive(Debug, Clone)]
pub struct PdfImage {
    pub width: i64,
    pub height: i64,
    /// The stream filter(s), e.g. `DCTDecode` (JPEG) or `FlateDecode`.
    pub filter: String,
    /// The length of the raw encoded stream bytes.
    pub bytes: usize,
    /// The SHA-256 of the raw encoded stream bytes, hex-encoded.
    pub sha256: String,
}

/// Reads every embedded image from a PDF file for fingerprinting.
pub fn images_from_path<P: AsRef<Path>>(path: P) -> Result<Vec<PdfImage>, OutputError> {
    images_from_bytes(&std::fs::read(path)?)
}

/// Reads every embedded image from a PDF in memory, de-duplicated by object id
/// (an image shared across pages is counted once).
pub fn images_from_bytes(bytes: &[u8]) -> Result<Vec<PdfImage>, OutputError> {
    let doc = Document::load_mem(bytes)?;
    let mut images = Vec::new();
    let mut seen: HashSet<ObjectId> = HashSet::new();
    for (_page, page_id) in doc.get_pages() {
        let Ok(page) = doc.get_dictionary(page_id) else {
            continue;
        };
        let Some(resources) = get_inherited::<&Dictionary>(&doc, page, b"Resources") else {
            continue;
        };
        let Some(xobjects) = resources
            .get(b"XObject")
            .ok()
            .and_then(|object| resolve_dictionary(&doc, object))
        else {
            continue;
        };
        for (_name, entry) in xobjects.iter() {
            let Ok(id) = entry.as_reference() else {
                continue;
            };
            if !seen.insert(id) {
                continue;
            }
            let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) else {
                continue;
            };
            if stream
                .dict
                .get(b"Subtype")
                .ok()
                .and_then(|o| o.as_name().ok())
                == Some(&b"Image"[..])
            {
                images.push(describe_image(stream));
            }
        }
    }
    Ok(images)
}

fn describe_image(stream: &Stream) -> PdfImage {
    let dict = &stream.dict;
    let int = |key: &[u8]| {
        dict.get(key)
            .ok()
            .and_then(|o| o.as_i64().ok())
            .unwrap_or(0)
    };
    let mut hasher = Sha256::new();
    hasher.update(&stream.content);
    PdfImage {
        width: int(b"Width"),
        height: int(b"Height"),
        filter: filter_name(dict),
        bytes: stream.content.len(),
        sha256: format!("{:x}", hasher.finalize()),
    }
}

/// The stream's filter name(s), joining a filter chain with `+`.
fn filter_name(dict: &Dictionary) -> String {
    match dict.get(b"Filter") {
        Ok(Object::Name(name)) => String::from_utf8_lossy(name).into_owned(),
        Ok(Object::Array(items)) => items
            .iter()
            .filter_map(|o| o.as_name().ok())
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect::<Vec<_>>()
            .join("+"),
        _ => "(raw)".to_owned(),
    }
}

/// One font referenced by a PDF, described for fingerprinting by its base name
/// (with the random subset prefix stripped), its subtype, and whether the font
/// program is embedded. A vendor's brand typeface recurs across its documents;
/// an embedded, non-standard font name is a distinctive signal even when there
/// is no logo image.
#[derive(Debug, Clone)]
pub struct PdfFont {
    /// The `/BaseFont` name with any `ABCDEF+` subset prefix removed.
    pub base_font: String,
    /// The font subtype: `Type1`, `TrueType`, `Type0`, `Type3`, …
    pub subtype: String,
    /// Whether the font program is embedded (a `FontFile`/`FontFile2`/`FontFile3`).
    pub embedded: bool,
}

/// Reads the distinct set of fonts a PDF file references.
pub fn fonts_from_path<P: AsRef<Path>>(path: P) -> Result<Vec<PdfFont>, OutputError> {
    fonts_from_bytes(&std::fs::read(path)?)
}

/// Reads the distinct set of fonts a PDF references, de-duplicated by
/// name/subtype/embedded across pages.
pub fn fonts_from_bytes(bytes: &[u8]) -> Result<Vec<PdfFont>, OutputError> {
    let doc = Document::load_mem(bytes)?;
    let mut fonts = Vec::new();
    let mut seen: HashSet<(String, String, bool)> = HashSet::new();
    for (_page, page_id) in doc.get_pages() {
        let Ok(page) = doc.get_dictionary(page_id) else {
            continue;
        };
        let Some(resources) = get_inherited::<&Dictionary>(&doc, page, b"Resources") else {
            continue;
        };
        let Some(font_dict) = resources
            .get(b"Font")
            .ok()
            .and_then(|object| resolve_dictionary(&doc, object))
        else {
            continue;
        };
        for (_name, entry) in font_dict.iter() {
            let Some(font) = resolve_dictionary(&doc, entry) else {
                continue;
            };
            let subtype = name_string(font, b"Subtype");
            let base_font = strip_subset(&name_string(font, b"BaseFont")).to_owned();
            if base_font.is_empty() {
                continue;
            }
            let embedded = font_embedded(&doc, font);
            if seen.insert((base_font.clone(), subtype.clone(), embedded)) {
                fonts.push(PdfFont {
                    base_font,
                    subtype,
                    embedded,
                });
            }
        }
    }
    Ok(fonts)
}

fn name_string(dict: &Dictionary, key: &[u8]) -> String {
    dict.get(key)
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .unwrap_or_default()
}

/// Strips a `ABCDEF+` six-uppercase-letter subset prefix from a font name.
fn strip_subset(name: &str) -> &str {
    match name.split_once('+') {
        Some((prefix, rest))
            if prefix.len() == 6 && prefix.bytes().all(|b| b.is_ascii_uppercase()) =>
        {
            rest
        }
        _ => name,
    }
}

/// Whether a font dictionary embeds its program — directly, or for a `Type0`
/// composite font, through its descendant CIDFont's descriptor.
fn font_embedded(doc: &Document, font: &Dictionary) -> bool {
    if descriptor_has_fontfile(doc, font) {
        return true;
    }
    let Ok(descendants) = font.get(b"DescendantFonts") else {
        return false;
    };
    let array = match descendants {
        Object::Array(items) => items.clone(),
        Object::Reference(id) => match doc.get_object(*id) {
            Ok(Object::Array(items)) => items.clone(),
            _ => return false,
        },
        _ => return false,
    };
    array.iter().any(|item| {
        resolve_dictionary(doc, item).is_some_and(|cid| descriptor_has_fontfile(doc, cid))
    })
}

fn descriptor_has_fontfile(doc: &Document, font: &Dictionary) -> bool {
    font.get(b"FontDescriptor")
        .ok()
        .and_then(|object| resolve_dictionary(doc, object))
        .is_some_and(|descriptor| {
            descriptor.get(b"FontFile").is_ok()
                || descriptor.get(b"FontFile2").is_ok()
                || descriptor.get(b"FontFile3").is_ok()
        })
}

// ===== links, attachments, and assorted object-graph markers =====

/// Every hyperlink URI reachable from a PDF's page annotations — a vendor's
/// site/payment/portal domain is a strong, stable identity marker.
pub fn links_from_path<P: AsRef<Path>>(path: P) -> Result<Vec<String>, OutputError> {
    links_from_bytes(&std::fs::read(path)?)
}

/// Every hyperlink URI from the page annotations of a PDF in memory, de-duped.
pub fn links_from_bytes(bytes: &[u8]) -> Result<Vec<String>, OutputError> {
    let doc = Document::load_mem(bytes)?;
    let mut uris = Vec::new();
    let mut seen = HashSet::new();
    for (_page, page_id) in doc.get_pages() {
        let Ok(page) = doc.get_dictionary(page_id) else {
            continue;
        };
        for annot in resolve_array(&doc, page, b"Annots") {
            let Some(dict) = resolve_dictionary(&doc, &annot) else {
                continue;
            };
            if let Some(action) = dict
                .get(b"A")
                .ok()
                .and_then(|o| resolve_dictionary(&doc, o))
                && let Ok(Object::String(uri, _)) = action.get(b"URI")
            {
                let uri = decode_pdf_string(uri);
                if seen.insert(uri.clone()) {
                    uris.push(uri);
                }
            }
        }
    }
    Ok(uris)
}

/// An embedded file attachment (e.g. a Factur-X / ZUGFeRD invoice XML).
#[derive(Debug, Clone)]
pub struct PdfAttachment {
    pub name: String,
    /// The `/AFRelationship`, e.g. `Alternative` (Factur-X) or `Data`.
    pub relationship: String,
    pub bytes: usize,
    pub sha256: String,
}

/// Every embedded-file attachment of a PDF file.
pub fn attachments_from_path<P: AsRef<Path>>(path: P) -> Result<Vec<PdfAttachment>, OutputError> {
    attachments_from_bytes(&std::fs::read(path)?)
}

/// Every embedded-file attachment of a PDF in memory (the `/EmbeddedFiles` name
/// tree plus catalog `/AF` associated files).
pub fn attachments_from_bytes(bytes: &[u8]) -> Result<Vec<PdfAttachment>, OutputError> {
    let doc = Document::load_mem(bytes)?;
    let mut out = Vec::new();
    if let Ok(catalog) = doc.catalog() {
        if let Some(tree) = catalog
            .get(b"Names")
            .ok()
            .and_then(|o| resolve_dictionary(&doc, o))
            .and_then(|names| names.get(b"EmbeddedFiles").ok())
            .and_then(|o| resolve_dictionary(&doc, o))
        {
            collect_name_tree(&doc, tree, &mut out);
        }
        for item in resolve_array(&doc, catalog, b"AF") {
            if let Some(spec) = resolve_dictionary(&doc, &item)
                && let Some(attachment) = filespec_attachment(&doc, spec)
            {
                out.push(attachment);
            }
        }
    }
    Ok(out)
}

fn collect_name_tree(doc: &Document, node: &Dictionary, out: &mut Vec<PdfAttachment>) {
    if let Ok(Object::Array(names)) = node.get(b"Names") {
        let mut index = 0;
        while index + 1 < names.len() {
            let name = match &names[index] {
                Object::String(bytes, _) => decode_pdf_string(bytes),
                _ => String::new(),
            };
            if let Some(spec) = resolve_dictionary(doc, &names[index + 1])
                && let Some(mut attachment) = filespec_attachment(doc, spec)
            {
                if attachment.name.is_empty() {
                    attachment.name = name;
                }
                out.push(attachment);
            }
            index += 2;
        }
    }
    for kid in resolve_array(doc, node, b"Kids") {
        if let Some(child) = resolve_dictionary(doc, &kid) {
            collect_name_tree(doc, child, out);
        }
    }
}

fn filespec_attachment(doc: &Document, spec: &Dictionary) -> Option<PdfAttachment> {
    let name = string_value(spec, b"UF")
        .or_else(|| string_value(spec, b"F"))
        .unwrap_or_default();
    let relationship = name_string(spec, b"AFRelationship");
    let embedded = spec
        .get(b"EF")
        .ok()
        .and_then(|o| resolve_dictionary(doc, o))?;
    let stream_ref = embedded.get(b"F").or_else(|_| embedded.get(b"UF")).ok()?;
    let Object::Reference(id) = stream_ref else {
        return None;
    };
    let Ok(Object::Stream(stream)) = doc.get_object(*id) else {
        return None;
    };
    let content = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    let mut hasher = Sha256::new();
    hasher.update(&content);
    Some(PdfAttachment {
        name,
        relationship,
        bytes: content.len(),
        sha256: format!("{:x}", hasher.finalize()),
    })
}

/// Assorted object-graph markers gathered cheaply, without parsing content
/// streams: page sizes, and structural features that hint at the producing
/// pipeline or the vendor's template.
#[derive(Debug, Clone, Default)]
pub struct PdfMarkers {
    /// Distinct page sizes as rounded `(width, height)` in points.
    pub page_sizes: Vec<(i64, i64)>,
    pub linearized: bool,
    pub tagged: bool,
    pub has_javascript: bool,
    pub lang: Option<String>,
    pub output_intent: Option<String>,
    pub acroform_fields: Vec<String>,
    pub layer_names: Vec<String>,
    /// The application keys under the catalog `/PieceInfo` private-data dict.
    pub pieceinfo_apps: Vec<String>,
}

/// Reads the object-graph markers of a PDF file.
pub fn markers_from_path<P: AsRef<Path>>(path: P) -> Result<PdfMarkers, OutputError> {
    markers_from_bytes(&std::fs::read(path)?)
}

/// Reads the object-graph markers of a PDF in memory.
pub fn markers_from_bytes(bytes: &[u8]) -> Result<PdfMarkers, OutputError> {
    let doc = Document::load_mem(bytes)?;
    let catalog = doc.catalog().ok();
    let mut sizes = std::collections::BTreeSet::new();
    for (_page, page_id) in doc.get_pages() {
        if let Ok(page) = doc.get_dictionary(page_id)
            && let Some(media) = get_inherited::<Vec<f32>>(&doc, page, b"MediaBox")
            && media.len() == 4
        {
            sizes.insert((
                (media[2] - media[0]).round() as i64,
                (media[3] - media[1]).round() as i64,
            ));
        }
    }
    let head = &bytes[..bytes.len().min(2048)];
    Ok(PdfMarkers {
        page_sizes: sizes.into_iter().collect(),
        linearized: head.windows(11).any(|w| w == b"/Linearized"),
        tagged: catalog.is_some_and(|c| c.get(b"StructTreeRoot").is_ok()),
        has_javascript: catalog.is_some_and(|c| has_javascript(&doc, c)),
        lang: catalog.and_then(|c| string_value(c, b"Lang")),
        output_intent: catalog.and_then(|c| output_intent(&doc, c)),
        acroform_fields: catalog
            .map(|c| acroform_fields(&doc, c))
            .unwrap_or_default(),
        layer_names: catalog.map(|c| layer_names(&doc, c)).unwrap_or_default(),
        pieceinfo_apps: catalog
            .map(|c| dict_keys(&doc, c, b"PieceInfo"))
            .unwrap_or_default(),
    })
}

/// Resolves `dict[key]` to an owned array, following one indirect reference.
fn resolve_array(doc: &Document, dict: &Dictionary, key: &[u8]) -> Vec<Object> {
    match dict.get(key) {
        Ok(Object::Array(items)) => items.clone(),
        Ok(Object::Reference(id)) => match doc.get_object(*id) {
            Ok(Object::Array(items)) => items.clone(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn string_value(dict: &Dictionary, key: &[u8]) -> Option<String> {
    match dict.get(key) {
        Ok(Object::String(bytes, _)) => Some(decode_pdf_string(bytes)),
        Ok(Object::Name(name)) => Some(String::from_utf8_lossy(name).into_owned()),
        _ => None,
    }
}

fn has_javascript(doc: &Document, catalog: &Dictionary) -> bool {
    let via_names = catalog
        .get(b"Names")
        .ok()
        .and_then(|o| resolve_dictionary(doc, o))
        .is_some_and(|names| names.get(b"JavaScript").is_ok());
    let via_open = catalog
        .get(b"OpenAction")
        .ok()
        .and_then(|o| resolve_dictionary(doc, o))
        .and_then(|action| action.get(b"S").ok().and_then(|s| s.as_name().ok()))
        == Some(b"JavaScript");
    via_names || via_open
}

fn output_intent(doc: &Document, catalog: &Dictionary) -> Option<String> {
    let intents = resolve_array(doc, catalog, b"OutputIntents");
    let dict = resolve_dictionary(doc, intents.first()?)?;
    string_value(dict, b"OutputConditionIdentifier").or_else(|| string_value(dict, b"Info"))
}

fn acroform_fields(doc: &Document, catalog: &Dictionary) -> Vec<String> {
    let Some(acroform) = catalog
        .get(b"AcroForm")
        .ok()
        .and_then(|o| resolve_dictionary(doc, o))
    else {
        return Vec::new();
    };
    resolve_array(doc, acroform, b"Fields")
        .iter()
        .filter_map(|field| resolve_dictionary(doc, field))
        .filter_map(|field| string_value(field, b"T"))
        .collect()
}

fn layer_names(doc: &Document, catalog: &Dictionary) -> Vec<String> {
    let Some(ocp) = catalog
        .get(b"OCProperties")
        .ok()
        .and_then(|o| resolve_dictionary(doc, o))
    else {
        return Vec::new();
    };
    resolve_array(doc, ocp, b"OCGs")
        .iter()
        .filter_map(|group| resolve_dictionary(doc, group))
        .filter_map(|group| string_value(group, b"Name"))
        .collect()
}

fn dict_keys(doc: &Document, parent: &Dictionary, key: &[u8]) -> Vec<String> {
    parent
        .get(key)
        .ok()
        .and_then(|o| resolve_dictionary(doc, o))
        .map(|dict| {
            dict.iter()
                .map(|(name, _)| String::from_utf8_lossy(name).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

// ===== content-stream palette: font sizes, colors, line widths =====

/// The template-stable "palette" of a PDF's page content: the set of font sizes,
/// the set of stroke/fill colors, and the set of stroke line widths. Individual
/// line/box coordinates vary per invoice, but a vendor's typographic scale,
/// brand colors, and rule weights are template-fixed.
#[derive(Debug, Clone, Default)]
pub struct PdfContent {
    pub font_sizes: Vec<String>,
    pub colors: Vec<String>,
    pub line_widths: Vec<String>,
}

/// Reads the content-stream palette of a PDF file.
pub fn content_from_path<P: AsRef<Path>>(path: P) -> Result<PdfContent, OutputError> {
    content_from_bytes(&std::fs::read(path)?)
}

/// Reads the content-stream palette of a PDF in memory.
pub fn content_from_bytes(bytes: &[u8]) -> Result<PdfContent, OutputError> {
    let doc = Document::load_mem(bytes)?;
    let mut sizes = std::collections::BTreeSet::new();
    let mut colors = std::collections::BTreeSet::new();
    let mut widths = std::collections::BTreeSet::new();
    for (_page, page_id) in doc.get_pages() {
        let Ok(page) = doc.get_dictionary(page_id) else {
            continue;
        };
        scan_content(
            &page_content(&doc, page),
            &mut sizes,
            &mut colors,
            &mut widths,
        );
    }
    Ok(PdfContent {
        font_sizes: sizes.into_iter().collect(),
        colors: colors.into_iter().collect(),
        line_widths: widths.into_iter().collect(),
    })
}

/// The concatenated, decompressed content streams of a page.
fn page_content(doc: &Document, page: &Dictionary) -> Vec<u8> {
    let refs = match page.get(b"Contents") {
        Ok(Object::Reference(id)) => vec![*id],
        Ok(Object::Array(items)) => items.iter().filter_map(|o| o.as_reference().ok()).collect(),
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for id in refs {
        if let Ok(Object::Stream(stream)) = doc.get_object(id) {
            out.extend(
                stream
                    .decompressed_content()
                    .unwrap_or_else(|_| stream.content.clone()),
            );
            out.push(b'\n');
        }
    }
    out
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0 | 12)
}

fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// A minimal content-stream tokenizer that tracks a numeric/name operand stack
/// and fires on `Tf` (font size), `w` (line width), and the colour operators.
fn scan_content(
    data: &[u8],
    sizes: &mut std::collections::BTreeSet<String>,
    colors: &mut std::collections::BTreeSet<String>,
    widths: &mut std::collections::BTreeSet<String>,
) {
    let mut nums: Vec<f64> = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if is_ws(b) {
            i += 1;
        } else if b == b'%' {
            while i < data.len() && data[i] != b'\n' && data[i] != b'\r' {
                i += 1;
            }
        } else if b == b'(' {
            i = skip_literal_string(data, i);
            nums.clear();
        } else if b == b'<' {
            if data.get(i + 1) == Some(&b'<') {
                i += 2;
            } else {
                while i < data.len() && data[i] != b'>' {
                    i += 1;
                }
                i += 1;
            }
        } else if matches!(b, b'>' | b'[' | b']' | b'{' | b'}') {
            i += 1;
        } else if b == b'/' {
            i += 1;
            while i < data.len() && !is_ws(data[i]) && !is_delim(data[i]) {
                i += 1;
            }
        } else if b == b'+' || b == b'-' || b == b'.' || b.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < data.len() && (data[i] == b'.' || data[i] == b'-' || data[i].is_ascii_digit())
            {
                i += 1;
            }
            if let Ok(value) = std::str::from_utf8(&data[start..i])
                .unwrap_or("")
                .parse::<f64>()
            {
                nums.push(value);
            }
        } else {
            let start = i;
            while i < data.len() && !is_ws(data[i]) && !is_delim(data[i]) {
                i += 1;
            }
            let op = &data[start..i];
            match op {
                b"Tf" => {
                    if let Some(size) = nums.last() {
                        sizes.insert(format!("{:.1}", size.abs()));
                    }
                }
                b"w" => {
                    if let Some(width) = nums.last() {
                        widths.insert(format!("{width:.2}"));
                    }
                }
                b"rg" | b"RG" => insert_color(colors, &nums, 3, ""),
                b"g" | b"G" => insert_color(colors, &nums, 1, "g"),
                b"k" | b"K" => insert_color(colors, &nums, 4, "k"),
                b"ID" => i = skip_inline_image(data, i),
                _ => {}
            }
            nums.clear();
        }
    }
}

fn insert_color(
    colors: &mut std::collections::BTreeSet<String>,
    nums: &[f64],
    n: usize,
    tag: &str,
) {
    if nums.len() >= n {
        let parts: Vec<String> = nums[nums.len() - n..]
            .iter()
            .map(|v| format!("{v:.3}"))
            .collect();
        colors.insert(format!("{tag}{}", parts.join(",")));
    }
}

/// Skips a `(...)` literal string, honouring `\` escapes and nested parens.
fn skip_literal_string(data: &[u8], mut i: usize) -> usize {
    let mut depth = 0;
    while i < data.len() {
        match data[i] {
            b'\\' => i += 1,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// Skips inline-image binary data after an `ID` operator, up to `EI`.
fn skip_inline_image(data: &[u8], mut i: usize) -> usize {
    i += 1; // the single whitespace after ID
    while i + 1 < data.len() {
        if data[i] == b'E'
            && data[i + 1] == b'I'
            && (i == 0 || is_ws(data[i - 1]))
            && data.get(i + 2).is_none_or(|&b| is_ws(b) || is_delim(b))
        {
            return i + 2;
        }
        i += 1;
    }
    data.len()
}
