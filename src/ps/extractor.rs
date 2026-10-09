//! Extract text from dvips-emitted PostScript.
//!
//! The dvips output format is stylised: the `TeXDict` procset at the top
//! defines short macros for font management and text output, the prolog
//! defines bitmap fonts (`/Fa … df<bitmap>code D … E`) without touching
//! the page surface, and each page is bracketed by `bop … eop` with a
//! sequence of font selectors (`Fa`, `Fb`, …), absolute move-tos (`a`),
//! and print operators (`p`, `l`, `m`, `n`, `o`, `q`, `r`, `s`, `t`,
//! `y`, `M`, `b`, `c`, `d`, `e`, `f`, `g`, `h`, `i`, `j`, `k`) that each
//! consume a preceding literal string.
//!
//! We do not execute PostScript — we only lift the text off by scanning
//! tokens linearly.

use std::collections::{HashMap, HashSet};

use crate::error::OutputError;
use crate::ps::encodings::{Encoding, decode_byte};
use crate::ps::tokeniser::{Token, Tokeniser};
use crate::types::{BoundingBox, TextSpan};

/// Classify a font's TeX encoding from its per-byte emission histogram.
/// OT1 (text fonts): emissions concentrated in 0x20-0x7E, with
/// occasional ligatures at 0x0B-0x0F. OML (CMMI math italic): heavy use
/// of 0x0B-0x29 (Greek lowercase) plus italic letters at ASCII
/// positions. OMS (CMSY math symbols): heavy use of 0x00-0x3F for
/// operators / relations / arrows.
fn detect_encoding(histogram: &[u32; 128]) -> Encoding {
    let total: u32 = histogram.iter().sum();
    if total == 0 {
        return Encoding::Ot1;
    }
    // Bytes in 0x00..=0x1F that aren't OT1 ligatures (0x0B..=0x0F)
    // are a strong math-font signal — OT1 text emits these almost
    // never. Threshold: 10% of emissions.
    let mut low_nonlig: u32 = 0;
    for (i, c) in histogram.iter().enumerate() {
        if i < 0x20 && !(0x0B..=0x0F).contains(&i) {
            low_nonlig += c;
        }
    }
    if low_nonlig * 100 / total < 10 {
        return Encoding::Ot1;
    }
    // Math font. Distinguish CMMI (italic letters present) from CMSY
    // (relations/operators) by letter density: if >30% of emissions
    // are in 0x41..=0x5A or 0x61..=0x7A, treat as OML; else OMS.
    let mut letters: u32 = 0;
    for (i, c) in histogram.iter().enumerate() {
        if (0x41..=0x5A).contains(&i) || (0x61..=0x7A).contains(&i) {
            letters += c;
        }
    }
    if letters * 100 / total > 30 {
        Encoding::Oml
    } else {
        Encoding::Oms
    }
}

/// dvips text-emitting operators. Every one of these consumes a single
/// literal-string argument and emits characters from the current font.
fn is_show_op(w: &str) -> bool {
    matches!(
        w,
        "p" | "l"
            | "m"
            | "n"
            | "o"
            | "q"
            | "r"
            | "s"
            | "t"
            | "y"
            | "M"
            | "b"
            | "c"
            | "d"
            | "e"
            | "f"
            | "g"
            | "h"
            | "i"
            | "j"
            | "k"
            | "show"
    )
}

/// Detect whether the input looks like dvips-emitted PostScript. We refuse
/// anything else — general PostScript has arbitrary font encodings and
/// we'd silently produce garbage.
fn is_dvips(src: &[u8]) -> bool {
    // Look for both signatures in the first 4 KB: the Adobe header and
    // either an explicit "Creator: dvips" comment or the TeXDict
    // procset marker.
    let head = &src[..src.len().min(4096)];
    let as_str = std::str::from_utf8(head).unwrap_or("");
    let adobe = as_str.starts_with("%!PS-Adobe");
    let dvips = as_str.contains("dvips") || as_str.contains("TeXDict");
    adobe && dvips
}

pub(crate) fn extract_lines(src: &[u8]) -> Result<Vec<Vec<TextSpan>>, OutputError> {
    if !is_dvips(src) {
        return Err(OutputError::Other(
            "input is not dvips-emitted PostScript".into(),
        ));
    }

    let mut tok = Tokeniser::new(src);

    // Pass 1: register fonts and build per-font byte histograms so we
    // can classify each font's TeX encoding before emission.
    let mut fonts: HashSet<String> = HashSet::new();
    let mut histograms: HashMap<String, [u32; 128]> = HashMap::new();
    {
        let mut t = Tokeniser::new(src);
        let mut pending_name: Option<String> = None;
        let mut pending_nums: u8 = 0;
        // State for tracking byte usage within bop..eop.
        let mut p1_in_page = false;
        let mut p1_font: Option<String> = None;
        let mut p1_pending_str: Option<Vec<u8>> = None;
        while let Some(tk) = t.next() {
            // Font-definition tracking.
            match &tk {
                Token::Name(n) => {
                    pending_name = Some(n.to_string());
                    pending_nums = 0;
                }
                Token::Number(_) if pending_name.is_some() => {
                    pending_nums += 1;
                }
                Token::Word(w) if (*w == "df" || *w == "dfs") && pending_nums >= 2 => {
                    if let Some(name) = pending_name.take() {
                        fonts.insert(name);
                    }
                    pending_nums = 0;
                }
                Token::Word(_) => {
                    pending_nums = 0;
                    pending_name = None;
                }
                _ => {}
            }
            // Content-histogram tracking.
            match tk {
                Token::Word("bop") => p1_in_page = true,
                Token::Word("eop") => {
                    p1_in_page = false;
                    p1_pending_str = None;
                }
                _ if !p1_in_page => {}
                Token::LitString(bytes) => p1_pending_str = Some(bytes),
                Token::Word(w) if fonts.contains(w) => p1_font = Some(w.to_string()),
                Token::Word(w) if is_show_op(w) => {
                    if let (Some(bytes), Some(fname)) = (p1_pending_str.take(), p1_font.as_deref())
                    {
                        let hist = histograms
                            .entry(fname.to_string())
                            .or_insert_with(|| [0u32; 128]);
                        for &b in &bytes {
                            if (b as usize) < 128 {
                                hist[b as usize] += 1;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Classify encoding per font.
    let mut encodings: HashMap<String, Encoding> = HashMap::new();
    for (name, hist) in &histograms {
        encodings.insert(name.clone(), detect_encoding(hist));
    }

    // Pass 2: walk page content and collect (font, byte) emissions with
    // cursor position. We break into lines by y-coordinate jumps.
    let mut lines: Vec<Vec<TextSpan>> = Vec::new();
    let mut current_line: Vec<TextSpan> = Vec::new();
    let mut current_font: Option<String> = None;
    let mut in_page = false;
    let mut page_num: u32 = 0;
    let mut last_x = 0.0f32;
    let mut last_y = 0.0f32;
    let mut pending_string: Option<Vec<u8>> = None;
    let mut last_numbers: Vec<f64> = Vec::new();
    // Kern applied by the PREVIOUS show operator, in dvips units. Compared
    // against WORD_SPACE_KERN to decide whether to insert a synthetic
    // space before the next emission. Zero when the next span follows
    // an absolute moveto (`a` / `y`) rather than an in-line kern.
    let mut prev_kern: f32 = 0.0;
    // dvips `delta` register. Set by `b` (delta=arg), `M` (delta+=arg),
    // `c`-`k` (delta+=const_for_op) via the `tail` macro. Read by `M`
    // and `c`-`k` to compute the final kern. `p`, `show`, `l`-`t` do
    // not touch delta. Absolute moveto resets it.
    let mut delta: f32 = 0.0;
    // Line break threshold in dvips units (300dpi → 72pt ≈ 1/300 inch per unit).
    // Paragraph line-spacing at 10pt is ~12pt = 50 units; use 15 as a
    // conservative break threshold.
    const LINE_BREAK_Y: f32 = 15.0;
    // Word-space kerns in dvips CM-based output: natural 14 units (CMR10
    // space at 300dpi), shrinkable to ~10 in tight justification,
    // stretchable to 20+. CMR letter-kern pairs are overwhelmingly
    // negative (tightening, e.g. "AV" = -18) or at most +3. Setting the
    // threshold at 6 cleanly separates: letter kerns (-20..+3) merge
    // into the preceding span; word-space kerns (10..20+) start a new
    // span. The rare positive letter kerns (3-5 units) get merged too;
    // intra-word letter kerns from l-t (always -4..+4) cannot trigger
    // a split.
    const WORD_SPACE_KERN: f32 = 6.0;

    loop {
        let Some(tk) = tok.next() else { break };
        match tk {
            Token::Word("bop") => {
                flush_line(&mut lines, &mut current_line);
                // bop consumes two numbers (page_num and prev_page_num);
                // the first one is the 1-based page number.
                if last_numbers.len() >= 2 {
                    page_num = last_numbers[last_numbers.len() - 2] as u32;
                } else {
                    page_num += 1;
                }
                in_page = true;
                // Conservatively reset the kerning register per page.
                // dvips' procset does not require this, but any real
                // page's first text emission will re-establish delta
                // via a `b` or `M`.
                prev_kern = 0.0;
                delta = 0.0;
                last_numbers.clear();
                pending_string = None;
            }
            Token::Word("eop") => {
                flush_line(&mut lines, &mut current_line);
                in_page = false;
                pending_string = None;
            }
            _ if !in_page => {
                // Outside bop/eop: skip everything (procset, fonts, setup).
            }
            Token::Number(n) => {
                last_numbers.push(n);
                if last_numbers.len() > 8 {
                    last_numbers.remove(0);
                }
            }
            Token::LitString(bytes) => {
                pending_string = Some(bytes);
                // strings don't count as numbers; leave last_numbers alone
            }
            Token::Word(w) if is_show_op(w) => {
                // Drain pending string into current font. Merge into the
                // previous span on the line when the prior kern falls in
                // the letter-kerning band (< WORD_SPACE_KERN); otherwise
                // start a fresh TextSpan. TextOutput::Display puts one
                // space between adjacent spans on a line, so a merged
                // word renders with no internal space and a word boundary
                // gets exactly one space.
                if let (Some(bytes), Some(fname)) = (pending_string.take(), current_font.as_deref())
                {
                    let merge =
                        prev_kern < WORD_SPACE_KERN && last_span_can_merge(&current_line, fname);
                    let enc = encodings.get(fname).copied().unwrap_or(Encoding::Ot1);
                    emit_string(
                        &bytes,
                        fname,
                        enc,
                        last_x,
                        last_y,
                        page_num,
                        merge,
                        &mut current_line,
                    );
                }
                // Compute this op's kern (applied AFTER the show) and
                // update the dvips `delta` register. Some kerning ops
                // (c-k, M) produce kerns as arg + delta and also set
                // delta = arg + delta — this state is what carries
                // word-space-sized kerns through runs of letter-level
                // show operators.
                let arg = last_numbers.last().copied().unwrap_or(0.0) as f32;
                let (k, new_delta) = kern_for_op(w, arg, delta);
                prev_kern = k;
                delta = new_delta;

                // `y` is "show then absolute-moveto" — the two preceding
                // numbers are the new position. Other show operators are
                // print-and-kern; numbers before them belong to earlier
                // operators.
                if w == "y" && last_numbers.len() >= 2 {
                    let ny = last_numbers[last_numbers.len() - 1] as f32;
                    let nx = last_numbers[last_numbers.len() - 2] as f32;
                    if (ny - last_y).abs() > LINE_BREAK_Y && !current_line.is_empty() {
                        flush_line(&mut lines, &mut current_line);
                    }
                    last_x = nx;
                    last_y = ny;
                    // `y`'s tail (`a`) doesn't touch delta in dvips; leave
                    // delta alone. Only reset prev_kern so the first
                    // emission after the moveto isn't merged by accident.
                    prev_kern = 0.0;
                }
                last_numbers.clear();
            }
            Token::Word("a") => {
                // Absolute moveto: consumes the two most recent numbers as x, y.
                // dvips' `/a{moveto}B` does NOT touch `delta`; TeX relies on
                // delta being carried across movetos so that subsequent
                // kerns from c-k / M still produce full word-width gaps.
                if last_numbers.len() >= 2 {
                    let y = last_numbers[last_numbers.len() - 1] as f32;
                    let x = last_numbers[last_numbers.len() - 2] as f32;
                    // Detect line break by y jump (dvips y grows downward).
                    if (y - last_y).abs() > LINE_BREAK_Y && !current_line.is_empty() {
                        flush_line(&mut lines, &mut current_line);
                    }
                    last_x = x;
                    last_y = y;
                }
                prev_kern = 0.0;
                last_numbers.clear();
            }
            Token::Word(w) if fonts.contains(w) => {
                current_font = Some(w.to_string());
                last_numbers.clear();
            }
            Token::Word(_) => {
                // Unknown operator: reset any pending string and the
                // number buffer so we don't accidentally attribute numbers
                // to the wrong operator.
                pending_string = None;
                last_numbers.clear();
            }
            _ => {}
        }
    }
    flush_line(&mut lines, &mut current_line);

    Ok(lines)
}

/// Kern in dvips units applied AFTER a show operator, and the new value
/// of the `delta` register. Used to decide whether to insert a
/// synthetic space before the NEXT emission.
///
/// Operator semantics from the dvips procset:
///   - `p` / `show`      : print, no kern, no delta change
///   - `b`               : print, move by arg, delta = arg
///   - `c`..`k`          : print, move by N+delta, delta = N+delta
///                         (N = -4,-3,-2,-1,0,+1,+2,+3,+4 respectively)
///   - `M`               : print, move by arg+delta, delta = arg+delta
///   - `l`..`t` (not m,n): print, move by N, delta unchanged
///                         (N from -4..-1 and +1..+4)
///   - `y`               : print, then absolute moveto; caller resets
fn kern_for_op(w: &str, arg: f32, delta: f32) -> (f32, f32) {
    match w {
        "p" | "show" => (0.0, delta),
        "b" => (arg, arg),
        "c" => {
            let k = -4.0 + delta;
            (k, k)
        }
        "d" => {
            let k = -3.0 + delta;
            (k, k)
        }
        "e" => {
            let k = -2.0 + delta;
            (k, k)
        }
        "f" => {
            let k = -1.0 + delta;
            (k, k)
        }
        "g" => (delta, delta),
        "h" => {
            let k = 1.0 + delta;
            (k, k)
        }
        "i" => {
            let k = 2.0 + delta;
            (k, k)
        }
        "j" => {
            let k = 3.0 + delta;
            (k, k)
        }
        "k" => {
            let k = 4.0 + delta;
            (k, k)
        }
        "M" => {
            let k = arg + delta;
            (k, k)
        }
        "l" => (-4.0, delta),
        "m" => (-3.0, delta),
        "n" => (-2.0, delta),
        "o" => (-1.0, delta),
        "q" => (1.0, delta),
        "r" => (2.0, delta),
        "s" => (3.0, delta),
        "t" => (4.0, delta),
        "y" => (0.0, delta),
        _ => (0.0, delta),
    }
}

/// Whether the last span on the current line can have more characters
/// appended — i.e. it exists and uses the same font.
fn last_span_can_merge(line: &[TextSpan], font: &str) -> bool {
    line.last().is_some_and(|s| s.font_name == font)
}

fn emit_string(
    bytes: &[u8],
    font: &str,
    encoding: Encoding,
    x: f32,
    y: f32,
    page_num: u32,
    merge: bool,
    line: &mut Vec<TextSpan>,
) {
    let mut text = String::new();
    for &b in bytes {
        text.push_str(decode_byte(encoding, b));
    }
    if text.is_empty() {
        return;
    }
    if merge {
        // Append to the trailing span; widen its right edge to cover
        // the appended content. Safe by precondition (caller only sets
        // merge=true when last_span_can_merge returned true).
        let last = line
            .last_mut()
            .expect("merge only set when last span exists");
        let extra_width = text.chars().count() as f32 * 17.5;
        last.bbox.r += extra_width;
        last.text.push_str(&text);
        return;
    }
    let char_count = text.chars().count() as f32;
    // Heuristic widths: dvips units are 1/300 inch ≈ 0.24 pt. A typical
    // 10pt letter is ~4.2 pt ≈ 17.5 units wide. Good enough for v1
    // coarse bboxes.
    let approx_char_width = 17.5;
    let span_width = char_count * approx_char_width;
    // dvips y axis grows downward from page top; translate to a bbox
    // where top < bottom. Font height ~30 units for 10pt body.
    let span_height = 30.0;
    let bbox = BoundingBox {
        t: y - span_height,
        r: x + span_width,
        b: y,
        l: x,
    };
    line.push(TextSpan {
        text,
        bbox,
        font_name: font.to_string(),
        font_size: 10.0,
        page_num,
    });
}

fn flush_line(lines: &mut Vec<Vec<TextSpan>>, current: &mut Vec<TextSpan>) {
    if !current.is_empty() {
        // Sort spans left-to-right by x coordinate so output is in
        // reading order regardless of dvips emission quirks.
        current.sort_by(|a, b| {
            a.bbox
                .l
                .partial_cmp(&b.bbox.l)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        lines.push(std::mem::take(current));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_dvips() {
        let src = b"%!PS-Adobe-3.0\n% some other PostScript\n";
        assert!(extract_lines(src).is_err());
    }

    #[test]
    fn accepts_dvips_header() {
        let src = b"%!PS-Adobe-2.0\n%%Creator: dvips 5.497\n";
        // No pages, so output is empty — but parsing must succeed.
        let out = extract_lines(src).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn extracts_simple_page() {
        // Minimal synthetic dvips PS: declare Fa, open page, show a word.
        let src = b"\
%!PS-Adobe-2.0
%%Creator: dvips 5.497
/Fa 1 128 df<00>97 D E
1 0 bop 100 200 a Fa(Hello) p eop
";
        let out = extract_lines(src).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 1);
        assert_eq!(out[0][0].text, "Hello");
    }

    #[test]
    fn handles_octal_escapes_for_ligatures() {
        // \014 (octal 14 = decimal 12) is the fi ligature in OT1.
        let src = b"\
%!PS-Adobe-2.0
%%Creator: dvips 5.497
/Fa 1 128 df<00>97 D E
1 0 bop 100 200 a Fa(spec\\014ed) p eop
";
        let out = extract_lines(src).unwrap();
        assert_eq!(out[0][0].text, "specfied");
    }

    /// End-to-end extraction of CADE1996.ps. Safe output: only numeric
    /// summaries + substring-presence booleans are printed. Run with:
    /// `cargo test ps::extractor::tests::cade1996_ps_extraction -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn cade1996_ps_extraction() {
        let ps_path = std::env::var("CADE1996_PS")
            .unwrap_or_else(|_| std::env::var("HOME").unwrap() + "/Downloads/CADE1996.ps");
        let Ok(bytes) = std::fs::read(&ps_path) else {
            eprintln!("skipped: {} not readable", ps_path);
            return;
        };
        let lines = extract_lines(&bytes).unwrap();
        let total_lines: usize = lines.len();
        // Spans on one line are joined with a single space, matching
        // TextOutput::Display — which is what the CLI actually emits.
        // Word-level merging in the extractor keeps each TextSpan
        // equal to a word or phrase, so this produces natural prose.
        let mut all_text = String::new();
        for line in &lines {
            for (i, span) in line.iter().enumerate() {
                if i > 0 {
                    all_text.push(' ');
                }
                all_text.push_str(&span.text);
            }
            all_text.push('\n');
        }
        let total_chars = all_text.chars().count();

        // Character-class + letter-frequency sanity (numbers only).
        let mut counts = [0u32; 26];
        for c in all_text.chars() {
            let idx = match c {
                'a'..='z' => Some((c as u8 - b'a') as usize),
                'A'..='Z' => Some((c as u8 - b'A') as usize),
                _ => None,
            };
            if let Some(i) = idx {
                counts[i] += 1;
            }
        }
        let mut pairs: Vec<(u8, u32)> = (0..26u8).map(|i| (i, counts[i as usize])).collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1));
        eprintln!("CADE1996.ps: {} lines, {} chars", total_lines, total_chars);
        eprint!("  top letters:");
        for (i, n) in pairs.iter().take(10) {
            eprint!(" {}:{}", (b'a' + i) as char, n);
        }
        eprintln!();

        // Substring presence — presence-only, no extracted text bodies.
        // Includes multi-word phrases to verify both intra-word merging
        // and inter-word spacing work together.
        let substrings: &[&str] = &[
            "Walther Recursion",
            "David McAllester",
            "Kostas Arkoudas",
            "AT&T Labs",
            "MIT Artificial Intelligence",
            "Primitive recursion",
            "Introduction",
            "termination",
            // Math-font content: OMS (CMSY) and OML (CMMI) must be
            // detected and decoded for these to appear.
            "\u{2264}",  // ≤ (from u ≤ v style relations)
            "\u{2200}",  // ∀ (for all)
            "\u{1D465}", // 𝑥 math italic x
        ];
        eprint!("  substrings:");
        for s in substrings {
            eprint!(" {}={}", s, if all_text.contains(*s) { 1 } else { 0 });
        }
        eprintln!();

        // Assertions: English-letter plausibility.
        assert!(
            total_chars > 10_000,
            "expected >10k chars; got {}",
            total_chars
        );
        let e_count = counts[(b'e' - b'a') as usize];
        let t_count = counts[(b't' - b'a') as usize];
        let l_count = counts[(b'l' - b'a') as usize];
        assert!(
            e_count > l_count && e_count > t_count / 2,
            "letter frequencies don't look English: e={} t={} l={}",
            e_count,
            t_count,
            l_count
        );
        for s in substrings {
            assert!(all_text.contains(*s), "expected '{}' in extracted text", s);
        }
    }

    #[test]
    fn merges_letter_kerned_spans() {
        // (W) -6 b (alther) — kern -6 is letter-level, so the two
        // emissions should merge into one TextSpan "Walther".
        let src = b"\
%!PS-Adobe-2.0
%%Creator: dvips 5.497
/Fa 1 128 df<00>97 D E
1 0 bop 100 200 a Fa(W)-6 b(alther) p eop
";
        let out = extract_lines(src).unwrap();
        assert_eq!(out[0].len(), 1);
        assert_eq!(out[0][0].text, "Walther");
    }

    #[test]
    fn splits_on_word_space_kern() {
        // (alther) 22 b (Recursion) — kern 22 is word-space sized, so
        // the two emissions should stay as distinct TextSpans. Display
        // will separate them with a single space.
        let src = b"\
%!PS-Adobe-2.0
%%Creator: dvips 5.497
/Fa 1 128 df<00>97 D E
1 0 bop 100 200 a Fa(alther)22 b(Recursion) p eop
";
        let out = extract_lines(src).unwrap();
        assert_eq!(out[0].len(), 2);
        assert_eq!(out[0][0].text, "alther");
        assert_eq!(out[0][1].text, "Recursion");
    }

    #[test]
    fn breaks_line_on_y_jump() {
        // Two emissions at different y positions.
        let src = b"\
%!PS-Adobe-2.0
%%Creator: dvips 5.497
/Fa 1 128 df<00>97 D E
1 0 bop 100 200 a Fa(hello) p 100 400 a Fa(world) p eop
";
        let out = extract_lines(src).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0][0].text, "hello");
        assert_eq!(out[1][0].text, "world");
    }
}
