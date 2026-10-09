//! PDF text extraction library
//!
//! This library provides functionality for extracting text content from PDF files,
//! handling font encodings, glyph mappings, and text positioning.

mod data;
mod error;
mod extract;
mod fonts;
mod metadata;
#[cfg(target_os = "macos")]
mod ocr;
mod output;
mod processor;
mod ps;
mod type3_bitmap;
mod type3_matcher;
mod types;
mod utils;

// Re-export error type
pub use error::OutputError;

// Re-export extraction API
pub use extract::{
    OcrPolicy, PdfExtractor, PdfExtractorBuilder, from_bytes, from_path, from_ps_bytes,
    from_ps_path, from_ps_reader, from_reader,
};

// Re-export public types
pub use types::{BoundingBox, MediaBox, Point, TextLine, TextOutput, TextPage, TextSpan};

// Re-export document metadata + embedded images + fonts + object-graph markers
// (fingerprinting)
pub use metadata::{
    PdfAttachment, PdfContent, PdfFont, PdfImage, PdfMarkers, PdfMetadata, attachments_from_bytes,
    attachments_from_path, content_from_bytes, content_from_path, fonts_from_bytes,
    fonts_from_path, images_from_bytes, images_from_path, links_from_bytes, links_from_path,
    markers_from_bytes, markers_from_path, metadata_from_bytes, metadata_from_path,
};
