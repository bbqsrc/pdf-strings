//! PostScript input support.
//!
//! Scope: dvips-emitted PostScript from LaTeX toolchains (the class that
//! produces documents like `CADE1996.ps`). General PostScript — arbitrary
//! font encodings, Type1 embedded fonts, vector drawing — is out of
//! scope; non-dvips PostScript is rejected with a clear error.

mod encodings;
mod extractor;
mod tokeniser;

pub(crate) use extractor::extract_lines;
