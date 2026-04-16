//! PDF text extraction library
//!
//! This library provides functionality for extracting text content from PDF files,
//! handling font encodings, glyph mappings, and text positioning.

mod data;
mod error;
mod extract;
mod fonts;
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
    PdfExtractor, PdfExtractorBuilder, from_bytes, from_path, from_ps_bytes, from_ps_path,
    from_ps_reader, from_reader,
};

// Re-export public types
pub use types::{BoundingBox, MediaBox, Point, TextLine, TextOutput, TextPage, TextSpan};
