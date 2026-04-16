//! TeX font encoding → Unicode tables.
//!
//! Covers:
//! - OT1: Knuth's original 7-bit text encoding (CMR, CMBX, CMTI, CMSS,
//!   CMTT, CMCSC families).
//! - OML: math italic encoding (CMMI family) — Greek lowercase at 0x0B-
//!   0x29, italic Latin letters at ASCII positions.
//! - OMS: math symbols encoding (CMSY family) — relations, operators,
//!   arrows, script capitals.
//!
//! OMX (math extensions for large delimiters) is not populated — those
//! glyphs rarely carry text we care about extracting, and mapping them
//! to Unicode is ambiguous (no single "big integral" codepoint).

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Encoding {
    Ot1,
    Oml,
    Oms,
}

pub(crate) fn decode_byte(enc: Encoding, b: u8) -> &'static str {
    let tbl: &[&str] = match enc {
        Encoding::Ot1 => OT1_TABLE,
        Encoding::Oml => OML_TABLE,
        Encoding::Oms => OMS_TABLE,
    };
    tbl.get(b as usize).copied().unwrap_or("\u{FFFD}")
}

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

/// OML encoding (CMMI family — math italic).
/// Greek lowercase at 0x0B-0x29, italic Latin at 0x41-0x7A mapped to the
/// Unicode Mathematical Italic alphanumerics (U+1D434+ for capitals,
/// U+1D44E+ for lowercase, with U+210E for italic h per the Unicode
/// reservation). Preserves the semantic distinction between math
/// variables and plain text letters.
static OML_TABLE: &[&str] = &[
    // 0x00..=0x0F — Greek capitals
    "\u{0393}", "\u{0394}", "\u{0398}", "\u{039B}", "\u{039E}", "\u{03A0}",
    "\u{03A3}", "\u{03A5}", "\u{03A6}", "\u{03A8}", "\u{03A9}",
    // 0x0B..=0x29 — Greek lowercase
    "\u{03B1}", // α
    "\u{03B2}", // β
    "\u{03B3}", // γ
    "\u{03B4}", // δ
    "\u{03B5}", // ε
    // 0x10..=0x1F
    "\u{03B6}", // ζ
    "\u{03B7}", // η
    "\u{03B8}", // θ
    "\u{03B9}", // ι
    "\u{03BA}", // κ
    "\u{03BB}", // λ
    "\u{03BC}", // μ
    "\u{03BD}", // ν
    "\u{03BE}", // ξ
    "\u{03C0}", // π
    "\u{03C1}", // ρ
    "\u{03C3}", // σ
    "\u{03C4}", // τ
    "\u{03C5}", // υ
    "\u{03C6}", // φ
    "\u{03C7}", // χ
    // 0x20..=0x2F
    "\u{03C8}", // ψ
    "\u{03C9}", // ω
    "\u{03B5}", // ε variant
    "\u{03D1}", // ϑ vartheta
    "\u{03D6}", // ϖ varpi
    "\u{03F1}", // ϱ varrho
    "\u{03C2}", // ς final sigma
    "\u{03D5}", // ϕ varphi
    "\u{21BC}", // ↼
    "\u{21BD}", // ↽
    "\u{21C0}", // ⇀
    "\u{21C1}", // ⇁
    "\u{02D8}", // ˘
    ",",        // 0x2D - comma in italic
    ".",        // 0x2E
    "/",        // 0x2F
    // 0x30..=0x39 — italic digits, render as plain
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
    // 0x3A..=0x40
    ".", ",", "<", "/", ">", "\u{22C6}", // ⋆
    "\u{2202}", // ∂ partial
    // 0x41..=0x5A — math italic uppercase (U+1D434..U+1D44D)
    "\u{1D434}", "\u{1D435}", "\u{1D436}", "\u{1D437}", "\u{1D438}",
    "\u{1D439}", "\u{1D43A}", "\u{1D43B}", "\u{1D43C}", "\u{1D43D}",
    "\u{1D43E}", "\u{1D43F}", "\u{1D440}", "\u{1D441}", "\u{1D442}",
    "\u{1D443}", "\u{1D444}", "\u{1D445}", "\u{1D446}", "\u{1D447}",
    "\u{1D448}", "\u{1D449}", "\u{1D44A}", "\u{1D44B}", "\u{1D44C}",
    "\u{1D44D}",
    // 0x5B..=0x60
    "\u{266D}", // ♭ flat
    "\u{266E}", // ♮ natural
    "\u{266F}", // ♯ sharp
    "\u{2323}", // ⌣ smile
    "\u{2322}", // ⌢ frown
    "\u{2113}", // ℓ
    // 0x61..=0x7A — math italic lowercase (U+1D44E..U+1D467, with h = U+210E
    // because U+1D455 is explicitly reserved in Unicode)
    "\u{1D44E}", "\u{1D44F}", "\u{1D450}", "\u{1D451}", "\u{1D452}",
    "\u{1D453}", "\u{1D454}",
    "\u{210E}", // h — Planck constant slot
    "\u{1D456}", "\u{1D457}", "\u{1D458}", "\u{1D459}", "\u{1D45A}",
    "\u{1D45B}", "\u{1D45C}", "\u{1D45D}", "\u{1D45E}", "\u{1D45F}",
    "\u{1D460}", "\u{1D461}", "\u{1D462}", "\u{1D463}", "\u{1D464}",
    "\u{1D465}", "\u{1D466}", "\u{1D467}",
    // 0x7B..=0x7F
    "\u{0131}", // ı dotless i
    "\u{0237}", // ȷ dotless j
    "\u{2118}", // ℘ Weierstrass p
    "\u{20D7}", // ⃗ combining vector arrow
    "\u{0320}", // combining tie (approximation)
];

/// OMS encoding (CMSY family — math symbols / relations / arrows).
static OMS_TABLE: &[&str] = &[
    // 0x00..=0x0F — binary operators
    "\u{2212}", // − minus
    "\u{00B7}", // · middot
    "\u{00D7}", // × times
    "\u{2217}", // ∗ ast
    "\u{00F7}", // ÷ div
    "\u{22C4}", // ⋄ diamond
    "\u{00B1}", // ± plus-minus
    "\u{2213}", // ∓ minus-plus
    "\u{2295}", // ⊕
    "\u{2296}", // ⊖
    "\u{2297}", // ⊗
    "\u{2298}", // ⊘
    "\u{2299}", // ⊙
    "\u{20DD}", // ⃝ enclosing circle
    "\u{2218}", // ∘
    "\u{2022}", // • bullet
    // 0x10..=0x1F — relations
    "\u{224D}", // ≍
    "\u{2261}", // ≡
    "\u{2286}", // ⊆
    "\u{2287}", // ⊇
    "\u{2264}", // ≤
    "\u{2265}", // ≥
    "\u{2AAF}", // ⪯
    "\u{2AB0}", // ⪰
    "\u{223C}", // ∼
    "\u{2248}", // ≈
    "\u{2282}", // ⊂
    "\u{2283}", // ⊃
    "\u{226A}", // ≪
    "\u{226B}", // ≫
    "\u{227A}", // ≺
    "\u{227B}", // ≻
    // 0x20..=0x2F — arrows + more relations
    "\u{2190}", // ←
    "\u{2192}", // →
    "\u{2191}", // ↑
    "\u{2193}", // ↓
    "\u{2194}", // ↔
    "\u{2197}", // ↗
    "\u{2198}", // ↘
    "\u{2243}", // ≃
    "\u{21D0}", // ⇐
    "\u{21D2}", // ⇒
    "\u{21D1}", // ⇑
    "\u{21D3}", // ⇓
    "\u{21D4}", // ⇔
    "\u{2196}", // ↖
    "\u{2199}", // ↙
    "\u{221D}", // ∝
    // 0x30..=0x3F — quantifiers + top/bottom
    "\u{2032}", // ′
    "\u{221E}", // ∞
    "\u{2208}", // ∈
    "\u{220B}", // ∋
    "\u{25B3}", // △
    "\u{25BD}", // ▽
    "/",        // slash (for negation)
    "\u{21A6}", // ↦
    "\u{2200}", // ∀
    "\u{2203}", // ∃
    "\u{00AC}", // ¬
    "\u{2205}", // ∅
    "\u{211C}", // ℜ
    "\u{2111}", // ℑ
    "\u{22A4}", // ⊤
    "\u{22A5}", // ⊥
    // 0x40..=0x5A — alef + script caps
    "\u{2135}", // ℵ alef
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
    "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
    // 0x5B..=0x5F
    "\u{222A}", // ∪
    "\u{2229}", // ∩
    "\u{228E}", // ⊎ multiset union
    "\u{2227}", // ∧
    "\u{2228}", // ∨
    // 0x60..=0x6F — turnstiles, brackets
    "\u{22A2}", // ⊢
    "\u{22A3}", // ⊣
    "\u{230A}", // ⌊
    "\u{230B}", // ⌋
    "\u{2308}", // ⌈
    "\u{2309}", // ⌉
    "{", "}",
    "\u{27E8}", // ⟨
    "\u{27E9}", // ⟩
    "|",
    "\u{2016}", // ‖
    "\u{2195}", // ↕
    "\u{21D5}", // ⇕
    "\\",
    "\u{2240}", // ≀
    // 0x70..=0x7F — radicals, gradients, suits
    "\u{221A}", // √
    "\u{2A3F}", // ⨿
    "\u{2207}", // ∇
    "\u{222B}", // ∫
    "\u{2294}", // ⊔
    "\u{2293}", // ⊓
    "\u{2291}", // ⊑
    "\u{2292}", // ⊒
    "\u{00A7}", // §
    "\u{2020}", // †
    "\u{2021}", // ‡
    "\u{00B6}", // ¶
    "\u{2663}", // ♣
    "\u{2662}", // ♢
    "\u{2661}", // ♡
    "\u{2660}", // ♠
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

    #[test]
    fn tables_are_128_entries() {
        assert_eq!(OT1_TABLE.len(), 128, "OT1_TABLE");
        assert_eq!(OML_TABLE.len(), 128, "OML_TABLE");
        assert_eq!(OMS_TABLE.len(), 128, "OMS_TABLE");
    }

    #[test]
    fn oms_common_symbols() {
        assert_eq!(decode_byte(Encoding::Oms, 0x14), "\u{2264}"); // ≤
        assert_eq!(decode_byte(Encoding::Oms, 0x15), "\u{2265}"); // ≥
        assert_eq!(decode_byte(Encoding::Oms, 0x38), "\u{2200}"); // ∀
        assert_eq!(decode_byte(Encoding::Oms, 0x39), "\u{2203}"); // ∃
        assert_eq!(decode_byte(Encoding::Oms, 0x32), "\u{2208}"); // ∈
        assert_eq!(decode_byte(Encoding::Oms, 0x21), "\u{2192}"); // →
    }

    #[test]
    fn oml_italic_letters_unicode() {
        // OML italic letters map to the Unicode Mathematical Italic block
        // (U+1D434+ caps, U+1D44E+ lowercase, with U+210E for h).
        assert_eq!(decode_byte(Encoding::Oml, b'A'), "\u{1D434}");
        assert_eq!(decode_byte(Encoding::Oml, b'X'), "\u{1D44B}");
        assert_eq!(decode_byte(Encoding::Oml, b'a'), "\u{1D44E}");
        assert_eq!(decode_byte(Encoding::Oml, b'h'), "\u{210E}");
        assert_eq!(decode_byte(Encoding::Oml, b'x'), "\u{1D465}");
        assert_eq!(decode_byte(Encoding::Oml, b'z'), "\u{1D467}");
    }

    #[test]
    fn oml_greek_lowercase() {
        assert_eq!(decode_byte(Encoding::Oml, 0x0B), "\u{03B1}"); // α
        assert_eq!(decode_byte(Encoding::Oml, 0x15), "\u{03BB}"); // λ
    }
}
