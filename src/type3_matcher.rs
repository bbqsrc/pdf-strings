//! Recover Unicode from Type3 bitmap glyphs by matching against reference
//! glyphs rasterised from embedded Computer Modern / Latin Modern OTFs.
//!
//! On first use, each embedded font is walked via its cmap and every glyph
//! with a known codepoint is rasterised, normalised to a 32×32 signature,
//! and pushed into a global reference table. A CharProc bitmap is then
//! matched by nearest Hamming distance in the signature space; if nothing
//! lands within `ACCEPTABLE` bits, we return None (caller emits U+FFFD).
//!
//! The reference pool is deliberately narrow — only regular/bold/italic
//! Roman from cm-unicode and Latin Modern 10pt. Math, sans-serif, and
//! typewriter coverage can be added by expanding `REFERENCE_FONTS`.

use std::collections::HashSet;
use std::sync::OnceLock;

use tracing::debug;

use crate::type3_bitmap::Bitmap;

/// Side of the square signature grid. A 32×32 grid gives 1024 bits of
/// shape information — enough to distinguish most Latin letterforms while
/// remaining cheap to compare by popcount-xor.
const NORM_SIZE: u32 = 32;
const SIG_BYTES: usize = (NORM_SIZE * NORM_SIZE / 8) as usize;
type Signature = [u8; SIG_BYTES];

/// Reject anything worse than this — shapes that different are unlikely to
/// be the same letter. Chosen empirically against CADE1996.pdf: at 140
/// we get the obvious body-text matches without too many confident-but-
/// wrong mappings. Remaining uncertainty → U+FFFD in the caller.
const ACCEPTABLE_HAMMING: u32 = 140;

const REFERENCE_FONTS: &[(&str, &[u8])] = &[
    ("cmunrm", include_bytes!("data/fonts/cmunrm.otf")),
    ("cmunbx", include_bytes!("data/fonts/cmunbx.otf")),
    ("cmunti", include_bytes!("data/fonts/cmunti.otf")),
    (
        "lmroman10-regular",
        include_bytes!("data/fonts/lmroman10-regular.otf"),
    ),
    (
        "lmroman10-bold",
        include_bytes!("data/fonts/lmroman10-bold.otf"),
    ),
    (
        "lmroman10-italic",
        include_bytes!("data/fonts/lmroman10-italic.otf"),
    ),
];

struct ReferenceEntry {
    sig: Signature,
    unicode: String,
}

static REFERENCE: OnceLock<Vec<ReferenceEntry>> = OnceLock::new();

/// Produce a 32×32 1-bpp signature from a bitmap: crop to ink bbox, centre
/// the ink inside a max(w,h)-sided square, resample via nearest-source.
fn signature(bm: &Bitmap) -> Option<Signature> {
    let (x0, y0, x1, y1) = bm.ink_bbox()?;
    let w = (x1 - x0) as i32;
    let h = (y1 - y0) as i32;
    let side = w.max(h).max(1);
    let off_x = (side - w) / 2;
    let off_y = (side - h) / 2;

    let mut sig = [0u8; SIG_BYTES];
    for oy in 0..NORM_SIZE {
        for ox in 0..NORM_SIZE {
            let sqx = (ox as i32 * side) / NORM_SIZE as i32;
            let sqy = (oy as i32 * side) / NORM_SIZE as i32;
            let ink_x = sqx - off_x;
            let ink_y = sqy - off_y;
            if ink_x < 0 || ink_y < 0 || ink_x >= w || ink_y >= h {
                continue;
            }
            if bm.get((x0 as i32 + ink_x) as u32, (y0 as i32 + ink_y) as u32) {
                let row_stride = (NORM_SIZE / 8) as usize;
                sig[oy as usize * row_stride + (ox / 8) as usize] |= 1 << (7 - (ox % 8));
            }
        }
    }
    Some(sig)
}

fn hamming(a: &Signature, b: &Signature) -> u32 {
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| (x ^ y).count_ones())
        .sum()
}

fn gray_to_bitmap(gray: &[u8], w: u32, h: u32) -> Bitmap {
    let row_stride = w.div_ceil(8) as usize;
    let mut pixels = vec![0u8; row_stride * h as usize];
    for y in 0..h {
        for x in 0..w {
            if gray[(y * w + x) as usize] >= 128 {
                pixels[(y as usize) * row_stride + (x / 8) as usize] |= 1 << (7 - (x % 8));
            }
        }
    }
    Bitmap {
        width: w,
        height: h,
        pixels,
    }
}

/// Codepoints worth including in the reference pool. Keep it narrow to
/// limit false positives — a dvips LaTeX PDF will overwhelmingly use
/// Latin letters, digits, common punctuation, basic math symbols, and
/// Greek letters for math mode.
///
/// Greek letters that render visually identical to a Latin letter
/// (Α/A, Β/B, Ε/E, Η/H, Ι/I, Κ/K, Μ/M, Ν/N, Ο/O, Ρ/P, Τ/T, Υ/Y,
/// Χ/X, ο/o, ν/v, …) are excluded: a shape hash cannot tell them
/// apart and their presence only produces Latin/Greek tie-breaking
/// ambiguity. The Latin form wins by default.
fn is_useful_codepoint(cp: u32) -> bool {
    matches!(cp,
        0x0021..=0x007E           // ASCII printable
        | 0x00A1..=0x00FF         // Latin-1 Supplement (accented letters, punctuation)
        | 0x0100..=0x017F         // Latin Extended-A
        | 0x0393                  // Gamma
        | 0x0394                  // Delta
        | 0x0398                  // Theta
        | 0x039B                  // Lambda
        | 0x039E                  // Xi
        | 0x03A0                  // Pi
        | 0x03A3                  // Sigma
        | 0x03A6                  // Phi
        | 0x03A8                  // Psi
        | 0x03A9                  // Omega
        | 0x03B1..=0x03BC         // α-μ (skip ν ο which look like v o)
        | 0x03BE..=0x03BE         // ξ
        | 0x03C0..=0x03C9         // π-ω (skip ο)
        | 0x2010..=0x2027         // General punctuation (hyphens, dashes, quotes)
        | 0x2030..=0x203E         // per-mille, primes, overline
        | 0x2044                  // fraction slash
        | 0x2070..=0x209F         // super/subscripts
        | 0x2190..=0x21FF         // arrows
        | 0x2200..=0x22FF         // math operators
        | 0x2308..=0x230B         // ceil/floor brackets
        | 0x27E6..=0x27EB         // math white brackets
        | 0xFB00..=0xFB06         // ff/fi/fl/ffi/ffl ligatures
    )
}

fn build_reference() -> Vec<ReferenceEntry> {
    let mut entries = Vec::new();
    for (name, data) in REFERENCE_FONTS {
        let Ok(font) = fontdue::Font::from_bytes(*data, fontdue::FontSettings::default()) else {
            debug!("fontdue failed to parse {}", name);
            continue;
        };
        let Ok(face) = ttf_parser::Face::parse(data, 0) else {
            debug!("ttf-parser failed to parse {}", name);
            continue;
        };
        let Some(cmap) = face.tables().cmap else {
            continue;
        };

        let mut seen: HashSet<u32> = HashSet::new();
        let mut glyph_count = 0;
        for subtable in cmap.subtables {
            let is_unicode = matches!(
                (subtable.platform_id, subtable.encoding_id),
                (ttf_parser::PlatformId::Unicode, _) | (ttf_parser::PlatformId::Windows, 1 | 10)
            );
            if !is_unicode {
                continue;
            }
            subtable.codepoints(|cp| {
                if !seen.insert(cp) || !is_useful_codepoint(cp) {
                    return;
                }
                let Some(ch) = char::from_u32(cp) else {
                    return;
                };

                let (metrics, bitmap_gray) = font.rasterize(ch, 64.0);
                if metrics.width == 0 || metrics.height == 0 {
                    return;
                }
                let bm = gray_to_bitmap(
                    &bitmap_gray,
                    metrics.width as u32,
                    metrics.height as u32,
                );
                let Some(sig) = signature(&bm) else {
                    return;
                };
                entries.push(ReferenceEntry {
                    sig,
                    unicode: ch.to_string(),
                });
                glyph_count += 1;
            });
        }
        debug!("reference pool: {} glyphs from {}", glyph_count, name);
    }
    debug!("reference pool: {} total entries", entries.len());
    entries
}

/// Result of looking up a bitmap against the reference pool.
#[allow(dead_code)] // runner_up_* are only read by #[ignore]-gated debug tests
pub(crate) struct MatchResult {
    pub unicode: String,
    pub distance: u32,
    pub runner_up_distance: u32,
    pub runner_up_unicode: String,
}

/// Match a Type3 CharProc bitmap against the embedded reference pool.
/// Returns the matching Unicode string, or None for no confident match.
pub(crate) fn match_bitmap(bm: &Bitmap) -> Option<String> {
    best_match(bm)
        .filter(|m| m.distance <= ACCEPTABLE_HAMMING)
        .map(|m| m.unicode)
}

/// Find the closest reference entry regardless of threshold. Used by
/// callers that want to inspect the match quality (e.g. diagnostics).
pub(crate) fn best_match(bm: &Bitmap) -> Option<MatchResult> {
    let reference = REFERENCE.get_or_init(build_reference);
    let target = signature(bm)?;

    let mut best: Option<(&ReferenceEntry, u32)> = None;
    let mut runner_up: Option<(&ReferenceEntry, u32)> = None;
    for entry in reference {
        let d = hamming(&entry.sig, &target);
        match best {
            None => best = Some((entry, d)),
            Some((cur, cur_d)) if d < cur_d => {
                if !cur.unicode.eq(&entry.unicode) {
                    runner_up = Some((cur, cur_d));
                }
                best = Some((entry, d));
            }
            Some((cur, _)) if !cur.unicode.eq(&entry.unicode) => match runner_up {
                None => runner_up = Some((entry, d)),
                Some((_, ru_d)) if d < ru_d => runner_up = Some((entry, d)),
                _ => {}
            },
            _ => {}
        }
    }
    let (entry, d) = best?;
    let (ru_entry_u, ru_d) = runner_up
        .map(|(e, d)| (e.unicode.clone(), d))
        .unwrap_or_else(|| (String::new(), u32::MAX));
    Some(MatchResult {
        unicode: entry.unicode.clone(),
        distance: d,
        runner_up_distance: ru_d,
        runner_up_unicode: ru_entry_u,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_translation_invariant() {
        // Same ink in different positions in the buffer should hash identically.
        let ink: Vec<(u32, u32)> = vec![(1, 1), (2, 1), (1, 2), (2, 2)];
        let make = |w: u32, h: u32, dx: u32, dy: u32| -> Bitmap {
            let row_stride = w.div_ceil(8) as usize;
            let mut pixels = vec![0u8; row_stride * h as usize];
            for (x, y) in &ink {
                let xx = x + dx;
                let yy = y + dy;
                pixels[yy as usize * row_stride + (xx / 8) as usize] |= 1 << (7 - (xx % 8));
            }
            Bitmap {
                width: w,
                height: h,
                pixels,
            }
        };
        let a = make(8, 8, 0, 0);
        let b = make(16, 16, 4, 4);
        assert_eq!(signature(&a), signature(&b));
    }

    /// Round-trip check: rasterise 'A' from the embedded CMU font, signature
    /// it, and verify `match_bitmap` recovers "A".
    #[test]
    fn matches_own_reference_glyph() {
        let (_, data) = REFERENCE_FONTS[0];
        let font = fontdue::Font::from_bytes(data, Default::default()).unwrap();
        let (metrics, gray) = font.rasterize('A', 64.0);
        let bm = gray_to_bitmap(&gray, metrics.width as u32, metrics.height as u32);
        assert_eq!(match_bitmap(&bm).as_deref(), Some("A"));
    }

    /// Safe end-to-end coverage check: extract CADE1996.pdf via the public
    /// API and report only COUNTS — never the recovered text itself, which
    /// can contain enough garbled bytes to trip protective filters in
    /// downstream tools. Run with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn cade1996_match_coverage() {
        let pdf_path = std::env::var("CADE1996_PDF")
            .unwrap_or_else(|_| std::env::var("HOME").unwrap() + "/Downloads/CADE1996.pdf");
        let Ok(output) = crate::from_path(&pdf_path) else {
            eprintln!("skipped: {} not readable", pdf_path);
            return;
        };
        let text: String = output
            .lines()
            .iter()
            .flatten()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let total = text.chars().count();
        let replacements = text.chars().filter(|c| *c == '\u{FFFD}').count();
        let matched = total - replacements;
        eprintln!(
            "CADE1996: {} chars total, {} matched, {} U+FFFD ({}% match)",
            total,
            matched,
            replacements,
            if total > 0 { matched * 100 / total } else { 0 }
        );
        assert!(total > 500, "expected at least 500 extracted chars");
        // CADE1996 is math-heavy; math fonts B/C/D have no matches in our
        // Roman-only reference pool and correctly emit U+FFFD. 20% is a
        // calibration floor against the Roman body-text portion of the
        // paper; regression beneath it signals a real matcher break.
        assert!(
            matched * 5 > total,
            "expected > 20% match rate; got {}/{}",
            matched,
            total
        );
    }

    /// Live coverage check against CADE1996.pdf — decode every CharProc in
    /// font "A" and report how many get a match + the first ~50 mappings.
    /// Gated on the file existing; run with `--ignored --nocapture`.
    ///
    /// WARNING: this test prints per-glyph mappings. Enable only when
    /// actively debugging; the output can include bytes that downstream
    /// filters classify as hostile.
    #[test]
    #[ignore]
    fn match_cade1996_font_a() {
        use crate::type3_bitmap::extract_bitmaps;
        use crate::utils::maybe_deref;
        use lopdf::{Document, Object};
        use std::collections::HashMap;

        let pdf_path = std::env::var("CADE1996_PDF")
            .unwrap_or_else(|_| std::env::var("HOME").unwrap() + "/Downloads/CADE1996.pdf");
        let Ok(doc) = Document::load(&pdf_path) else {
            eprintln!("skipped: {} not readable", pdf_path);
            return;
        };

        for (_obj_id, obj) in doc.objects.iter() {
            let Object::Dictionary(dict) = obj else {
                continue;
            };
            let Ok(Object::Name(subtype)) = dict.get(b"Subtype") else {
                continue;
            };
            if subtype != b"Type3" {
                continue;
            }
            let font_name = dict
                .get(b"Name")
                .ok()
                .and_then(|o| o.as_name().ok())
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_else(|| "?".into());
            if font_name != "A" {
                continue;
            }

            let mut code_to_name: HashMap<u32, String> = HashMap::new();
            if let Ok(enc_obj) = dict.get(b"Encoding") {
                let enc = maybe_deref(&doc, enc_obj);
                if let Ok(enc_dict) = enc.as_dict() {
                    if let Ok(Object::Array(diffs)) = enc_dict.get(b"Differences") {
                        let mut code = 0i64;
                        for o in diffs {
                            let o = maybe_deref(&doc, o);
                            match o {
                                Object::Integer(i) => code = *i,
                                Object::Name(n) => {
                                    code_to_name.insert(
                                        code as u32,
                                        String::from_utf8_lossy(n).into_owned(),
                                    );
                                    code += 1;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            let bitmaps = extract_bitmaps(&doc, dict, &code_to_name);
            let mut matched = 0usize;
            let mut unmatched = 0usize;
            let mut codes: Vec<_> = bitmaps.keys().copied().collect();
            codes.sort();
            for code in codes {
                let bm = &bitmaps[&code];
                let m = best_match(bm);
                let accepted = match_bitmap(bm);
                match (accepted, m) {
                    (Some(s), _) => {
                        matched += 1;
                        eprintln!(
                            "  ✓ code {:3} /{:>4} → {:?}  ({}x{})",
                            code, code_to_name[&code], s, bm.width, bm.height
                        );
                    }
                    (None, Some(m)) => {
                        unmatched += 1;
                        if unmatched <= 40 {
                            eprintln!(
                                "  ✗ code {:3} /{:>4} best={:?} d={} ru={:?} rud={}  ({}x{})",
                                code,
                                code_to_name[&code],
                                m.unicode,
                                m.distance,
                                m.runner_up_unicode,
                                m.runner_up_distance,
                                bm.width,
                                bm.height,
                            );
                        }
                    }
                    (None, None) => {
                        unmatched += 1;
                    }
                }
            }
            eprintln!(
                "Font A: {} matched, {} unmatched (of {} extracted)",
                matched,
                unmatched,
                bitmaps.len()
            );
            return;
        }
        panic!("no Type3 font named A found in {}", pdf_path);
    }
}
