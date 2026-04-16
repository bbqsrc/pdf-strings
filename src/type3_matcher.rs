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
/// be the same letter. Chosen from the CADE1996 distance histogram: the
/// peak of true-positive matches sits in 100-200 bits (font variation
/// + size quantisation accounts for ~10-18% of pixels), with the
/// distribution tailing off beyond 200. 180 empirically captures most
/// of the genuine matches while keeping the long tail (likely wrong or
/// out-of-pool glyphs) rejected. Remaining uncertainty → U+FFFD.
const ACCEPTABLE_HAMMING: u32 = 260;

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
    (
        "lmromanslant10-regular",
        include_bytes!("data/fonts/lmromanslant10-regular.otf"),
    ),
    (
        "lmsans10-regular",
        include_bytes!("data/fonts/lmsans10-regular.otf"),
    ),
    (
        "lmsans10-bold",
        include_bytes!("data/fonts/lmsans10-bold.otf"),
    ),
    (
        "lmmono10-regular",
        include_bytes!("data/fonts/lmmono10-regular.otf"),
    ),
    (
        "latinmodern-math",
        include_bytes!("data/fonts/latinmodern-math.otf"),
    ),
];

struct ReferenceEntry {
    sig: Signature,
    unicode: String,
    aspect: u8,
}

/// Max difference in aspect bucket between target and candidate we will tolerate.
/// 1 means "adjacent buckets allowed" — lets e.g. an 'i' (tall) match a slightly
/// wider variant while still excluding square and wide glyphs.
const ASPECT_TOLERANCE: i32 = 1;

static REFERENCE: OnceLock<Vec<ReferenceEntry>> = OnceLock::new();

/// 0-7 bucket representing glyph shape ratio. 0 = very tall, ~3 = square,
/// 7 = very wide. Scales smoothly with w / (w + h).
fn aspect_bucket(w: u32, h: u32) -> u8 {
    let w = w as u64;
    let h = h.max(1) as u64;
    ((w * 7) / (w + h)).min(7) as u8
}

/// Produce a 32×32 1-bpp signature from a bitmap: crop to ink bbox, centre
/// the ink inside a max(w,h)-sided square, resample via nearest-source.
/// Also returns the glyph's aspect bucket for candidate pruning.
fn signature(bm: &Bitmap) -> Option<(Signature, u8)> {
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
    Some((sig, aspect_bucket(w as u32, h as u32)))
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

/// Codepoints worth including in the reference pool. Broad by design:
/// a dvips LaTeX PDF can use Latin letters, digits, punctuation, a wide
/// range of math operators, Greek, blackboard-bold / script / fraktur
/// (theorem names), arrows, and ligatures. We admit the Latin/Greek
/// look-alike ambiguity (e.g. Α/A, Β/B, Ο/o) — shape hashing cannot
/// distinguish them, and dropping either side means losing real hits.
fn is_useful_codepoint(cp: u32) -> bool {
    matches!(cp,
        0x0021..=0x007E           // ASCII printable
        | 0x00A1..=0x00FF         // Latin-1 Supplement
        | 0x0100..=0x017F         // Latin Extended-A
        | 0x0370..=0x03FF         // Greek and Coptic (full range)
        | 0x2010..=0x2027         // General punctuation (hyphens, dashes, quotes)
        | 0x2030..=0x205E         // per-mille, primes, overline, dagger
        | 0x2070..=0x209F         // super/subscripts
        | 0x20D0..=0x20FF         // combining diacriticals for symbols
        | 0x2100..=0x214F         // Letterlike symbols (ℂ ℕ ℝ ℤ ℙ ℚ ℍ ℓ ℜ ℑ …)
        | 0x2190..=0x21FF         // arrows
        | 0x2200..=0x22FF         // math operators (full)
        | 0x2300..=0x23FF         // misc technical (⌈ ⌉ ⌊ ⌋ ⌀)
        | 0x25A0..=0x25FF         // geometric shapes (▪ ■ □ ◦ ● ○ …)
        | 0x2600..=0x26FF         // misc symbols (a few used in math)
        | 0x2700..=0x27BF         // dingbats (✓ ✗ ☆ ★)
        | 0x27C0..=0x27FF         // misc math A (⟨ ⟩ ⟦ ⟧ ⟪ ⟫)
        | 0x2900..=0x29FF         // supplemental arrows / math operators
        | 0x2A00..=0x2AFF         // supplemental math operators
        | 0x1D400..=0x1D7FF       // math alphanumeric symbols (𝐀 𝒜 𝔸 𝔄 𝕬 …)
        | 0xFB00..=0xFB06         // ff/fi/fl/ffi/ffl ligatures
    )
}

/// Sizes at which to rasterise each reference glyph. Covers the range of
/// CharProc pixel dimensions seen in dvips-era bitmaps (roughly 13-164px
/// tall). Small size helps match coarse/compact CharProcs; large size
/// helps match display-size CharProcs.
const REFERENCE_SIZES_PX: &[f32] = &[32.0, 64.0, 96.0];

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

                // Rasterise at each reference size; push an entry per
                // distinct signature. The matcher picks the best match
                // across all sizes.
                let mut last_sig: Option<Signature> = None;
                for &px in REFERENCE_SIZES_PX {
                    let (metrics, bitmap_gray) = font.rasterize(ch, px);
                    if metrics.width == 0 || metrics.height == 0 {
                        continue;
                    }
                    let bm = gray_to_bitmap(
                        &bitmap_gray,
                        metrics.width as u32,
                        metrics.height as u32,
                    );
                    let Some((sig, aspect)) = signature(&bm) else {
                        continue;
                    };
                    // Dedup: small and large rasterisations of the same
                    // glyph often hash identically after normalisation.
                    if last_sig.as_ref() == Some(&sig) {
                        continue;
                    }
                    last_sig = Some(sig);
                    entries.push(ReferenceEntry {
                        sig,
                        unicode: ch.to_string(),
                        aspect,
                    });
                    glyph_count += 1;
                }
            });
        }
        debug!("reference pool: {} entries from {}", glyph_count, name);
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
    let (target_sig, target_aspect) = signature(bm)?;

    let mut best: Option<(&ReferenceEntry, u32)> = None;
    let mut runner_up: Option<(&ReferenceEntry, u32)> = None;
    for entry in reference {
        let aspect_diff = (entry.aspect as i32 - target_aspect as i32).abs();
        if aspect_diff > ASPECT_TOLERANCE {
            continue;
        }
        let d = hamming(&entry.sig, &target_sig);
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

    /// Distance-distribution diagnostic for CADE1996.pdf. Prints only a
    /// numeric histogram of Hamming distances and runner-up gaps across
    /// every extracted Type3 CharProc — no per-glyph characters. Use the
    /// output to calibrate ACCEPTABLE_HAMMING.
    ///
    /// Run with: `cargo test type3_matcher::tests::cade1996_distance_histogram -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn cade1996_distance_histogram() {
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

        // One histogram per Type3 font, plus a combined one.
        let mut per_font: Vec<(String, Vec<u32>, Vec<u32>)> = Vec::new();
        const BUCKETS: usize = 11; // 0-49, 50-99, …, 450-499, 500+
        const BUCKET_SIZE: u32 = 50;

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
            let mut dist_hist = vec![0u32; BUCKETS];
            let mut gap_hist = vec![0u32; BUCKETS];
            for bm in bitmaps.values() {
                if let Some(m) = best_match(bm) {
                    let db = (m.distance / BUCKET_SIZE).min(BUCKETS as u32 - 1) as usize;
                    dist_hist[db] += 1;
                    if m.runner_up_distance != u32::MAX {
                        let gap = m.runner_up_distance.saturating_sub(m.distance);
                        let gb = (gap / BUCKET_SIZE).min(BUCKETS as u32 - 1) as usize;
                        gap_hist[gb] += 1;
                    }
                }
            }
            per_font.push((font_name, dist_hist, gap_hist));
        }

        // Print only numeric summaries — safe.
        eprintln!("CADE1996 distance histogram (per font):");
        for (name, dist, gap) in &per_font {
            let total: u32 = dist.iter().sum();
            eprint!("  {:>3} n={:>4} dist:", name, total);
            for (i, c) in dist.iter().enumerate() {
                let lo = i as u32 * BUCKET_SIZE;
                eprint!(" [{}:{}]", lo, c);
            }
            eprintln!();
            eprint!("           gap :");
            for (i, c) in gap.iter().enumerate() {
                let lo = i as u32 * BUCKET_SIZE;
                eprint!(" [{}:{}]", lo, c);
            }
            eprintln!();
        }

        // Totals.
        let mut total_dist = vec![0u32; BUCKETS];
        let mut total_gap = vec![0u32; BUCKETS];
        for (_, d, g) in &per_font {
            for i in 0..BUCKETS {
                total_dist[i] += d[i];
                total_gap[i] += g[i];
            }
        }
        let grand_total: u32 = total_dist.iter().sum();
        eprint!("  ALL n={:>4} dist:", grand_total);
        for (i, c) in total_dist.iter().enumerate() {
            let lo = i as u32 * BUCKET_SIZE;
            eprint!(" [{}:{}]", lo, c);
        }
        eprintln!();
        eprint!("           gap :");
        for (i, c) in total_gap.iter().enumerate() {
            let lo = i as u32 * BUCKET_SIZE;
            eprint!(" [{}:{}]", lo, c);
        }
        eprintln!();
        eprintln!(
            "(buckets are 50 bits wide out of {} total signature bits)",
            NORM_SIZE * NORM_SIZE
        );
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
        // Character-class sanity histogram for matched output. "mod-diac"
        // tracks orphaned combining marks left by diacritical merging when
        // the accent glyph matched but its neighbouring base letter did
        // not; not a false-positive signal.
        let (
            mut ascii,
            mut latin_ext,
            mut mod_diacritic,
            mut greek,
            mut math,
            mut space_punct,
            mut other,
        ) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
        for c in text.chars() {
            let cp = c as u32;
            if c == '\u{FFFD}' {
                continue;
            }
            if c.is_whitespace() {
                space_punct += 1;
            } else if (0x20..=0x7E).contains(&cp) {
                if c.is_ascii_alphanumeric() {
                    ascii += 1;
                } else {
                    space_punct += 1;
                }
            } else if (0x00A0..=0x017F).contains(&cp) {
                latin_ext += 1;
            } else if (0x0250..=0x036F).contains(&cp) {
                mod_diacritic += 1;
            } else if (0x0370..=0x03FF).contains(&cp) || (0x1F00..=0x1FFF).contains(&cp) {
                greek += 1;
            } else if (0x2000..=0x2BFF).contains(&cp) || (0x1D400..=0x1D7FF).contains(&cp) {
                math += 1;
            } else {
                other += 1;
            }
        }
        eprintln!(
            "CADE1996: {} chars total, {} matched, {} U+FFFD ({}% match)",
            total,
            matched,
            replacements,
            if total > 0 { matched * 100 / total } else { 0 }
        );
        eprintln!(
            "  classes: ascii-alnum={} latin-ext={} mod-diac={} greek={} math={} space/punct={} other={}",
            ascii, latin_ext, mod_diacritic, greek, math, space_punct, other
        );
        assert!(total > 500, "expected at least 500 extracted chars");
        // Calibrated floor. Landed ~50% after expanding the reference pool
        // (adding LM Math/Sans/Mono/Slanted), adding aspect-ratio bucketing
        // to the signature, rasterising references at three sizes, and
        // raising ACCEPTABLE_HAMMING to 260 based on the CADE1996 distance
        // histogram. Floor is 40% — a regression below that signals the
        // matcher pipeline broke, not just "a new test glyph is hard".
        assert!(
            matched * 5 > total * 2,
            "expected > 40% match rate; got {}/{}",
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
