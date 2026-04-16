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

use std::collections::HashSet;

use crate::error::OutputError;
use crate::ps::encodings::ot1_to_unicode;
use crate::ps::tokeniser::{Token, Tokeniser};
use crate::types::{BoundingBox, TextSpan};

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

    // Pass 1: register font names. A dvips font definition begins with
    // `/Foo <first> <last> df`. We accept any `/Name` followed by two
    // numbers and then the bare word `df` as registering that font.
    let mut fonts: HashSet<String> = HashSet::new();
    {
        let mut t = Tokeniser::new(src);
        let mut pending_name: Option<String> = None;
        let mut pending_nums: u8 = 0;
        while let Some(tk) = t.next() {
            match tk {
                Token::Name(n) => {
                    pending_name = Some(n.to_string());
                    pending_nums = 0;
                }
                Token::Number(_) if pending_name.is_some() => {
                    pending_nums += 1;
                }
                Token::Word("df") | Token::Word("dfs") if pending_nums >= 2 => {
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
        }
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
    // Line break threshold in dvips units (300dpi → 72pt ≈ 1/300 inch per unit).
    // Paragraph line-spacing at 10pt is ~12pt = 50 units; use 15 as a
    // conservative break threshold.
    const LINE_BREAK_Y: f32 = 15.0;

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
                // Drain pending string into current font.
                if let (Some(bytes), Some(fname)) =
                    (pending_string.take(), current_font.as_deref())
                {
                    emit_string(
                        &bytes,
                        fname,
                        last_x,
                        last_y,
                        page_num,
                        &mut current_line,
                    );
                }
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
                }
                last_numbers.clear();
            }
            Token::Word("a") => {
                // Absolute moveto: consumes the two most recent numbers as x, y.
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

fn emit_string(
    bytes: &[u8],
    font: &str,
    x: f32,
    y: f32,
    page_num: u32,
    line: &mut Vec<TextSpan>,
) {
    let mut text = String::new();
    for &b in bytes {
        text.push_str(ot1_to_unicode(b));
    }
    if text.is_empty() {
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
        // Spans within a line are joined without separators — dvips
        // emits kerning-split words (Fx(W)-6 b(alther)) as multiple
        // spans on the same line, and separating them with spaces
        // would break word-level substring matching.
        let mut all_text = String::new();
        for line in &lines {
            for span in line {
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
        let mut pairs: Vec<(u8, u32)> =
            (0..26u8).map(|i| (i, counts[i as usize])).collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1));
        eprintln!("CADE1996.ps: {} lines, {} chars", total_lines, total_chars);
        eprint!("  top letters:");
        for (i, n) in pairs.iter().take(10) {
            eprint!(" {}:{}", (b'a' + i) as char, n);
        }
        eprintln!();

        // Substring presence — presence-only, no extracted text bodies.
        let substrings: &[&str] = &[
            "Walther",
            "Recursion",
            "Introduction",
            "McAllester",
            "primitive",
            "termination",
        ];
        eprint!("  substrings:");
        for s in substrings {
            eprint!(" {}={}", s, if all_text.contains(*s) { 1 } else { 0 });
        }
        eprintln!();

        // Assertions: English-letter plausibility.
        assert!(total_chars > 10_000, "expected >10k chars; got {}", total_chars);
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
            assert!(
                all_text.contains(*s),
                "expected '{}' in extracted text",
                s
            );
        }
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
