//! Extract bitmap glyphs from dvips-generated Type3 fonts.
//!
//! dvips/ghostscript emits Type3 CharProcs in a highly constrained form:
//!
//! ```text
//! <wx> <wy> <llx> <lly> <urx> <ury> d1
//! <a> <b> <c> <d> <e> <f> cm
//! BI
//! /IM true /W <w> /H <h> /BPC 1 /F /CCF /DP <<...>>
//! ID
//! <CCITT Group 4 compressed bitmap bytes>
//! EI
//! ```
//!
//! We parse that subset and decode the G4 bitstream into a 1-bpp raster.

use std::collections::HashMap;

use lopdf::{Dictionary, Document};
use tracing::{debug, warn};

use crate::fonts::CharCode;
use crate::utils::{get_contents, maybe_deref};

/// A decoded bitmap glyph. 1 bit per pixel, row-major, MSB-first, with
/// each row padded to a byte boundary. 1 = ink, 0 = background (matching
/// PDF imagemask semantics with `/Decode [0 1]`).
#[derive(Clone, Debug)]
pub(crate) struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Bitmap {
    /// Test whether pixel (x, y) is set (ink).
    pub fn get(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let row_stride = self.width.div_ceil(8);
        let byte = self.pixels[(y * row_stride + x / 8) as usize];
        (byte >> (7 - (x % 8))) & 1 == 1
    }

    /// Ink bounding box: (min_x, min_y, max_x+1, max_y+1). Returns None if empty.
    pub fn ink_bbox(&self) -> Option<(u32, u32, u32, u32)> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        let mut any = false;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get(x, y) {
                    any = true;
                    if x < x0 {
                        x0 = x;
                    }
                    if y < y0 {
                        y0 = y;
                    }
                    if x + 1 > x1 {
                        x1 = x + 1;
                    }
                    if y + 1 > y1 {
                        y1 = y + 1;
                    }
                }
            }
        }
        if any { Some((x0, y0, x1, y1)) } else { None }
    }

    /// Resample to `out_w × out_h` via nearest-source sampling. Returns a
    /// packed 1-bpp bitmap the same shape as `Self`. The input region
    /// `(src_x0..src_x1, src_y0..src_y1)` is mapped onto the full output.
    pub fn resample(
        &self,
        src_x0: u32,
        src_y0: u32,
        src_x1: u32,
        src_y1: u32,
        out_w: u32,
        out_h: u32,
    ) -> Bitmap {
        let sw = (src_x1 - src_x0).max(1);
        let sh = (src_y1 - src_y0).max(1);
        let row_stride = out_w.div_ceil(8);
        let mut pixels = vec![0u8; (row_stride * out_h) as usize];
        for oy in 0..out_h {
            for ox in 0..out_w {
                let sx = src_x0 + (ox * sw) / out_w;
                let sy = src_y0 + (oy * sh) / out_h;
                if self.get(sx, sy) {
                    let i = (oy * row_stride + ox / 8) as usize;
                    pixels[i] |= 1 << (7 - (ox % 8));
                }
            }
        }
        Bitmap {
            width: out_w,
            height: out_h,
            pixels,
        }
    }
}

/// Extract all glyph bitmaps from a Type3 font's /CharProcs dictionary.
///
/// `differences_by_code` maps CharCode → glyph name (from the font's
/// /Encoding /Differences array). Glyphs whose CharProc does not match
/// the dvips bitmap pattern are silently omitted; the caller treats a
/// missing entry as "unrecognised" (→ U+FFFD upstream).
pub(crate) fn extract_bitmaps(
    doc: &Document,
    font: &Dictionary,
    differences_by_code: &HashMap<CharCode, String>,
) -> HashMap<CharCode, Bitmap> {
    let mut out = HashMap::new();
    let Ok(char_procs_obj) = font.get(b"CharProcs") else {
        return out;
    };
    let char_procs = match maybe_deref(doc, char_procs_obj).as_dict() {
        Ok(d) => d,
        Err(_) => return out,
    };

    for (code, glyph_name) in differences_by_code {
        let Ok(proc_obj) = char_procs.get(glyph_name.as_bytes()) else {
            continue;
        };
        let proc_obj = maybe_deref(doc, proc_obj);
        let Ok(stream) = proc_obj.as_stream() else {
            continue;
        };
        let content = get_contents(stream);
        match parse_charproc(&content) {
            Some(bitmap) => {
                out.insert(*code, bitmap);
            }
            None => {
                debug!(
                    "CharProc /{} (code {}) did not match dvips bitmap pattern",
                    glyph_name, code
                );
            }
        }
    }
    out
}

/// Parse a single CharProc content stream and return its rasterised bitmap.
///
/// Returns None for anything that isn't the narrow dvips pattern (d0/d1 +
/// cm + a single BI…EI inline image with F/CCF G4 compression).
fn parse_charproc(content: &[u8]) -> Option<Bitmap> {
    // Locate the inline image region.
    let bi_pos = find_token(content, b"BI", 0)?;
    let id_pos = find_token(content, b"ID", bi_pos + 2)?;
    let ei_pos = find_token_backwards(content, b"EI")?;
    if ei_pos <= id_pos {
        return None;
    }

    let dict_slice = &content[bi_pos + 2..id_pos];
    let (width, height, filter) = parse_inline_dict(dict_slice)?;

    // Data lies between ID<sp> and EI. Per PDF 1.7 §8.9.7, exactly one
    // whitespace byte follows ID; we skip any run of whitespace to be safe.
    let mut data_start = id_pos + 2;
    while data_start < ei_pos && is_whitespace(content[data_start]) {
        data_start += 1;
    }
    // Strip trailing whitespace before EI.
    let mut data_end = ei_pos;
    while data_end > data_start && is_whitespace(content[data_end - 1]) {
        data_end -= 1;
    }
    let image_data = &content[data_start..data_end];

    match filter {
        Filter::Ccitt => decode_g4_bitmap(image_data, width, height),
        Filter::None => decode_raw_bitmap(image_data, width, height),
    }
}

/// Find `token` as a whole word (surrounded by whitespace or start/end of input).
fn find_token(haystack: &[u8], token: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + token.len() <= haystack.len() {
        if &haystack[i..i + token.len()] == token {
            let before_ok = i == 0 || is_whitespace(haystack[i - 1]);
            let after_idx = i + token.len();
            let after_ok = after_idx == haystack.len() || is_whitespace(haystack[after_idx]);
            if before_ok && after_ok {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn find_token_backwards(haystack: &[u8], token: &[u8]) -> Option<usize> {
    if haystack.len() < token.len() {
        return None;
    }
    let mut i = haystack.len() - token.len();
    loop {
        if &haystack[i..i + token.len()] == token {
            let before_ok = i == 0 || is_whitespace(haystack[i - 1]);
            let after_idx = i + token.len();
            let after_ok = after_idx == haystack.len() || is_whitespace(haystack[after_idx]);
            if before_ok && after_ok {
                return Some(i);
            }
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

fn is_whitespace(b: u8) -> bool {
    matches!(b, b'\0' | b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

#[derive(Copy, Clone, Debug)]
enum Filter {
    Ccitt,
    None,
}

/// Parse the inline image dict between BI and ID. Only extracts /W, /H, and /F.
///
/// The dvips pattern is tightly constrained; we scan for `/W`, `/H`, `/F`
/// and their values without building a full PDF object parser. Nested
/// dicts (e.g. `/DP<<...>>`) are tolerated by bracket counting.
fn parse_inline_dict(slice: &[u8]) -> Option<(u32, u32, Filter)> {
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut filter: Option<Filter> = None;

    let mut i = 0;
    while i < slice.len() {
        let b = slice[i];
        if b == b'<' && i + 1 < slice.len() && slice[i + 1] == b'<' {
            // Nested dict — skip to matching >>.
            i = skip_nested_dict(slice, i + 2)?;
            continue;
        }
        if b != b'/' {
            i += 1;
            continue;
        }
        // Read name.
        let name_start = i + 1;
        let mut name_end = name_start;
        while name_end < slice.len() && !is_name_terminator(slice[name_end]) {
            name_end += 1;
        }
        let name = &slice[name_start..name_end];
        i = name_end;

        // Skip whitespace to value.
        while i < slice.len() && is_whitespace(slice[i]) {
            i += 1;
        }

        match name {
            b"W" | b"Width" => {
                width = read_number(slice, &mut i);
            }
            b"H" | b"Height" => {
                height = read_number(slice, &mut i);
            }
            b"F" | b"Filter" => {
                // Value is a name (single-filter form). Arrays not expected in dvips output.
                if i < slice.len() && slice[i] == b'/' {
                    let v_start = i + 1;
                    let mut v_end = v_start;
                    while v_end < slice.len() && !is_name_terminator(slice[v_end]) {
                        v_end += 1;
                    }
                    let fname = &slice[v_start..v_end];
                    filter = Some(match fname {
                        b"CCF" | b"CCITTFaxDecode" => Filter::Ccitt,
                        _ => return None,
                    });
                    i = v_end;
                }
            }
            _ => {
                // Skip value: another name, number, bool, or nested dict.
                if i < slice.len() && slice[i] == b'<' && i + 1 < slice.len() && slice[i + 1] == b'<'
                {
                    i = skip_nested_dict(slice, i + 2)?;
                }
                // Otherwise, loop will resume and next iteration will handle it.
            }
        }
    }

    Some((width?, height?, filter.unwrap_or(Filter::None)))
}

fn is_name_terminator(b: u8) -> bool {
    is_whitespace(b) || matches!(b, b'/' | b'<' | b'>' | b'[' | b']' | b'(' | b')')
}

fn read_number(slice: &[u8], i: &mut usize) -> Option<u32> {
    let start = *i;
    while *i < slice.len() && (slice[*i].is_ascii_digit() || slice[*i] == b'-') {
        *i += 1;
    }
    if *i == start {
        return None;
    }
    std::str::from_utf8(&slice[start..*i]).ok()?.parse().ok()
}

fn skip_nested_dict(slice: &[u8], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = start;
    while i + 1 < slice.len() {
        if slice[i] == b'<' && slice[i + 1] == b'<' {
            depth += 1;
            i += 2;
        } else if slice[i] == b'>' && slice[i + 1] == b'>' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return Some(i);
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Decode a CCITT Group 4 bitstream into a packed 1-bpp bitmap.
fn decode_g4_bitmap(data: &[u8], width: u32, height: u32) -> Option<Bitmap> {
    use fax::Color;
    use fax::decoder;

    let row_stride = (width as usize).div_ceil(8);
    let mut pixels = vec![0u8; row_stride * height as usize];
    let mut row_index = 0usize;

    let ok = decoder::decode_g4(data.iter().copied(), width as u16, Some(height as u16), |line| {
        if row_index >= height as usize {
            return;
        }
        let row_off = row_index * row_stride;
        let mut x = 0u32;
        for color in decoder::pels(line, width as u16) {
            if x >= width {
                break;
            }
            if matches!(color, Color::Black) {
                pixels[row_off + (x / 8) as usize] |= 1 << (7 - (x % 8));
            }
            x += 1;
        }
        row_index += 1;
    });
    if ok.is_none() {
        warn!("CCITT G4 decode failed for {}x{} glyph", width, height);
        return None;
    }
    Some(Bitmap {
        width,
        height,
        pixels,
    })
}

/// Decode an unfiltered 1-bpp inline image (no filter applied).
fn decode_raw_bitmap(data: &[u8], width: u32, height: u32) -> Option<Bitmap> {
    let row_stride = (width as usize).div_ceil(8);
    let expected = row_stride * height as usize;
    if data.len() < expected {
        return None;
    }
    Some(Bitmap {
        width,
        height,
        pixels: data[..expected].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_get_and_ink_bbox() {
        // 8x2, second row: 0b11110000
        let bm = Bitmap {
            width: 8,
            height: 2,
            pixels: vec![0b00000000, 0b11110000],
        };
        assert!(!bm.get(0, 0));
        assert!(bm.get(0, 1));
        assert!(bm.get(3, 1));
        assert!(!bm.get(4, 1));
        assert_eq!(bm.ink_bbox(), Some((0, 1, 4, 2)));
    }

    #[test]
    fn parse_dvips_header_only() {
        // Minimal CharProc header + empty G4 stream. We just want the dict
        // + offsets parsing to work; a synthetic empty stream will fail G4
        // decoding, which should surface as None.
        let content = b"0 0 0 0 4 4 d1\n4 0 0 4 0 0 cm\nBI\n/IM true/W 4/H 4/BPC 1/F/CCF/DP<</K -1/Columns 4>>\nID \nEI";
        let result = parse_charproc(content);
        // No body bytes → decode fails → None.
        assert!(result.is_none());
    }

    /// End-to-end: decode the first Type3 font from CADE1996.pdf and print
    /// a few decoded glyphs as ASCII art. Gated on the test PDF existing —
    /// otherwise skipped. Run with: cargo test type3_bitmap::tests::decode_real_charprocs -- --ignored --nocapture
    #[test]
    #[ignore]
    fn decode_real_charprocs() {
        use lopdf::Object;
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

            // Build a code → name map from the font's encoding /Differences.
            let mut code_to_name: HashMap<CharCode, String> = HashMap::new();
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
                                        code as CharCode,
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
            eprintln!(
                "Font {}: {} CharProcs decoded (of {})",
                font_name,
                bitmaps.len(),
                code_to_name.len()
            );

            // Print ASCII art for a few interesting glyphs.
            let mut codes: Vec<_> = bitmaps.keys().copied().collect();
            codes.sort();
            for code in codes.iter().take(6) {
                let bm = &bitmaps[code];
                eprintln!(
                    "  code {} /{} → {}x{}",
                    code, code_to_name[code], bm.width, bm.height
                );
                print_ascii(bm);
            }
            return;
        }
        panic!("no Type3 font found in {}", pdf_path);
    }

    #[cfg(test)]
    fn print_ascii(bm: &Bitmap) {
        for y in 0..bm.height {
            let mut line = String::new();
            for x in 0..bm.width {
                line.push(if bm.get(x, y) { '#' } else { '.' });
            }
            eprintln!("    {}", line);
        }
        eprintln!();
    }
}
