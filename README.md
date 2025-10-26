## pdf-extract

<!-- [![Build Status](https://github.com/bbqsrc/pdf-extract/actions/workflows/rust.yml/badge.svg)](https://github.com/bbqsrc/pdf-strings/actions) -->
[![crates.io](https://img.shields.io/crates/v/pdf-strings.svg)](https://crates.io/crates/pdf-strings)
[![Documentation](https://docs.rs/pdf-extract/badge.svg)](https://docs.rs/pdf-strings)

Extract text from PDFs with position data.

## Usage

```rust
// Simple extraction
let output = pdf_strings::from_path("file.pdf")?;
println!("{}", output);  // Plain text

// With password
let output = pdf_strings::PdfExtractor::builder()
    .password("secret")
    .build()
    .from_path("encrypted.pdf")?;

// Preserve spatial layout
println!("{}", output.to_string_pretty());

// Access structured data with bounding boxes
for line in output.lines() {
    for span in line {
        println!("{} at {:?}", span.text, span.bbox);
    }
}
```

## Features

- Plain text extraction
- Spatial layout preservation
- Bounding box coordinates for every text span
- Font encoding resolution (ToUnicode, Type1, TrueType, CID, Type3)
- Password-protected PDF support
- Handles complex fonts, rotated text, and multi-column layouts

## API

Three output formats:
- `to_string()` - Plain text
- `to_string_pretty()` - Character grid rendering that preserves spatial layout
- `lines()` - Structured data with `TextSpan` objects containing text, bounding boxes, and font sizes

## Acknowledgements

This is a fork of [pdf-extract](https://github.com/jrmuizel/pdf-extract). Thanks for laying the groundwork, PDFs are ... something else.
