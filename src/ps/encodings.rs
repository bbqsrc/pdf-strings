//! TeX font encoding → Unicode tables.
//!
//! Only the OT1 encoding (Knuth's original Computer Modern 7-bit encoding,
//! used by cmr, cmbx, cmti, cmss, cmtt, cmcsc, cmu, and similar) is
//! populated for v1 — it covers the body-text portion of a LaTeX-to-PS
//! document, which is what we primarily want to read. OMS (math italic +
//! math symbols) and OMX (extensible delimiters) are stubs for now.

/// Map an OT1-encoded byte to a Unicode string. Returns a single codepoint
/// for most bytes, a short string for ligatures (`fi` → "fi"), and the
/// replacement character for genuinely unmapped codes.
pub(crate) fn ot1_to_unicode(b: u8) -> &'static str {
    OT1_TABLE.get(b as usize).copied().unwrap_or("\u{FFFD}")
}

/// OT1 encoding table, 0..=127.
///
/// Sources: Knuth's "The TeXbook", Appendix F; the plain.tex font-loading
/// section; TeX font-encoding specification OT1.
static OT1_TABLE: &[&str] = &[
    // 0x00..=0x0F
    "\u{0393}", // 0x00 Γ GREEK CAPITAL LETTER GAMMA
    "\u{0394}", // 0x01 Δ DELTA
    "\u{0398}", // 0x02 Θ THETA
    "\u{039B}", // 0x03 Λ LAMBDA
    "\u{039E}", // 0x04 Ξ XI
    "\u{03A0}", // 0x05 Π PI
    "\u{03A3}", // 0x06 Σ SIGMA
    "\u{03A5}", // 0x07 Υ UPSILON
    "\u{03A6}", // 0x08 Φ PHI
    "\u{03A8}", // 0x09 Ψ PSI
    "\u{03A9}", // 0x0A Ω OMEGA
    "ff",       // 0x0B ff ligature
    "fi",       // 0x0C fi ligature
    "fl",       // 0x0D fl ligature
    "ffi",      // 0x0E ffi ligature
    "ffl",      // 0x0F ffl ligature
    // 0x10..=0x1F
    "\u{0131}",       // 0x10 ı dotless i
    "\u{0237}",       // 0x11 ȷ dotless j
    "\u{0060}",       // 0x12 ` grave (spacing)
    "\u{00B4}",       // 0x13 ´ acute (spacing)
    "\u{02C7}",       // 0x14 ˇ caron
    "\u{02D8}",       // 0x15 ˘ breve
    "\u{00AF}",       // 0x16 ¯ macron
    "\u{02DA}",       // 0x17 ˚ ring
    "\u{00B8}",       // 0x18 ¸ cedilla
    "\u{00DF}",       // 0x19 ß German sharp s
    "\u{00E6}",       // 0x1A æ
    "\u{0153}",       // 0x1B œ
    "\u{00F8}",       // 0x1C ø
    "\u{00C6}",       // 0x1D Æ
    "\u{0152}",       // 0x1E Œ
    "\u{00D8}",       // 0x1F Ø
    // 0x20..=0x2F — mostly ASCII, with OT1 specials.
    "\u{0020}", // 0x20 space (some OT1 sources emit suppressed "space" but U+0020 is the safe default)
    "!",        // 0x21
    "\u{201D}", // 0x22 " → ” (OT1 renders " as right-double-quote)
    "#",        // 0x23
    "$",        // 0x24
    "%",        // 0x25
    "&",        // 0x26
    "\u{2019}", // 0x27 ' → ’ (right single quote)
    "(",        // 0x28
    ")",        // 0x29
    "*",        // 0x2A
    "+",        // 0x2B
    ",",        // 0x2C
    "-",        // 0x2D
    ".",        // 0x2E
    "/",        // 0x2F
    // 0x30..=0x3F
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", ":", ";",
    "\u{00A1}", // 0x3C < → ¡ (OT1)
    "=", //
    "\u{00BF}", // 0x3E > → ¿ (OT1)
    "?",        // 0x3F
    // 0x40..=0x5F — ASCII uppercase with tweaks
    "@", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
    "S", "T", "U", "V", "W", "X", "Y", "Z", "[",
    "\u{201C}", // 0x5C \\ → “ (left double quote)
    "]",
    "\u{02C6}", // 0x5E ^ → ˆ circumflex
    "\u{02D9}", // 0x5F _ → ˙ dot above
    // 0x60..=0x7F
    "\u{2018}", // 0x60 ` → ‘ (left single quote)
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z",
    "\u{2013}", // 0x7B { → – en-dash
    "\u{2014}", // 0x7C | → — em-dash
    "\u{02DD}", // 0x7D } → ˝ Hungarian umlaut
    "\u{02DC}", // 0x7E ~ → ˜ tilde above
    "\u{00A8}", // 0x7F DEL → ¨ diaeresis (per OT1)
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ot1_ascii_letters() {
        assert_eq!(ot1_to_unicode(b'W'), "W");
        assert_eq!(ot1_to_unicode(b'e'), "e");
        assert_eq!(ot1_to_unicode(b' '), " ");
    }

    #[test]
    fn ot1_ligatures() {
        assert_eq!(ot1_to_unicode(0x0B), "ff");
        assert_eq!(ot1_to_unicode(0x0C), "fi");
        assert_eq!(ot1_to_unicode(0x0D), "fl");
        assert_eq!(ot1_to_unicode(0x0E), "ffi");
        assert_eq!(ot1_to_unicode(0x0F), "ffl");
    }

    #[test]
    fn ot1_greek() {
        assert_eq!(ot1_to_unicode(0x00), "\u{0393}"); // Γ
        assert_eq!(ot1_to_unicode(0x0A), "\u{03A9}"); // Ω
    }

    #[test]
    fn ot1_ot1_substitutions() {
        // OT1 remaps some ASCII bytes to typographic characters.
        assert_eq!(ot1_to_unicode(b'\''), "\u{2019}"); // right quote
        assert_eq!(ot1_to_unicode(b'`'), "\u{2018}"); // left quote
        assert_eq!(ot1_to_unicode(b'"'), "\u{201D}"); // right double
        assert_eq!(ot1_to_unicode(b'\\'), "\u{201C}"); // left double
        assert_eq!(ot1_to_unicode(b'<'), "\u{00A1}"); // ¡
        assert_eq!(ot1_to_unicode(b'>'), "\u{00BF}"); // ¿
    }

    #[test]
    fn ot1_out_of_range() {
        assert_eq!(ot1_to_unicode(0x80), "\u{FFFD}");
        assert_eq!(ot1_to_unicode(0xFF), "\u{FFFD}");
    }
}
