use std::collections::{HashMap, hash_map::Entry};
use std::fmt::{self, Debug};
use std::rc::Rc;
use std::slice::Iter;

use adobe_cmap_parser::{ByteMapping, CIDRange, CodeRange};
use lopdf::{Dictionary, Document, Object};
use tracing::{debug, warn};
use unicode_normalization::UnicodeNormalization;

use crate::data::*;
use crate::utils::*;

pub(crate) type CharCode = u32;

#[derive(Clone)]
pub(crate) struct PdfSimpleFont<'a> {
    font: &'a Dictionary,
    doc: &'a Document,
    base_name: String,
    encoding: Option<Vec<u16>>,
    unicode_map: Option<HashMap<u32, String>>,
    widths: HashMap<CharCode, f32>,
    missing_width: f32,
}

#[derive(Clone)]
pub(crate) struct PdfType3Font<'a> {
    font: &'a Dictionary,
    base_name: String,
    encoding: Option<Vec<u16>>,
    unicode_map: Option<HashMap<CharCode, String>>,
    widths: HashMap<CharCode, f32>,
}

pub(crate) struct PdfCIDFont<'a> {
    font: &'a Dictionary,
    #[allow(dead_code)]
    doc: &'a Document,
    base_name: String,
    #[allow(dead_code)]
    encoding: ByteMapping,
    to_unicode: Option<HashMap<u32, String>>,
    fallback_unicode: Option<HashMap<u32, String>>,
    width_fallback: Option<HashMap<u32, String>>,
    widths: HashMap<CharCode, f32>,
    default_width: Option<f32>,
}

#[derive(Copy, Clone)]
pub(crate) struct PdfFontDescriptor<'a> {
    desc: &'a Dictionary,
    doc: &'a Document,
}

pub(crate) struct PdfFontIter<'a> {
    i: Iter<'a, u8>,
    font: &'a dyn PdfFont,
}

impl<'a> Iterator for PdfFontIter<'a> {
    type Item = (CharCode, u8);
    fn next(&mut self) -> Option<(CharCode, u8)> {
        self.font.next_char(&mut self.i)
    }
}

pub(crate) trait PdfFont: Debug {
    fn get_width(&self, id: CharCode) -> f32;
    fn next_char(&self, iter: &mut Iter<u8>) -> Option<(CharCode, u8)>;
    fn decode_char(&self, char: CharCode) -> String;
    fn get_font_name(&self) -> &str;
}

impl<'a> dyn PdfFont + 'a {
    pub(crate) fn char_codes(&'a self, chars: &'a [u8]) -> PdfFontIter<'a> {
        PdfFontIter {
            i: chars.iter(),
            font: self,
        }
    }
    pub(crate) fn decode(&self, chars: &[u8]) -> String {
        let strings = self
            .char_codes(chars)
            .map(|x| self.decode_char(x.0))
            .collect::<Vec<_>>();
        strings.join("")
    }
}

pub(crate) fn make_font<'a>(doc: &'a Document, font: &'a Dictionary) -> Rc<dyn PdfFont + 'a> {
    let subtype = get_name_string(doc, font, b"Subtype");
    debug!("MakeFont({})", subtype);
    if subtype == "Type0" {
        Rc::new(PdfCIDFont::new(doc, font))
    } else if subtype == "Type3" {
        Rc::new(PdfType3Font::new(doc, font))
    } else {
        Rc::new(PdfSimpleFont::new(doc, font))
    }
}

fn encoding_to_unicode_table(name: &[u8]) -> Vec<u16> {
    let encoding = match &name[..] {
        b"MacRomanEncoding" => MAC_ROMAN_ENCODING,
        b"MacExpertEncoding" => MAC_EXPERT_ENCODING,
        b"WinAnsiEncoding" => WIN_ANSI_ENCODING,
        _ => panic!("unexpected encoding {:?}", pdf_to_utf8(name)),
    };
    let encoding_table = encoding
        .iter()
        .map(|x| {
            if let &Some(x) = x {
                GLYPH_NAMES
                    .binary_search_by_key(&x, |&(n, _)| n)
                    .ok()
                    .map(|i| GLYPH_NAMES[i].1)
                    .unwrap()
            } else {
                0
            }
        })
        .collect();
    encoding_table
}

impl<'a> PdfSimpleFont<'a> {
    fn new(doc: &'a Document, font: &'a Dictionary) -> PdfSimpleFont<'a> {
        let base_name = get_name_string(doc, font, b"BaseFont");
        let subtype = get_name_string(doc, font, b"Subtype");

        let encoding: Option<&Object> = get(doc, font, b"Encoding");
        debug!(
            "base_name {} {} enc:{:?} {:?}",
            base_name, subtype, encoding, font
        );
        let descriptor: Option<&Dictionary> = get(doc, font, b"FontDescriptor");
        let mut type1_encoding = None;
        let mut unicode_map: Option<HashMap<u32, String>> = None;
        if let Some(descriptor) = descriptor {
            debug!("descriptor {:?}", descriptor);
            if subtype == "Type1" {
                let file = maybe_get_obj(doc, descriptor, b"FontFile");
                match file {
                    Some(&Object::Stream(ref s)) => {
                        let s = get_contents(s);
                        type1_encoding =
                            Some(type1_encoding_parser::get_encoding_map(&s).expect("encoding"));
                    }
                    _ => {
                        debug!("font file {:?}", file)
                    }
                }
            } else if subtype == "TrueType" {
                let file = maybe_get_obj(doc, descriptor, b"FontFile2");
                match file {
                    Some(&Object::Stream(ref s)) => {
                        let _s = get_contents(s);
                    }
                    _ => {
                        debug!("font file {:?}", file)
                    }
                }
            }

            let font_file3 = get::<Option<&Object>>(doc, descriptor, b"FontFile3");
            match font_file3 {
                Some(&Object::Stream(ref s)) => {
                    let subtype = get_name_string(doc, &s.dict, b"Subtype");
                    debug!("font file {}, {:?}", subtype, s);
                    let s = get_contents(s);
                    if subtype == "Type1C" {
                        debug!(
                            "Parsing Type1C font - will use PDF-level encoding instead of CFF encoding"
                        );
                        // For Type1C fonts, we don't extract the CFF encoding table
                        // Instead, we'll use the PDF-level encoding (MacRomanEncoding, etc.)
                        // combined with the font's glyph names
                    }
                }
                None => {}
                _ => {
                    debug!("unexpected")
                }
            }

            let charset = maybe_get_obj(doc, descriptor, b"CharSet");
            let _charset = match charset {
                Some(&Object::String(ref s, _)) => Some(pdf_to_utf8(&s)),
                _ => None,
            };
        }

        let mut unicode_map = match unicode_map {
            Some(mut unicode_map) => {
                unicode_map.extend(get_unicode_map(doc, font).unwrap_or(HashMap::new()));
                Some(unicode_map)
            }
            None => get_unicode_map(doc, font),
        };

        let mut encoding_table = None;

        // Use PDF-level encoding (MacRomanEncoding, etc.)
        match encoding {
            Some(&Object::Name(ref encoding_name)) => {
                debug!("encoding {:?}", pdf_to_utf8(encoding_name));
                encoding_table = Some(encoding_to_unicode_table(encoding_name));
            }
            _ => {}
        }

        match encoding {
            Some(&Object::Name(_)) if encoding_table.is_some() => {
                // Already handled above
            }
            Some(&Object::Dictionary(ref encoding)) => {
                let mut table =
                    if let Some(base_encoding) = maybe_get_name(doc, encoding, b"BaseEncoding") {
                        debug!("BaseEncoding {:?}", base_encoding);
                        encoding_to_unicode_table(base_encoding)
                    } else {
                        Vec::from(PDFDocEncoding)
                    };
                let differences = maybe_get_array(doc, encoding, b"Differences");
                if let Some(differences) = differences {
                    debug!("Differences");
                    let mut code = 0;
                    for o in differences {
                        let o = maybe_deref(doc, o);
                        match o {
                            &Object::Integer(i) => {
                                code = i;
                            }
                            &Object::Name(ref n) => {
                                let name = pdf_to_utf8(&n);
                                let unicode = GLYPH_NAMES
                                    .binary_search_by_key(&name.as_str(), |&(n, _)| n)
                                    .ok()
                                    .map(|i| GLYPH_NAMES[i].1);
                                if let Some(unicode) = unicode {
                                    table[code as usize] = unicode;
                                    if let Some(ref mut unicode_map) = unicode_map {
                                        let be = [unicode];
                                        match unicode_map.entry(code as u32) {
                                            Entry::Vacant(v) => {
                                                v.insert(String::from_utf16(&be).unwrap());
                                            }
                                            Entry::Occupied(e) => {
                                                if e.get() != &String::from_utf16(&be).unwrap() {
                                                    let normal_match =
                                                        e.get().nfkc().eq(String::from_utf16(&be)
                                                            .unwrap()
                                                            .nfkc());
                                                    if !normal_match {
                                                        warn!(
                                                            "Unicode mismatch {} {} {:?} {:?} {:?}",
                                                            normal_match,
                                                            name,
                                                            e.get(),
                                                            String::from_utf16(&be),
                                                            be
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    match unicode_map {
                                        Some(ref mut unicode_map)
                                            if base_name.contains("FontAwesome") =>
                                        {
                                            match unicode_map.entry(code as u32) {
                                                Entry::Vacant(v) => {
                                                    v.insert("".to_owned());
                                                }
                                                Entry::Occupied(_e) => {
                                                    panic!("unexpected entry in unicode map")
                                                }
                                            }
                                        }
                                        _ => {
                                            warn!(
                                                "unknown glyph name '{}' for font {}",
                                                name, base_name
                                            );
                                        }
                                    }
                                }
                                debug!("{} = {} ({:?})", code, name, unicode);
                                if let Some(ref mut unicode_map) = unicode_map {
                                    debug!("{} {:?}", code, unicode_map.get(&(code as u32)));
                                }
                                code += 1;
                            }
                            _ => {
                                panic!("wrong type {:?}", o);
                            }
                        }
                    }
                }
                let name = encoding
                    .get(b"Type")
                    .and_then(|x| x.as_name())
                    .and_then(|x| Ok(pdf_to_utf8(x)));
                debug!("name: {:?}", name);

                encoding_table = Some(table);
            }
            None => {
                if let Some(type1_encoding) = type1_encoding {
                    let mut table = Vec::from(PDFDocEncoding);
                    debug!("type1encoding");
                    for (code, name) in type1_encoding {
                        let name_str = pdf_to_utf8(&name);
                        let unicode = GLYPH_NAMES
                            .binary_search_by_key(&name_str.as_str(), |&(n, _)| n)
                            .ok()
                            .map(|i| GLYPH_NAMES[i].1);
                        if let Some(unicode) = unicode {
                            table[code as usize] = unicode;
                        } else {
                            debug!("unknown character {}", pdf_to_utf8(&name));
                        }
                    }
                    encoding_table = Some(table)
                } else if subtype == "TrueType" {
                    encoding_table = Some(
                        WIN_ANSI_ENCODING
                            .iter()
                            .map(|x| {
                                if let &Some(x) = x {
                                    GLYPH_NAMES
                                        .binary_search_by_key(&x, |&(n, _)| n)
                                        .ok()
                                        .map(|i| GLYPH_NAMES[i].1)
                                        .unwrap()
                                } else {
                                    0
                                }
                            })
                            .collect(),
                    );
                }
            }
            _ => {
                panic!()
            }
        }

        let mut width_map = HashMap::new();

        if let (Some(first_char), Some(last_char), Some(widths)) = (
            maybe_get::<i64>(doc, font, b"FirstChar"),
            maybe_get::<i64>(doc, font, b"LastChar"),
            maybe_get::<Vec<f32>>(doc, font, b"Widths"),
        ) {
            let mut i: i64 = 0;
            debug!(
                "first_char {:?}, last_char: {:?}, widths: {} {:?}",
                first_char,
                last_char,
                widths.len(),
                widths
            );

            for w in widths {
                width_map.insert((first_char + i) as CharCode, w);
                i += 1;
            }
            assert_eq!(first_char + i - 1, last_char);
        } else {
            for font_metrics in CORE_FONT_METRICS.iter() {
                if font_metrics.0 == base_name {
                    if let Some(ref encoding) = encoding_table {
                        debug!("has encoding");
                        for w in font_metrics.2 {
                            let c = GLYPH_NAMES
                                .binary_search_by_key(&w.2, |&(n, _)| n)
                                .ok()
                                .map(|i| GLYPH_NAMES[i].1)
                                .unwrap();
                            for i in 0..encoding.len() {
                                if encoding[i] == c {
                                    width_map.insert(i as CharCode, w.1 as f32);
                                }
                            }
                        }
                    } else {
                        let mut table = vec![0; 256];
                        for w in font_metrics.2 {
                            debug!("{} {}", w.0, w.2);
                            if w.0 != -1 {
                                table[w.0 as usize] = if base_name == "ZapfDingbats" {
                                    ZAPF_DINGBATS_NAMES
                                        .binary_search_by_key(&w.2, |&(n, _)| n)
                                        .ok()
                                        .map(|i| ZAPF_DINGBATS_NAMES[i].1)
                                        .unwrap_or_else(|| panic!("bad name {:?}", w))
                                } else {
                                    GLYPH_NAMES
                                        .binary_search_by_key(&w.2, |&(n, _)| n)
                                        .ok()
                                        .map(|i| GLYPH_NAMES[i].1)
                                        .unwrap()
                                }
                            }
                        }

                        let encoding = &table[..];
                        for w in font_metrics.2 {
                            width_map.insert(w.0 as CharCode, w.1 as f32);
                        }
                        encoding_table = Some(encoding.to_vec());
                    }
                }
            }
        }

        let missing_width = get::<Option<f32>>(doc, font, b"MissingWidth").unwrap_or(0.);
        PdfSimpleFont {
            doc,
            font,
            base_name,
            widths: width_map,
            encoding: encoding_table,
            missing_width,
            unicode_map,
        }
    }

    #[allow(dead_code)]
    fn get_type(&self) -> String {
        get_name_string(self.doc, self.font, b"Type")
    }
    #[allow(dead_code)]
    fn get_basefont(&self) -> String {
        get_name_string(self.doc, self.font, b"BaseFont")
    }
    #[allow(dead_code)]
    fn get_subtype(&self) -> String {
        get_name_string(self.doc, self.font, b"Subtype")
    }
    #[allow(dead_code)]
    fn get_widths(&self) -> Option<&Vec<Object>> {
        maybe_get_obj(self.doc, self.font, b"Widths")
            .map(|widths| widths.as_array().expect("Widths should be an array"))
    }
    #[allow(dead_code)]
    fn get_name(&self) -> Option<String> {
        maybe_get_name_string(self.doc, self.font, b"Name")
    }

    #[allow(dead_code)]
    fn get_descriptor(&self) -> Option<PdfFontDescriptor<'_>> {
        maybe_get_obj(self.doc, self.font, b"FontDescriptor")
            .and_then(|desc| desc.as_dict().ok())
            .map(|desc| PdfFontDescriptor {
                desc: desc,
                doc: self.doc,
            })
    }
}

impl<'a> PdfFont for PdfSimpleFont<'a> {
    fn get_width(&self, id: CharCode) -> f32 {
        let width = self.widths.get(&id);
        if let Some(width) = width {
            return *width;
        } else {
            let mut widths = self.widths.iter().collect::<Vec<_>>();
            widths.sort_by_key(|x| x.0);
            debug!(
                "missing width for {} len(widths) = {}, {:?} falling back to missing_width {:?}",
                id,
                self.widths.len(),
                widths,
                self.font
            );
            return self.missing_width;
        }
    }

    fn next_char(&self, iter: &mut Iter<u8>) -> Option<(CharCode, u8)> {
        iter.next().map(|x| (*x as CharCode, 1))
    }
    fn decode_char(&self, char: CharCode) -> String {
        let slice = [char as u8];
        if let Some(ref unicode_map) = self.unicode_map {
            let s = unicode_map.get(&char);
            let s = match s {
                None => {
                    debug!(
                        "missing char {} in unicode map, falling back to encoding",
                        char
                    );
                    let encoding = self
                        .encoding
                        .as_ref()
                        .map(|x| &x[..])
                        .expect("missing unicode map and encoding");
                    let s = to_utf8(encoding, &slice);
                    debug!("falling back to encoding {} -> {:?}", char, s);
                    s
                }
                Some(s) => s.clone(),
            };
            return s;
        }
        let encoding = self
            .encoding
            .as_ref()
            .map(|x| &x[..])
            .unwrap_or(&PDFDocEncoding);
        let s = to_utf8(encoding, &slice);
        s
    }
    fn get_font_name(&self) -> &str {
        &self.base_name
    }
}

impl<'a> fmt::Debug for PdfSimpleFont<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.font.fmt(f)
    }
}

impl<'a> PdfType3Font<'a> {
    fn new(doc: &'a Document, font: &'a Dictionary) -> PdfType3Font<'a> {
        let base_name = get_name_string(doc, font, b"BaseFont");
        let unicode_map = get_unicode_map(doc, font);
        let encoding: Option<&Object> = get(doc, font, b"Encoding");

        let encoding_table;
        match encoding {
            Some(&Object::Name(ref encoding_name)) => {
                debug!("encoding {:?}", pdf_to_utf8(encoding_name));
                encoding_table = Some(encoding_to_unicode_table(encoding_name));
            }
            Some(&Object::Dictionary(ref encoding)) => {
                let mut table =
                    if let Some(base_encoding) = maybe_get_name(doc, encoding, b"BaseEncoding") {
                        debug!("BaseEncoding {:?}", base_encoding);
                        encoding_to_unicode_table(base_encoding)
                    } else {
                        Vec::from(PDFDocEncoding)
                    };
                let differences = maybe_get_array(doc, encoding, b"Differences");
                if let Some(differences) = differences {
                    debug!("Differences");
                    let mut code = 0;
                    for o in differences {
                        match o {
                            &Object::Integer(i) => {
                                code = i;
                            }
                            &Object::Name(ref n) => {
                                let name = pdf_to_utf8(&n);
                                let unicode = GLYPH_NAMES
                                    .binary_search_by_key(&name.as_str(), |&(n, _)| n)
                                    .ok()
                                    .map(|i| GLYPH_NAMES[i].1);
                                if let Some(unicode) = unicode {
                                    table[code as usize] = unicode;
                                }
                                debug!("{} = {} ({:?})", code, name, unicode);
                                if let Some(ref unicode_map) = unicode_map {
                                    debug!("{} {:?}", code, unicode_map.get(&(code as u32)));
                                }
                                code += 1;
                            }
                            _ => {
                                panic!("wrong type");
                            }
                        }
                    }
                }
                let name_encoded = encoding.get(b"Type");
                if let Ok(Object::Name(name)) = name_encoded {
                    debug!("name: {}", pdf_to_utf8(name));
                } else {
                    debug!("name not found");
                }

                encoding_table = Some(table);
            }
            _ => {
                panic!()
            }
        }

        let first_char: i64 = get(doc, font, b"FirstChar");
        let last_char: i64 = get(doc, font, b"LastChar");
        let widths: Vec<f32> = get(doc, font, b"Widths");

        let mut width_map = HashMap::new();

        let mut i = 0;
        debug!(
            "first_char {:?}, last_char: {:?}, widths: {} {:?}",
            first_char,
            last_char,
            widths.len(),
            widths
        );

        for w in widths {
            width_map.insert((first_char + i) as CharCode, w);
            i += 1;
        }
        assert_eq!(first_char + i - 1, last_char);
        PdfType3Font {
            font,
            base_name,
            widths: width_map,
            encoding: encoding_table,
            unicode_map,
        }
    }
}

impl<'a> PdfFont for PdfType3Font<'a> {
    fn get_width(&self, id: CharCode) -> f32 {
        let width = self.widths.get(&id);
        if let Some(width) = width {
            return *width;
        } else {
            panic!("missing width for {} {:?}", id, self.font);
        }
    }

    fn next_char(&self, iter: &mut Iter<u8>) -> Option<(CharCode, u8)> {
        iter.next().map(|x| (*x as CharCode, 1))
    }
    fn decode_char(&self, char: CharCode) -> String {
        let slice = [char as u8];
        if let Some(ref unicode_map) = self.unicode_map {
            let s = unicode_map.get(&char);
            let s = match s {
                None => {
                    debug!(
                        "missing char {} in unicode map, falling back to encoding",
                        char
                    );
                    let encoding = self
                        .encoding
                        .as_ref()
                        .map(|x| &x[..])
                        .expect("missing unicode map and encoding");
                    let s = to_utf8(encoding, &slice);
                    debug!("falling back to encoding {} -> {:?}", char, s);
                    s
                }
                Some(s) => s.clone(),
            };
            return s;
        }
        let encoding = self
            .encoding
            .as_ref()
            .map(|x| &x[..])
            .unwrap_or(&PDFDocEncoding);
        let s = to_utf8(encoding, &slice);
        s
    }
    fn get_font_name(&self) -> &str {
        &self.base_name
    }
}

impl<'a> fmt::Debug for PdfType3Font<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.font.fmt(f)
    }
}

fn get_unicode_map<'a>(doc: &'a Document, font: &'a Dictionary) -> Option<HashMap<u32, String>> {
    let to_unicode = maybe_get_obj(doc, font, b"ToUnicode");
    debug!("ToUnicode: {:?}", to_unicode);
    let mut unicode_map = None;
    match to_unicode {
        Some(&Object::Stream(ref stream)) => {
            let contents = get_contents(stream);
            debug!("Stream: {}", String::from_utf8(contents.clone()).unwrap());

            let cmap = adobe_cmap_parser::get_unicode_map(&contents).unwrap();

            if let Some(bytes_1374) = cmap.get(&1374) {
                debug!(
                    "adobe_cmap_parser returned for CID 1374: {:?} (bytes: {:02X?})",
                    bytes_1374, bytes_1374
                );
            } else {
                debug!("adobe_cmap_parser has NO entry for CID 1374");
            }

            let mut unicode = HashMap::new();
            for (&k, v) in cmap.iter() {
                let mut be: Vec<u16> = Vec::new();
                let mut i = 0;
                assert!(v.len() % 2 == 0);
                while i < v.len() {
                    be.push(((v[i] as u16) << 8) | v[i + 1] as u16);
                    i += 2;
                }
                match &be[..] {
                    [0xd800..=0xdfff] => {
                        continue;
                    }
                    _ => {}
                }
                let s = String::from_utf16(&be).unwrap();

                if k == 1374 {
                    debug!(
                        "Processing CID 1374: raw bytes {:02X?} → UTF-16BE {:04X?} → string {:?}",
                        v, be, s
                    );
                }

                unicode.insert(k, s);
            }
            unicode_map = Some(unicode);

            debug!("map: {:?}", unicode_map);
        }
        None => {}
        Some(&Object::Name(ref name)) => {
            let name = pdf_to_utf8(name);
            if name != "Identity-H" {
                todo!("unsupported ToUnicode name: {:?}", name);
            }
        }
        _ => {
            panic!("unsupported cmap {:?}", to_unicode)
        }
    }
    unicode_map
}

fn get_fallback_unicode_from_font<'a>(
    doc: &'a Document,
    ciddict: &'a Dictionary,
) -> Option<HashMap<u32, String>> {
    let font_descriptor = maybe_get_obj(doc, ciddict, b"FontDescriptor")?;
    let font_descriptor = font_descriptor.as_dict().ok()?;

    let mut cid_to_gid: Option<HashMap<u32, u32>> = None;
    if let Some(cid_to_gid_map) = maybe_get_obj(doc, ciddict, b"CIDToGIDMap") {
        debug!("Found CIDToGIDMap object: {:?}", cid_to_gid_map);
        match cid_to_gid_map {
            &Object::Stream(ref stream) => {
                let data = get_contents(stream);
                debug!("CIDToGIDMap stream has {} bytes", data.len());
                let mut map = HashMap::new();
                for (cid, chunk) in data.chunks_exact(2).enumerate() {
                    let gid = ((chunk[0] as u32) << 8) | (chunk[1] as u32);
                    if gid != 0 {
                        map.insert(cid as u32, gid);
                        if cid < 5 || cid == 1374 {
                            debug!("  CIDToGIDMap[{}] = {}", cid, gid);
                        }
                    }
                }
                debug!("Loaded CIDToGIDMap with {} entries", map.len());
                cid_to_gid = Some(map);
            }
            &Object::Name(ref name) => {
                let name = pdf_to_utf8(name);
                if name == "Identity" {
                    debug!("CIDToGIDMap is Identity (CID = GID)");
                } else {
                    debug!("Unknown CIDToGIDMap name: {}", name);
                }
            }
            _ => {
                debug!("CIDToGIDMap is unexpected type");
            }
        }
    } else {
        debug!("No CIDToGIDMap found, assuming Identity");
    }

    let font_stream = maybe_get_obj(doc, font_descriptor, b"FontFile2")
        .or_else(|| maybe_get_obj(doc, font_descriptor, b"FontFile3"));

    if let Some(&Object::Stream(ref stream)) = font_stream {
        let font_data = get_contents(stream);

        if let Ok(face) = ttf_parser::Face::parse(&font_data, 0) {
            let mut fallback_map = HashMap::new();

            if let Some(cmap_table) = face.tables().cmap {
                let mut count = 0;
                for subtable in cmap_table.subtables {
                    debug!(
                        "  cmap subtable {}: platform={:?} encoding={} format={:?}",
                        count, subtable.platform_id, subtable.encoding_id, subtable.format
                    );
                    count += 1;
                }
                debug!("Font has {} cmap subtables total", count);
            }

            let num_glyphs = face.number_of_glyphs();
            debug!("Font has {} total glyphs", num_glyphs);

            if let Some(_post_table) = face.tables().post {
                debug!("Font has post table");
                let gid_1374 = ttf_parser::GlyphId(1374);
                if gid_1374.0 < num_glyphs {
                    if let Some(name) = face.glyph_name(gid_1374) {
                        debug!("GID 1374 has name: {:?}", name);
                    } else {
                        debug!("GID 1374 exists but has NO name in post table");
                    }
                } else {
                    debug!(
                        "GID 1374 is out of range (font has only {} glyphs)",
                        num_glyphs
                    );
                }
            } else {
                debug!("Font has NO post table");
            }

            if let Some(_cff_table) = face.tables().cff {
                debug!("Font has CFF table (OpenType CFF font)");
                let gid_1374 = ttf_parser::GlyphId(1374);
                if gid_1374.0 < num_glyphs {
                    if let Some(name) = face.glyph_name(gid_1374) {
                        debug!("GID 1374 CFF name: {:?}", name);
                    } else {
                        debug!("GID 1374 has no CFF glyph name");
                    }
                }
            } else {
                debug!("Font is NOT a CFF font (probably TrueType)");
            }

            for subtable in face.tables().cmap.iter().flat_map(|cmap| cmap.subtables) {
                let is_unicode = match (subtable.platform_id, subtable.encoding_id) {
                    (ttf_parser::PlatformId::Unicode, _) => true,
                    (ttf_parser::PlatformId::Windows, 1) => true,
                    (ttf_parser::PlatformId::Windows, 10) => true,
                    _ => false,
                };

                if is_unicode {
                    let mut gid_to_unicode: HashMap<u32, String> = HashMap::new();
                    let mut sample_count = 0;
                    subtable.codepoints(|codepoint| {
                        if let Some(gid) = subtable.glyph_index(codepoint) {
                            if let Some(c) = char::from_u32(codepoint) {
                                gid_to_unicode.insert(gid.0 as u32, c.to_string());
                                if codepoint == 0x002D
                                    || codepoint == 0x2010
                                    || codepoint == 0x2011
                                    || codepoint == 0x2012
                                    || codepoint == 0x2013
                                {
                                    debug!(
                                        "Found dash character U+{:04X} at GID {}",
                                        codepoint, gid.0
                                    );
                                }
                                if sample_count < 10 {
                                    debug!(
                                        "  cmap sample: U+{:04X} '{}' → GID {}",
                                        codepoint, c, gid.0
                                    );
                                    sample_count += 1;
                                }
                            }
                        }
                    });

                    if let Some(ref cid_gid_map) = cid_to_gid {
                        for (&cid, &gid) in cid_gid_map.iter() {
                            if let Some(unicode) = gid_to_unicode.get(&gid) {
                                fallback_map.insert(cid, unicode.clone());
                            }
                        }
                        debug!(
                            "Built fallback map using CIDToGIDMap: {} CIDs mapped",
                            fallback_map.len()
                        );
                    } else {
                        for (&gid, unicode) in gid_to_unicode.iter() {
                            fallback_map.insert(gid, unicode.clone());
                        }
                        debug!(
                            "Built fallback map assuming Identity (CID=GID): {} entries",
                            fallback_map.len()
                        );
                        if gid_to_unicode.contains_key(&1374) {
                            debug!("GID 1374 maps to: {:?}", gid_to_unicode.get(&1374));
                        } else {
                            debug!("GID 1374 NOT found in font cmap");
                        }
                    }

                    if !fallback_map.is_empty() {
                        debug!(
                            "Built fallback Unicode map from embedded font with {} entries",
                            fallback_map.len()
                        );
                        return Some(fallback_map);
                    }
                }
            }
        }
    }

    None
}

fn get_width_fallback_from_system_font(
    font: &Dictionary,
    pdf_widths: &HashMap<u32, f32>,
) -> Option<HashMap<u32, String>> {
    let base_name = if let Ok(Object::Name(name)) = font.get(b"BaseFont") {
        pdf_to_utf8(name)
    } else {
        return None;
    };

    debug!("Attempting to load system font for BaseFont: {}", base_name);

    let system_font_data = load_system_font(&base_name)?;
    let system_face = ttf_parser::Face::parse(&system_font_data, 0).ok()?;

    debug!(
        "Loaded system font: {} glyphs",
        system_face.number_of_glyphs()
    );

    let units_per_em = system_face.units_per_em() as f32;
    let mut width_to_chars: HashMap<i32, Vec<char>> = HashMap::new();

    if let Some(cmap_table) = system_face.tables().cmap {
        for subtable in cmap_table.subtables {
            subtable.codepoints(|codepoint| {
                if let Some(char_val) = char::from_u32(codepoint) {
                    if let Some(gid) = subtable.glyph_index(codepoint) {
                        if let Some(width) = system_face.glyph_hor_advance(gid) {
                            let normalized_width = ((width as f32 / units_per_em) * 1000.0) as i32;
                            width_to_chars
                                .entry(normalized_width)
                                .or_insert_with(Vec::new)
                                .push(char_val);
                        }
                    }
                }
            });
        }
    }

    debug!(
        "Built width map with {} distinct widths from system font",
        width_to_chars.len()
    );

    let mut fallback_map = HashMap::new();
    const WIDTH_TOLERANCE_PERCENT: f32 = 2.0;

    for (&cid, &pdf_width) in pdf_widths.iter() {
        let pdf_width_int = pdf_width as i32;
        let tolerance = ((pdf_width * WIDTH_TOLERANCE_PERCENT / 100.0) as i32).max(1);

        for width_offset in 0..=tolerance {
            for &sign in &[1, -1] {
                let test_width = pdf_width_int + (sign * width_offset);
                if let Some(chars) = width_to_chars.get(&test_width) {
                    let preferred_char = chars
                        .iter()
                        .find(|&&c| {
                            c == ' ' || c == '-' || c.is_ascii_digit() || c.is_ascii_punctuation()
                        })
                        .or_else(|| chars.first());

                    if let Some(&matched_char) = preferred_char {
                        debug!(
                            "Width match: CID {} (width {}) → '{}' U+{:04X} (width {})",
                            cid, pdf_width, matched_char, matched_char as u32, test_width
                        );
                        fallback_map.insert(cid, matched_char.to_string());
                        break;
                    }
                }
            }
            if fallback_map.contains_key(&cid) {
                break;
            }
        }
    }

    if !fallback_map.is_empty() {
        debug!(
            "Built width fallback map with {} entries",
            fallback_map.len()
        );
        Some(fallback_map)
    } else {
        None
    }
}

fn load_system_font(font_name: &str) -> Option<Vec<u8>> {
    let parts: Vec<&str> = font_name.split('-').collect();
    if parts.is_empty() {
        return None;
    }

    let family = parts[0];
    let style = if parts.len() > 1 { parts[1] } else { "Regular" };

    debug!(
        "Looking for system font: family='{}' style='{}'",
        family, style
    );

    let search_paths = vec![
        format!("/System/Library/Fonts/{}.ttc", family),
        format!("/System/Library/Fonts/{}.ttf", family),
        format!("/Library/Fonts/{}-{}.ttf", family, style),
        format!("/Library/Fonts/{}/{}-{}.ttf", family, family, style),
        format!(
            "/usr/share/fonts/truetype/{}/{}-{}.ttf",
            family.to_lowercase(),
            family,
            style
        ),
        format!("/usr/share/fonts/TTF/{}-{}.ttf", family, style),
        format!(
            "{}/.local/share/fonts/{}-{}.ttf",
            std::env::var("HOME").unwrap_or_default(),
            family,
            style
        ),
        format!(
            "{}/Library/Fonts/{}-{}.ttf",
            std::env::var("HOME").unwrap_or_default(),
            family,
            style
        ),
    ];

    for path in search_paths {
        if let Ok(data) = std::fs::read(&path) {
            debug!("Found system font at: {}", path);
            return Some(data);
        }
    }

    debug!("Could not find system font for '{}'", font_name);
    None
}

impl<'a> PdfCIDFont<'a> {
    fn new(doc: &'a Document, font: &'a Dictionary) -> PdfCIDFont<'a> {
        let base_name = get_name_string(doc, font, b"BaseFont");
        let descendants =
            maybe_get_array(doc, font, b"DescendantFonts").expect("Descendant fonts required");
        let ciddict = maybe_deref(doc, &descendants[0])
            .as_dict()
            .expect("should be CID dict");
        let encoding =
            maybe_get_obj(doc, font, b"Encoding").expect("Encoding required in type0 fonts");
        debug!("base_name {} {:?}", base_name, font);

        let encoding = match encoding {
            &Object::Name(ref name) => {
                let name = pdf_to_utf8(name);
                debug!("encoding {:?}", name);
                if name == "Identity-H" || name == "Identity-V" {
                    ByteMapping {
                        codespace: vec![CodeRange {
                            width: 2,
                            start: 0,
                            end: 0xffff,
                        }],
                        cid: vec![CIDRange {
                            src_code_lo: 0,
                            src_code_hi: 0xffff,
                            dst_CID_lo: 0,
                        }],
                    }
                } else {
                    panic!("unsupported encoding {}", name);
                }
            }
            &Object::Stream(ref stream) => {
                let contents = get_contents(stream);
                debug!("Stream: {}", String::from_utf8(contents.clone()).unwrap());
                adobe_cmap_parser::get_byte_mapping(&contents).unwrap()
            }
            _ => {
                panic!("unsupported encoding {:?}", encoding)
            }
        };

        let unicode_map = get_unicode_map(doc, font);

        let fallback_unicode = get_fallback_unicode_from_font(doc, ciddict);

        debug!("descendents {:?} {:?}", descendants, ciddict);

        let font_dict = maybe_get_obj(doc, ciddict, b"FontDescriptor").expect("required");
        debug!("{:?}", font_dict);
        let _f = font_dict.as_dict().expect("must be dict");
        let default_width = get::<Option<i64>>(doc, ciddict, b"DW").unwrap_or(1000);
        let w: Option<Vec<&Object>> = get(doc, ciddict, b"W");
        debug!("widths {:?}", w);
        let mut widths = HashMap::new();
        let mut i = 0;
        if let Some(w) = w {
            while i < w.len() {
                if let &Object::Array(ref wa) = w[i + 1] {
                    let cid = w[i].as_i64().expect("id should be num");
                    let mut j = 0;
                    debug!("wa: {:?} -> {:?}", cid, wa);
                    for w in wa {
                        widths.insert((cid + j) as CharCode, as_num(w));
                        j += 1;
                    }
                    i += 2;
                } else {
                    let c_first = w[i].as_i64().expect("first should be num");
                    let c_last = w[i].as_i64().expect("last should be num");
                    let c_width = as_num(&w[i]);
                    for id in c_first..c_last {
                        widths.insert(id as CharCode, c_width);
                    }
                    i += 3;
                }
            }
        }
        let width_fallback = get_width_fallback_from_system_font(font, &widths);
        PdfCIDFont {
            doc,
            font,
            base_name,
            widths,
            to_unicode: unicode_map,
            fallback_unicode,
            width_fallback,
            encoding,
            default_width: Some(default_width as f32),
        }
    }
}

impl<'a> PdfFont for PdfCIDFont<'a> {
    fn get_width(&self, id: CharCode) -> f32 {
        let width = self.widths.get(&id);
        if let Some(width) = width {
            debug!("GetWidth {} -> {}", id, *width);
            return *width;
        } else {
            debug!("missing width for {} falling back to default_width", id);
            return self.default_width.unwrap();
        }
    }

    fn next_char(&self, iter: &mut Iter<u8>) -> Option<(CharCode, u8)> {
        let mut c = *iter.next()? as u32;
        let mut code = None;
        'outer: for width in 1..=4 {
            for range in &self.encoding.codespace {
                if c as u32 >= range.start && c as u32 <= range.end && range.width == width {
                    code = Some((c as u32, width));
                    break 'outer;
                }
            }
            let next = *iter.next()?;
            c = ((c as u32) << 8) | next as u32;
        }
        let code = code?;
        for range in &self.encoding.cid {
            if code.0 >= range.src_code_lo && code.0 <= range.src_code_hi {
                return Some((code.0 + range.dst_CID_lo, code.1 as u8));
            }
        }
        None
    }
    fn decode_char(&self, char: CharCode) -> String {
        let s = self.to_unicode.as_ref().and_then(|x| x.get(&char));
        if let Some(s) = s {
            if !s.is_empty() && !s.contains('\0') {
                return s.clone();
            }
            debug!(
                "ToUnicode returned null/empty for char {} (font: {:?}) - trying fallback",
                char,
                maybe_get_obj(self.doc, self.font, b"BaseFont")
            );
        }

        if let Some(ref fallback) = self.fallback_unicode {
            if let Some(s) = fallback.get(&char) {
                debug!(
                    "Using embedded font cmap fallback for char {}: {:?}",
                    char, s
                );
                return s.clone();
            }
        }

        if let Some(ref width_fallback) = self.width_fallback {
            if let Some(s) = width_fallback.get(&char) {
                debug!("Using width-based fallback for char {}: {:?}", char, s);
                return s.clone();
            }
        }

        debug!("Unknown character {} (no mapping found)", char);
        "".to_string()
    }
    fn get_font_name(&self) -> &str {
        &self.base_name
    }
}

impl<'a> fmt::Debug for PdfCIDFont<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.font.fmt(f)
    }
}

impl<'a> PdfFontDescriptor<'a> {
    #[allow(dead_code)]
    fn get_file(&self) -> Option<&'a Object> {
        maybe_get_obj(self.doc, self.desc, b"FontFile")
    }
}

impl<'a> fmt::Debug for PdfFontDescriptor<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.desc.fmt(f)
    }
}
