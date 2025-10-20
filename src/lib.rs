use std::collections::{HashMap, hash_map::Entry};
use std::fmt::{self, Debug, Formatter};
use std::{fs::File, marker::PhantomData, rc::Rc, slice::Iter, str};

use adobe_cmap_parser::{ByteMapping, CIDRange, CodeRange};
use encoding_rs::UTF_16BE;
use euclid::{Transform2D, vec2};
use lopdf::content::Content;
use lopdf::encryption::DecryptionError;
use lopdf::{Dictionary, Document, Error, Object, ObjectId, Stream, StringFormat};
use tracing::{debug, error, warn};
use unicode_normalization::UnicodeNormalization;

mod core_fonts;
mod encodings;
mod glyphnames;
mod zapfglyphnames;

pub struct Space;
pub type Transform = Transform2D<f64, Space, Space>;

#[derive(Debug)]
pub enum OutputError {
    FormatError(std::fmt::Error),
    IoError(std::io::Error),
    PdfError(lopdf::Error),
}

impl std::fmt::Display for OutputError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            OutputError::FormatError(e) => write!(f, "Formating error: {}", e),
            OutputError::IoError(e) => write!(f, "IO error: {}", e),
            OutputError::PdfError(e) => write!(f, "PDF error: {}", e),
        }
    }
}

impl std::error::Error for OutputError {}

impl From<std::fmt::Error> for OutputError {
    fn from(e: std::fmt::Error) -> Self {
        OutputError::FormatError(e)
    }
}

impl From<std::io::Error> for OutputError {
    fn from(e: std::io::Error) -> Self {
        OutputError::IoError(e)
    }
}

impl From<lopdf::Error> for OutputError {
    fn from(e: lopdf::Error) -> Self {
        OutputError::PdfError(e)
    }
}

macro_rules! dlog {
    ($($e:expr),*) => { {$(let _ = $e;)*} }
    //($($t:tt)*) => { eprintln!($($t)*) }
}

fn get_info(doc: &Document) -> Option<&Dictionary> {
    match doc.trailer.get(b"Info") {
        Ok(&Object::Reference(ref id)) => match doc.get_object(*id) {
            Ok(&Object::Dictionary(ref info)) => {
                return Some(info);
            }
            _ => {}
        },
        _ => {}
    }
    None
}

fn get_catalog(doc: &Document) -> &Dictionary {
    match doc.trailer.get(b"Root").unwrap() {
        &Object::Reference(ref id) => match doc.get_object(*id) {
            Ok(&Object::Dictionary(ref catalog)) => {
                return catalog;
            }
            _ => {}
        },
        _ => {}
    }
    panic!();
}

fn get_pages(doc: &Document) -> &Dictionary {
    let catalog = get_catalog(doc);
    match catalog.get(b"Pages").unwrap() {
        &Object::Reference(ref id) => match doc.get_object(*id) {
            Ok(&Object::Dictionary(ref pages)) => {
                return pages;
            }
            other => {
                dlog!("pages: {:?}", other)
            }
        },
        other => {
            dlog!("pages: {:?}", other)
        }
    }
    dlog!("catalog {:?}", catalog);
    panic!();
}

#[allow(non_upper_case_globals)]
const PDFDocEncoding: &'static [u16] = &[
    0x0000, 0x0001, 0x0002, 0x0003, 0x0004, 0x0005, 0x0006, 0x0007, 0x0008, 0x0009, 0x000a, 0x000b,
    0x000c, 0x000d, 0x000e, 0x000f, 0x0010, 0x0011, 0x0012, 0x0013, 0x0014, 0x0015, 0x0016, 0x0017,
    0x02d8, 0x02c7, 0x02c6, 0x02d9, 0x02dd, 0x02db, 0x02da, 0x02dc, 0x0020, 0x0021, 0x0022, 0x0023,
    0x0024, 0x0025, 0x0026, 0x0027, 0x0028, 0x0029, 0x002a, 0x002b, 0x002c, 0x002d, 0x002e, 0x002f,
    0x0030, 0x0031, 0x0032, 0x0033, 0x0034, 0x0035, 0x0036, 0x0037, 0x0038, 0x0039, 0x003a, 0x003b,
    0x003c, 0x003d, 0x003e, 0x003f, 0x0040, 0x0041, 0x0042, 0x0043, 0x0044, 0x0045, 0x0046, 0x0047,
    0x0048, 0x0049, 0x004a, 0x004b, 0x004c, 0x004d, 0x004e, 0x004f, 0x0050, 0x0051, 0x0052, 0x0053,
    0x0054, 0x0055, 0x0056, 0x0057, 0x0058, 0x0059, 0x005a, 0x005b, 0x005c, 0x005d, 0x005e, 0x005f,
    0x0060, 0x0061, 0x0062, 0x0063, 0x0064, 0x0065, 0x0066, 0x0067, 0x0068, 0x0069, 0x006a, 0x006b,
    0x006c, 0x006d, 0x006e, 0x006f, 0x0070, 0x0071, 0x0072, 0x0073, 0x0074, 0x0075, 0x0076, 0x0077,
    0x0078, 0x0079, 0x007a, 0x007b, 0x007c, 0x007d, 0x007e, 0x0000, 0x2022, 0x2020, 0x2021, 0x2026,
    0x2014, 0x2013, 0x0192, 0x2044, 0x2039, 0x203a, 0x2212, 0x2030, 0x201e, 0x201c, 0x201d, 0x2018,
    0x2019, 0x201a, 0x2122, 0xfb01, 0xfb02, 0x0141, 0x0152, 0x0160, 0x0178, 0x017d, 0x0131, 0x0142,
    0x0153, 0x0161, 0x017e, 0x0000, 0x20ac, 0x00a1, 0x00a2, 0x00a3, 0x00a4, 0x00a5, 0x00a6, 0x00a7,
    0x00a8, 0x00a9, 0x00aa, 0x00ab, 0x00ac, 0x0000, 0x00ae, 0x00af, 0x00b0, 0x00b1, 0x00b2, 0x00b3,
    0x00b4, 0x00b5, 0x00b6, 0x00b7, 0x00b8, 0x00b9, 0x00ba, 0x00bb, 0x00bc, 0x00bd, 0x00be, 0x00bf,
    0x00c0, 0x00c1, 0x00c2, 0x00c3, 0x00c4, 0x00c5, 0x00c6, 0x00c7, 0x00c8, 0x00c9, 0x00ca, 0x00cb,
    0x00cc, 0x00cd, 0x00ce, 0x00cf, 0x00d0, 0x00d1, 0x00d2, 0x00d3, 0x00d4, 0x00d5, 0x00d6, 0x00d7,
    0x00d8, 0x00d9, 0x00da, 0x00db, 0x00dc, 0x00dd, 0x00de, 0x00df, 0x00e0, 0x00e1, 0x00e2, 0x00e3,
    0x00e4, 0x00e5, 0x00e6, 0x00e7, 0x00e8, 0x00e9, 0x00ea, 0x00eb, 0x00ec, 0x00ed, 0x00ee, 0x00ef,
    0x00f0, 0x00f1, 0x00f2, 0x00f3, 0x00f4, 0x00f5, 0x00f6, 0x00f7, 0x00f8, 0x00f9, 0x00fa, 0x00fb,
    0x00fc, 0x00fd, 0x00fe, 0x00ff,
];

fn pdf_to_utf8(s: &[u8]) -> String {
    if s.len() > 2 && s[0] == 0xfe && s[1] == 0xff {
        return UTF_16BE
            .decode_without_bom_handling_and_without_replacement(&s[2..])
            .unwrap()
            .to_string();
    } else {
        let r: Vec<u8> = s
            .iter()
            .map(|x| *x)
            .flat_map(|x| {
                let k = PDFDocEncoding[x as usize];
                vec![(k >> 8) as u8, k as u8].into_iter()
            })
            .collect();
        return UTF_16BE
            .decode_without_bom_handling_and_without_replacement(&r)
            .unwrap()
            .to_string();
    }
}

fn to_utf8(encoding: &[u16], s: &[u8]) -> String {
    if s.len() > 2 && s[0] == 0xfe && s[1] == 0xff {
        return UTF_16BE
            .decode_without_bom_handling_and_without_replacement(&s[2..])
            .unwrap()
            .to_string();
    } else {
        let r: Vec<u8> = s
            .iter()
            .map(|x| *x)
            .flat_map(|x| {
                let k = encoding[x as usize];
                if k == 0 {
                    vec![].into_iter()
                } else {
                    vec![(k >> 8) as u8, k as u8].into_iter()
                }
            })
            .collect();
        return UTF_16BE
            .decode_without_bom_handling_and_without_replacement(&r)
            .unwrap()
            .to_string();
    }
}

fn maybe_deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    match o {
        &Object::Reference(r) => doc.get_object(r).expect("missing object reference"),
        _ => o,
    }
}

fn maybe_get_obj<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    dict.get(key).map(|o| maybe_deref(doc, o)).ok()
}

// an intermediate trait that can be used to chain conversions that may have failed
trait FromOptObj<'a> {
    fn from_opt_obj(doc: &'a Document, obj: Option<&'a Object>, key: &[u8]) -> Self;
}

// conditionally convert to Self returns None if the conversion failed
trait FromObj<'a>
where
    Self: std::marker::Sized,
{
    fn from_obj(doc: &'a Document, obj: &'a Object) -> Option<Self>;
}

impl<'a, T: FromObj<'a>> FromOptObj<'a> for Option<T> {
    fn from_opt_obj(doc: &'a Document, obj: Option<&'a Object>, _key: &[u8]) -> Self {
        obj.and_then(|x| T::from_obj(doc, x))
    }
}

impl<'a, T: FromObj<'a>> FromOptObj<'a> for T {
    fn from_opt_obj(doc: &'a Document, obj: Option<&'a Object>, key: &[u8]) -> Self {
        T::from_obj(doc, obj.expect(&String::from_utf8_lossy(key))).expect("wrong type")
    }
}

// we follow the same conventions as pdfium for when to support indirect objects:
// on arrays, streams and dicts
impl<'a, T: FromObj<'a>> FromObj<'a> for Vec<T> {
    fn from_obj(doc: &'a Document, obj: &'a Object) -> Option<Self> {
        maybe_deref(doc, obj)
            .as_array()
            .map(|x| {
                x.iter()
                    .map(|x| T::from_obj(doc, x).expect("wrong type"))
                    .collect()
            })
            .ok()
    }
}

// XXX: These will panic if we don't have the right number of items
// we don't want to do that
impl<'a, T: FromObj<'a>> FromObj<'a> for [T; 4] {
    fn from_obj(doc: &'a Document, obj: &'a Object) -> Option<Self> {
        maybe_deref(doc, obj)
            .as_array()
            .map(|x| {
                let mut all = x.iter().map(|x| T::from_obj(doc, x).expect("wrong type"));
                [
                    all.next().unwrap(),
                    all.next().unwrap(),
                    all.next().unwrap(),
                    all.next().unwrap(),
                ]
            })
            .ok()
    }
}

impl<'a, T: FromObj<'a>> FromObj<'a> for [T; 3] {
    fn from_obj(doc: &'a Document, obj: &'a Object) -> Option<Self> {
        maybe_deref(doc, obj)
            .as_array()
            .map(|x| {
                let mut all = x.iter().map(|x| T::from_obj(doc, x).expect("wrong type"));
                [
                    all.next().unwrap(),
                    all.next().unwrap(),
                    all.next().unwrap(),
                ]
            })
            .ok()
    }
}

impl<'a> FromObj<'a> for f64 {
    fn from_obj(_doc: &Document, obj: &Object) -> Option<Self> {
        match obj {
            &Object::Integer(i) => Some(i as f64),
            &Object::Real(f) => Some(f.into()),
            _ => None,
        }
    }
}

impl<'a> FromObj<'a> for i64 {
    fn from_obj(_doc: &Document, obj: &Object) -> Option<Self> {
        match obj {
            &Object::Integer(i) => Some(i),
            _ => None,
        }
    }
}

impl<'a> FromObj<'a> for &'a Dictionary {
    fn from_obj(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
        maybe_deref(doc, obj).as_dict().ok()
    }
}

impl<'a> FromObj<'a> for &'a Stream {
    fn from_obj(doc: &'a Document, obj: &'a Object) -> Option<&'a Stream> {
        maybe_deref(doc, obj).as_stream().ok()
    }
}

impl<'a> FromObj<'a> for &'a Object {
    fn from_obj(doc: &'a Document, obj: &'a Object) -> Option<&'a Object> {
        Some(maybe_deref(doc, obj))
    }
}

fn get<'a, T: FromOptObj<'a>>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> T {
    T::from_opt_obj(doc, dict.get(key).ok(), key)
}

fn maybe_get<'a, T: FromObj<'a>>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<T> {
    maybe_get_obj(doc, dict, key).and_then(|o| T::from_obj(doc, o))
}

fn get_name_string<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> String {
    pdf_to_utf8(
        dict.get(key)
            .map(|o| maybe_deref(doc, o))
            .unwrap_or_else(|_| panic!("deref"))
            .as_name()
            .expect("name"),
    )
}

#[allow(dead_code)]
fn maybe_get_name_string<'a>(
    doc: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<String> {
    maybe_get_obj(doc, dict, key)
        .and_then(|n| n.as_name().ok())
        .map(|n| pdf_to_utf8(n))
}

fn maybe_get_name<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a [u8]> {
    maybe_get_obj(doc, dict, key).and_then(|n| n.as_name().ok())
}

fn maybe_get_array<'a>(
    doc: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Vec<Object>> {
    maybe_get_obj(doc, dict, key).and_then(|n| n.as_array().ok())
}

#[derive(Clone)]
struct PdfSimpleFont<'a> {
    font: &'a Dictionary,
    doc: &'a Document,
    encoding: Option<Vec<u16>>,
    unicode_map: Option<HashMap<u32, String>>,
    widths: HashMap<CharCode, f64>, // should probably just use i32 here
    missing_width: f64,
}

#[derive(Clone)]
struct PdfType3Font<'a> {
    font: &'a Dictionary,
    doc: &'a Document,
    encoding: Option<Vec<u16>>,
    unicode_map: Option<HashMap<CharCode, String>>,
    widths: HashMap<CharCode, f64>, // should probably just use i32 here
}

fn make_font<'a>(doc: &'a Document, font: &'a Dictionary) -> Rc<dyn PdfFont + 'a> {
    let subtype = get_name_string(doc, font, b"Subtype");
    dlog!("MakeFont({})", subtype);
    if subtype == "Type0" {
        Rc::new(PdfCIDFont::new(doc, font))
    } else if subtype == "Type3" {
        Rc::new(PdfType3Font::new(doc, font))
    } else {
        Rc::new(PdfSimpleFont::new(doc, font))
    }
}

fn is_core_font(name: &str) -> bool {
    match name {
        "Courier-Bold"
        | "Courier-BoldOblique"
        | "Courier-Oblique"
        | "Courier"
        | "Helvetica-Bold"
        | "Helvetica-BoldOblique"
        | "Helvetica-Oblique"
        | "Helvetica"
        | "Symbol"
        | "Times-Bold"
        | "Times-BoldItalic"
        | "Times-Italic"
        | "Times-Roman"
        | "ZapfDingbats" => true,
        _ => false,
    }
}

fn encoding_to_unicode_table(name: &[u8]) -> Vec<u16> {
    let encoding = match &name[..] {
        b"MacRomanEncoding" => encodings::MAC_ROMAN_ENCODING,
        b"MacExpertEncoding" => encodings::MAC_EXPERT_ENCODING,
        b"WinAnsiEncoding" => encodings::WIN_ANSI_ENCODING,
        _ => panic!("unexpected encoding {:?}", pdf_to_utf8(name)),
    };
    let encoding_table = encoding
        .iter()
        .map(|x| {
            if let &Some(x) = x {
                glyphnames::name_to_unicode(x).unwrap()
            } else {
                0
            }
        })
        .collect();
    encoding_table
}

/* "Glyphs in the font are selected by single-byte character codes obtained from a string that
    is shown by the text-showing operators. Logically, these codes index into a table of 256
    glyphs; the mapping from codes to glyphs is called the font’s encoding. Each font program
    has a built-in encoding. Under some circumstances, the encoding can be altered by means
    described in Section 5.5.5, “Character Encoding.”
*/
impl<'a> PdfSimpleFont<'a> {
    fn new(doc: &'a Document, font: &'a Dictionary) -> PdfSimpleFont<'a> {
        let base_name = get_name_string(doc, font, b"BaseFont");
        let subtype = get_name_string(doc, font, b"Subtype");

        let encoding: Option<&Object> = get(doc, font, b"Encoding");
        dlog!(
            "base_name {} {} enc:{:?} {:?}",
            base_name,
            subtype,
            encoding,
            font
        );
        let descriptor: Option<&Dictionary> = get(doc, font, b"FontDescriptor");
        let mut type1_encoding = None;
        let mut unicode_map = None;
        if let Some(descriptor) = descriptor {
            dlog!("descriptor {:?}", descriptor);
            if subtype == "Type1" {
                let file = maybe_get_obj(doc, descriptor, b"FontFile");
                match file {
                    Some(&Object::Stream(ref s)) => {
                        let s = get_contents(s);
                        //dlog!("font contents {:?}", pdf_to_utf8(&s));
                        type1_encoding =
                            Some(type1_encoding_parser::get_encoding_map(&s).expect("encoding"));
                    }
                    _ => {
                        dlog!("font file {:?}", file)
                    }
                }
            } else if subtype == "TrueType" {
                let file = maybe_get_obj(doc, descriptor, b"FontFile2");
                match file {
                    Some(&Object::Stream(ref s)) => {
                        let _s = get_contents(s);
                        //File::create(format!("/tmp/{}", base_name)).unwrap().write_all(&s);
                    }
                    _ => {
                        dlog!("font file {:?}", file)
                    }
                }
            }

            let font_file3 = get::<Option<&Object>>(doc, descriptor, b"FontFile3");
            match font_file3 {
                Some(&Object::Stream(ref s)) => {
                    let subtype = get_name_string(doc, &s.dict, b"Subtype");
                    dlog!("font file {}, {:?}", subtype, s);
                    let s = get_contents(s);
                    if subtype == "Type1C" {
                        let table = cff_parser::Table::parse(&s).unwrap();
                        let charset = table.charset.get_table();
                        let encoding = table.encoding.get_table();
                        let mut mapping = HashMap::new();
                        for i in 0..encoding.len().min(charset.len()) {
                            let cid = encoding[i];
                            let sid = charset[i];
                            let name = cff_parser::string_by_id(&table, sid).unwrap();
                            let unicode = glyphnames::name_to_unicode(&name)
                                .or_else(|| zapfglyphnames::zapfdigbats_names_to_unicode(name));
                            if let Some(unicode) = unicode {
                                let str = String::from_utf16(&[unicode]).unwrap();
                                mapping.insert(cid as u32, str);
                            }
                        }
                        unicode_map = Some(mapping);
                        //
                        //File::create(format!("/tmp/{}", base_name)).unwrap().write_all(&s);
                    }

                    //
                    //File::create(format!("/tmp/{}", base_name)).unwrap().write_all(&s);
                }
                None => {}
                _ => {
                    dlog!("unexpected")
                }
            }

            let charset = maybe_get_obj(doc, descriptor, b"CharSet");
            let _charset = match charset {
                Some(&Object::String(ref s, _)) => Some(pdf_to_utf8(&s)),
                _ => None,
            };
            //dlog!("charset {:?}", charset);
        }

        let mut unicode_map = match unicode_map {
            Some(mut unicode_map) => {
                unicode_map.extend(get_unicode_map(doc, font).unwrap_or(HashMap::new()));
                Some(unicode_map)
            }
            None => get_unicode_map(doc, font),
        };

        let mut encoding_table = None;
        match encoding {
            Some(&Object::Name(ref encoding_name)) => {
                dlog!("encoding {:?}", pdf_to_utf8(encoding_name));
                encoding_table = Some(encoding_to_unicode_table(encoding_name));
            }
            Some(&Object::Dictionary(ref encoding)) => {
                //dlog!("Encoding {:?}", encoding);
                let mut table =
                    if let Some(base_encoding) = maybe_get_name(doc, encoding, b"BaseEncoding") {
                        dlog!("BaseEncoding {:?}", base_encoding);
                        encoding_to_unicode_table(base_encoding)
                    } else {
                        Vec::from(PDFDocEncoding)
                    };
                let differences = maybe_get_array(doc, encoding, b"Differences");
                if let Some(differences) = differences {
                    dlog!("Differences");
                    let mut code = 0;
                    for o in differences {
                        let o = maybe_deref(doc, o);
                        match o {
                            &Object::Integer(i) => {
                                code = i;
                            }
                            &Object::Name(ref n) => {
                                let name = pdf_to_utf8(&n);
                                // XXX: names of Type1 fonts can map to arbitrary strings instead of real
                                // unicode names, so we should probably handle this differently
                                let unicode = glyphnames::name_to_unicode(&name);
                                if let Some(unicode) = unicode {
                                    table[code as usize] = unicode;
                                    if let Some(ref mut unicode_map) = unicode_map {
                                        let be = [unicode];
                                        match unicode_map.entry(code as u32) {
                                            // If there's a unicode table entry missing use one based on the name
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
                                            // the fontawesome tex package will use glyph names that don't have a corresponding unicode
                                            // code point, so we'll use an empty string instead. See issue #76
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
                                dlog!("{} = {} ({:?})", code, name, unicode);
                                if let Some(ref mut unicode_map) = unicode_map {
                                    // The unicode map might not have the code in it, but the code might
                                    // not be used so we don't want to panic here.
                                    // An example of this is the 'suppress' character in the TeX Latin Modern font.
                                    // This shows up in https://arxiv.org/pdf/2405.01295v1.pdf
                                    dlog!("{} {:?}", code, unicode_map.get(&(code as u32)));
                                }
                                code += 1;
                            }
                            _ => {
                                panic!("wrong type {:?}", o);
                            }
                        }
                    }
                }
                // "Type" is optional
                let name = encoding
                    .get(b"Type")
                    .and_then(|x| x.as_name())
                    .and_then(|x| Ok(pdf_to_utf8(x)));
                dlog!("name: {:?}", name);

                encoding_table = Some(table);
            }
            None => {
                if let Some(type1_encoding) = type1_encoding {
                    let mut table = Vec::from(PDFDocEncoding);
                    dlog!("type1encoding");
                    for (code, name) in type1_encoding {
                        let unicode = glyphnames::name_to_unicode(&pdf_to_utf8(&name));
                        if let Some(unicode) = unicode {
                            table[code as usize] = unicode;
                        } else {
                            dlog!("unknown character {}", pdf_to_utf8(&name));
                        }
                    }
                    encoding_table = Some(table)
                } else if subtype == "TrueType" {
                    encoding_table = Some(
                        encodings::WIN_ANSI_ENCODING
                            .iter()
                            .map(|x| {
                                if let &Some(x) = x {
                                    glyphnames::name_to_unicode(x).unwrap()
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
        /* "Ordinarily, a font dictionary that refers to one of the standard fonts
        should omit the FirstChar, LastChar, Widths, and FontDescriptor entries.
        However, it is permissible to override a standard font by including these
        entries and embedding the font program in the PDF file."

        Note: some PDFs include a descriptor but still don't include these entries */

        // If we have widths prefer them over the core font widths. Needed for https://dkp.de/wp-content/uploads/parteitage/Sozialismusvorstellungen-der-DKP.pdf
        if let (Some(first_char), Some(last_char), Some(widths)) = (
            maybe_get::<i64>(doc, font, b"FirstChar"),
            maybe_get::<i64>(doc, font, b"LastChar"),
            maybe_get::<Vec<f64>>(doc, font, b"Widths"),
        ) {
            // Some PDF's don't have these like fips-197.pdf
            let mut i: i64 = 0;
            dlog!(
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
            let name = if is_core_font(&base_name) {
                &base_name
            } else {
                warn!("no widths and not core font {:?}", base_name);

                // This situation is handled differently by different readers
                // but basically we try to substitute the best font that we can.

                // Poppler/Xpdf:
                // this is technically an error -- the Widths entry is required
                // for all but the Base-14 fonts -- but certain PDF generators
                // apparently don't include widths for Arial and TimesNewRoman

                // Pdfium: CFX_FontMapper::FindSubstFont

                // mupdf: pdf_load_substitute_font

                // We can try to do a better job guessing at a font by looking at the flags
                // or the basename but for now we'll just use Helvetica
                "Helvetica"
            };
            for font_metrics in core_fonts::metrics().iter() {
                if font_metrics.0 == base_name {
                    if let Some(ref encoding) = encoding_table {
                        dlog!("has encoding");
                        for w in font_metrics.2 {
                            let c = glyphnames::name_to_unicode(w.2).unwrap();
                            for i in 0..encoding.len() {
                                if encoding[i] == c {
                                    width_map.insert(i as CharCode, w.1 as f64);
                                }
                            }
                        }
                    } else {
                        // Instead of using the encoding from the core font we'll just look up all
                        // of the character names. We should probably verify that this produces the
                        // same result.

                        let mut table = vec![0; 256];
                        for w in font_metrics.2 {
                            dlog!("{} {}", w.0, w.2);
                            // -1 is "not encoded"
                            if w.0 != -1 {
                                table[w.0 as usize] = if base_name == "ZapfDingbats" {
                                    zapfglyphnames::zapfdigbats_names_to_unicode(w.2)
                                        .unwrap_or_else(|| panic!("bad name {:?}", w))
                                } else {
                                    glyphnames::name_to_unicode(w.2).unwrap()
                                }
                            }
                        }

                        let encoding = &table[..];
                        for w in font_metrics.2 {
                            width_map.insert(w.0 as CharCode, w.1 as f64);
                            // -1 is "not encoded"
                        }
                        encoding_table = Some(encoding.to_vec());
                    }
                    /* "Ordinarily, a font dictionary that refers to one of the standard fonts
                    should omit the FirstChar, LastChar, Widths, and FontDescriptor entries.
                    However, it is permissible to override a standard font by including these
                    entries and embedding the font program in the PDF file."

                    Note: some PDFs include a descriptor but still don't include these entries */
                    // assert!(maybe_get_obj(doc, font, b"FirstChar").is_none());
                    // assert!(maybe_get_obj(doc, font, b"LastChar").is_none());
                    // assert!(maybe_get_obj(doc, font, b"Widths").is_none());
                }
            }
        }

        let missing_width = get::<Option<f64>>(doc, font, b"MissingWidth").unwrap_or(0.);
        PdfSimpleFont {
            doc,
            font,
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
    /* For type1: This entry is obsolescent and its use is no longer recommended. (See
     * implementation note 42 in Appendix H.) */
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

impl<'a> PdfType3Font<'a> {
    fn new(doc: &'a Document, font: &'a Dictionary) -> PdfType3Font<'a> {
        let unicode_map = get_unicode_map(doc, font);
        let encoding: Option<&Object> = get(doc, font, b"Encoding");

        let encoding_table;
        match encoding {
            Some(&Object::Name(ref encoding_name)) => {
                dlog!("encoding {:?}", pdf_to_utf8(encoding_name));
                encoding_table = Some(encoding_to_unicode_table(encoding_name));
            }
            Some(&Object::Dictionary(ref encoding)) => {
                //dlog!("Encoding {:?}", encoding);
                let mut table =
                    if let Some(base_encoding) = maybe_get_name(doc, encoding, b"BaseEncoding") {
                        dlog!("BaseEncoding {:?}", base_encoding);
                        encoding_to_unicode_table(base_encoding)
                    } else {
                        Vec::from(PDFDocEncoding)
                    };
                let differences = maybe_get_array(doc, encoding, b"Differences");
                if let Some(differences) = differences {
                    dlog!("Differences");
                    let mut code = 0;
                    for o in differences {
                        match o {
                            &Object::Integer(i) => {
                                code = i;
                            }
                            &Object::Name(ref n) => {
                                let name = pdf_to_utf8(&n);
                                // XXX: names of Type1 fonts can map to arbitrary strings instead of real
                                // unicode names, so we should probably handle this differently
                                let unicode = glyphnames::name_to_unicode(&name);
                                if let Some(unicode) = unicode {
                                    table[code as usize] = unicode;
                                }
                                dlog!("{} = {} ({:?})", code, name, unicode);
                                if let Some(ref unicode_map) = unicode_map {
                                    dlog!("{} {:?}", code, unicode_map.get(&(code as u32)));
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
                    dlog!("name: {}", pdf_to_utf8(name));
                } else {
                    dlog!("name not found");
                }

                encoding_table = Some(table);
            }
            _ => {
                panic!()
            }
        }

        let first_char: i64 = get(doc, font, b"FirstChar");
        let last_char: i64 = get(doc, font, b"LastChar");
        let widths: Vec<f64> = get(doc, font, b"Widths");

        let mut width_map = HashMap::new();

        let mut i = 0;
        dlog!(
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
            doc,
            font,
            widths: width_map,
            encoding: encoding_table,
            unicode_map,
        }
    }
}

type CharCode = u32;

struct PdfFontIter<'a> {
    i: Iter<'a, u8>,
    font: &'a dyn PdfFont,
}

impl<'a> Iterator for PdfFontIter<'a> {
    type Item = (CharCode, u8);
    fn next(&mut self) -> Option<(CharCode, u8)> {
        self.font.next_char(&mut self.i)
    }
}

trait PdfFont: Debug {
    fn get_width(&self, id: CharCode) -> f64;
    fn next_char(&self, iter: &mut Iter<u8>) -> Option<(CharCode, u8)>;
    fn decode_char(&self, char: CharCode) -> String;

    /*fn char_codes<'a>(&'a self, chars: &'a [u8]) -> PdfFontIter {
        let p = self;
        PdfFontIter{i: chars.iter(), font: p as &PdfFont}
    }*/
}

impl<'a> dyn PdfFont + 'a {
    fn char_codes(&'a self, chars: &'a [u8]) -> PdfFontIter<'a> {
        PdfFontIter {
            i: chars.iter(),
            font: self,
        }
    }
    fn decode(&self, chars: &[u8]) -> String {
        let strings = self
            .char_codes(chars)
            .map(|x| self.decode_char(x.0))
            .collect::<Vec<_>>();
        strings.join("")
    }
}

impl<'a> PdfFont for PdfSimpleFont<'a> {
    fn get_width(&self, id: CharCode) -> f64 {
        let width = self.widths.get(&id);
        if let Some(width) = width {
            return *width;
        } else {
            let mut widths = self.widths.iter().collect::<Vec<_>>();
            widths.sort_by_key(|x| x.0);
            dlog!(
                "missing width for {} len(widths) = {}, {:?} falling back to missing_width {:?}",
                id,
                self.widths.len(),
                widths,
                self.font
            );
            return self.missing_width;
        }
    }
    /*fn decode(&self, chars: &[u8]) -> String {
        let encoding = self.encoding.as_ref().map(|x| &x[..]).unwrap_or(&PDFDocEncoding);
        to_utf8(encoding, chars)
    }*/

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
                    // some pdf's like http://arxiv.org/pdf/2312.00064v1 are missing entries in their unicode map but do have
                    // entries in the encoding.
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
        //dlog!("char_code {:?} {:?}", char, self.encoding);
        let s = to_utf8(encoding, &slice);
        s
    }
}

impl<'a> fmt::Debug for PdfSimpleFont<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.font.fmt(f)
    }
}

impl<'a> PdfFont for PdfType3Font<'a> {
    fn get_width(&self, id: CharCode) -> f64 {
        let width = self.widths.get(&id);
        if let Some(width) = width {
            return *width;
        } else {
            panic!("missing width for {} {:?}", id, self.font);
        }
    }
    /*fn decode(&self, chars: &[u8]) -> String {
        let encoding = self.encoding.as_ref().map(|x| &x[..]).unwrap_or(&PDFDocEncoding);
        to_utf8(encoding, chars)
    }*/

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
                    // some pdf's like http://arxiv.org/pdf/2312.00577v1 are missing entries in their unicode map but do have
                    // entries in the encoding.
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
        //dlog!("char_code {:?} {:?}", char, self.encoding);
        let s = to_utf8(encoding, &slice);
        s
    }
}

impl<'a> fmt::Debug for PdfType3Font<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.font.fmt(f)
    }
}

struct PdfCIDFont<'a> {
    font: &'a Dictionary,
    #[allow(dead_code)]
    doc: &'a Document,
    #[allow(dead_code)]
    encoding: ByteMapping,
    to_unicode: Option<HashMap<u32, String>>,
    fallback_unicode: Option<HashMap<u32, String>>, // From embedded font cmap table
    width_fallback: Option<HashMap<u32, String>>,   // From system font width matching
    widths: HashMap<CharCode, f64>,                 // should probably just use i32 here
    default_width: Option<f64>, // only used for CID fonts and we should probably brake out the different font types
}

fn get_unicode_map<'a>(doc: &'a Document, font: &'a Dictionary) -> Option<HashMap<u32, String>> {
    let to_unicode = maybe_get_obj(doc, font, b"ToUnicode");
    dlog!("ToUnicode: {:?}", to_unicode);
    let mut unicode_map = None;
    match to_unicode {
        Some(&Object::Stream(ref stream)) => {
            let contents = get_contents(stream);
            dlog!("Stream: {}", String::from_utf8(contents.clone()).unwrap());

            let cmap = adobe_cmap_parser::get_unicode_map(&contents).unwrap();

            // Debug: Check what the parser returns for CID 1374
            if let Some(bytes_1374) = cmap.get(&1374) {
                debug!(
                    "adobe_cmap_parser returned for CID 1374: {:?} (bytes: {:02X?})",
                    bytes_1374, bytes_1374
                );
            } else {
                debug!("adobe_cmap_parser has NO entry for CID 1374");
            }

            let mut unicode = HashMap::new();
            // "It must use the beginbfchar, endbfchar, beginbfrange, and endbfrange operators to
            // define the mapping from character codes to Unicode character sequences expressed in
            // UTF-16BE encoding."
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
                        // this range is not specified as not being encoded
                        // we ignore them so we don't an error from from_utt16
                        continue;
                    }
                    _ => {}
                }
                let s = String::from_utf16(&be).unwrap();

                // Debug CID 1374 specifically
                if k == 1374 {
                    debug!(
                        "Processing CID 1374: raw bytes {:02X?} → UTF-16BE {:04X?} → string {:?}",
                        v, be, s
                    );
                }

                // Insert the mapping even if it's null - decode_char will detect and use fallback
                unicode.insert(k, s);
            }
            unicode_map = Some(unicode);

            dlog!("map: {:?}", unicode_map);
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

// Extract CID-to-Unicode mapping from embedded TrueType/OpenType font cmap table
fn get_fallback_unicode_from_font<'a>(
    doc: &'a Document,
    ciddict: &'a Dictionary,
) -> Option<HashMap<u32, String>> {
    let font_descriptor = maybe_get_obj(doc, ciddict, b"FontDescriptor")?;
    let font_descriptor = font_descriptor.as_dict().ok()?;

    // Check for CIDToGIDMap to convert CID -> GID
    let mut cid_to_gid: Option<HashMap<u32, u32>> = None;
    if let Some(cid_to_gid_map) = maybe_get_obj(doc, ciddict, b"CIDToGIDMap") {
        debug!("Found CIDToGIDMap object: {:?}", cid_to_gid_map);
        match cid_to_gid_map {
            &Object::Stream(ref stream) => {
                // CIDToGIDMap is a stream of 16-bit GID values indexed by CID
                let data = get_contents(stream);
                debug!("CIDToGIDMap stream has {} bytes", data.len());
                let mut map = HashMap::new();
                for (cid, chunk) in data.chunks_exact(2).enumerate() {
                    let gid = ((chunk[0] as u32) << 8) | (chunk[1] as u32);
                    if gid != 0 {
                        // 0 means .notdef, skip it
                        map.insert(cid as u32, gid);
                        // Debug first few and CID 1374
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
                    // No mapping needed, CID = GID
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

    // Try FontFile2 (TrueType) first
    let font_stream = maybe_get_obj(doc, font_descriptor, b"FontFile2")
        .or_else(|| maybe_get_obj(doc, font_descriptor, b"FontFile3"));

    if let Some(&Object::Stream(ref stream)) = font_stream {
        let font_data = get_contents(stream);

        // Parse the TrueType/OpenType font
        if let Ok(face) = ttf_parser::Face::parse(&font_data, 0) {
            let mut fallback_map = HashMap::new();

            // Debug: List ALL cmap subtables
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

            // Debug: Check font metrics
            let num_glyphs = face.number_of_glyphs();
            debug!("Font has {} total glyphs", num_glyphs);

            // Debug: Check if post table exists and get name for GID 1374
            if let Some(_post_table) = face.tables().post {
                debug!("Font has post table");
                // Check what GID 1374 is named
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

            // Debug: Check if this is a CFF font
            if let Some(_cff_table) = face.tables().cff {
                debug!("Font has CFF table (OpenType CFF font)");
                // Try to get glyph name from CFF
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

            // Iterate through all supported cmap subtables to build GID → Unicode mapping first
            for subtable in face.tables().cmap.iter().flat_map(|cmap| cmap.subtables) {
                // We want Unicode cmaps (platform 0 or platform 3 with encoding 1/10)
                let is_unicode = match (subtable.platform_id, subtable.encoding_id) {
                    (ttf_parser::PlatformId::Unicode, _) => true, // Unicode platform
                    (ttf_parser::PlatformId::Windows, 1) => true, // Windows platform, Unicode BMP
                    (ttf_parser::PlatformId::Windows, 10) => true, // Windows platform, Unicode full repertoire
                    _ => false,
                };

                if is_unicode {
                    // Build GID → Unicode map
                    let mut gid_to_unicode: HashMap<u32, String> = HashMap::new();
                    let mut sample_count = 0;
                    subtable.codepoints(|codepoint| {
                        if let Some(gid) = subtable.glyph_index(codepoint) {
                            if let Some(c) = char::from_u32(codepoint) {
                                gid_to_unicode.insert(gid.0 as u32, c.to_string());
                                // Debug: check for hyphen/minus characters
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
                                // Sample first few mappings
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

                    // Now build CID → Unicode using CIDToGIDMap if available
                    if let Some(ref cid_gid_map) = cid_to_gid {
                        // Use explicit CIDToGIDMap
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
                        // Assume Identity mapping (CID = GID)
                        for (&gid, unicode) in gid_to_unicode.iter() {
                            fallback_map.insert(gid, unicode.clone());
                        }
                        debug!(
                            "Built fallback map assuming Identity (CID=GID): {} entries",
                            fallback_map.len()
                        );
                        // Check if commonly problematic CID is in the map
                        if gid_to_unicode.contains_key(&1374) {
                            debug!("GID 1374 maps to: {:?}", gid_to_unicode.get(&1374));
                        } else {
                            debug!("GID 1374 NOT found in font cmap");
                        }
                    }

                    // We found a good Unicode cmap, use it
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

// Try to load system font and build width-based fallback mapping
fn get_width_fallback_from_system_font(
    font: &Dictionary,
    pdf_widths: &HashMap<u32, f64>,
) -> Option<HashMap<u32, String>> {
    // Get BaseFont name (e.g., "Inter-SemiBold")
    let base_name = if let Ok(Object::Name(name)) = font.get(b"BaseFont") {
        pdf_to_utf8(name)
    } else {
        return None;
    };

    debug!("Attempting to load system font for BaseFont: {}", base_name);

    // Try to find and load the system font
    let system_font_data = load_system_font(&base_name)?;
    let system_face = ttf_parser::Face::parse(&system_font_data, 0).ok()?;

    debug!(
        "Loaded system font: {} glyphs",
        system_face.number_of_glyphs()
    );

    // Build a map of normalized width → Vec<(char, unicode)> from system font
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

    // Now match PDF widths to system font characters
    let mut fallback_map = HashMap::new();
    const WIDTH_TOLERANCE_PERCENT: f64 = 2.0; // ±2%

    for (&cid, &pdf_width) in pdf_widths.iter() {
        let pdf_width_int = pdf_width as i32;
        let tolerance = ((pdf_width * WIDTH_TOLERANCE_PERCENT / 100.0) as i32).max(1);

        // Try to find matching width in system font (±tolerance)
        for width_offset in 0..=tolerance {
            for &sign in &[1, -1] {
                let test_width = pdf_width_int + (sign * width_offset);
                if let Some(chars) = width_to_chars.get(&test_width) {
                    // Found character(s) with matching width
                    // Prefer common ASCII characters (space, digits, punctuation)
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

// Load system font by name
fn load_system_font(font_name: &str) -> Option<Vec<u8>> {
    // Parse font name to extract family and style
    // E.g., "Inter-SemiBold" → family="Inter", style="SemiBold"
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

    // Try common font paths on macOS/Linux
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
        dlog!("base_name {} {:?}", base_name, font);

        let encoding = match encoding {
            &Object::Name(ref name) => {
                let name = pdf_to_utf8(name);
                dlog!("encoding {:?}", name);
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
                dlog!("Stream: {}", String::from_utf8(contents.clone()).unwrap());
                adobe_cmap_parser::get_byte_mapping(&contents).unwrap()
            }
            _ => {
                panic!("unsupported encoding {:?}", encoding)
            }
        };

        // Sometimes a Type0 font might refer to the same underlying data as regular font. In this case we may be able to extract some encoding
        // data.
        // We should also look inside the truetype data to see if there's a cmap table. It will help us convert as well.
        // This won't work if the cmap has been subsetted. A better approach might be to hash glyph contents and use that against
        // a global library of glyph hashes
        let unicode_map = get_unicode_map(doc, font);

        // Extract fallback Unicode mapping from embedded font's cmap table
        let fallback_unicode = get_fallback_unicode_from_font(doc, ciddict);

        dlog!("descendents {:?} {:?}", descendants, ciddict);

        let font_dict = maybe_get_obj(doc, ciddict, b"FontDescriptor").expect("required");
        dlog!("{:?}", font_dict);
        let _f = font_dict.as_dict().expect("must be dict");
        let default_width = get::<Option<i64>>(doc, ciddict, b"DW").unwrap_or(1000);
        let w: Option<Vec<&Object>> = get(doc, ciddict, b"W");
        dlog!("widths {:?}", w);
        let mut widths = HashMap::new();
        let mut i = 0;
        if let Some(w) = w {
            while i < w.len() {
                if let &Object::Array(ref wa) = w[i + 1] {
                    let cid = w[i].as_i64().expect("id should be num");
                    let mut j = 0;
                    dlog!("wa: {:?} -> {:?}", cid, wa);
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
            widths,
            to_unicode: unicode_map,
            fallback_unicode,
            width_fallback,
            encoding,
            default_width: Some(default_width as f64),
        }
    }
}

impl<'a> PdfFont for PdfCIDFont<'a> {
    fn get_width(&self, id: CharCode) -> f64 {
        let width = self.widths.get(&id);
        if let Some(width) = width {
            dlog!("GetWidth {} -> {}", id, *width);
            return *width;
        } else {
            dlog!("missing width for {} falling back to default_width", id);
            return self.default_width.unwrap();
        }
    } /*
    fn decode(&self, chars: &[u8]) -> String {
    self.char_codes(chars);

    //let utf16 = Vec::new();

    let encoding = self.encoding.as_ref().map(|x| &x[..]).unwrap_or(&PDFDocEncoding);
    to_utf8(encoding, chars)
    }*/

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
        // Try ToUnicode CMap first
        let s = self.to_unicode.as_ref().and_then(|x| x.get(&char));
        if let Some(s) = s {
            // Check if it's a null character or empty - these are faulty mappings
            if !s.is_empty() && !s.contains('\0') {
                return s.clone();
            }
            // ToUnicode gave us null or empty - fall through to try the fallback
            debug!(
                "ToUnicode returned null/empty for char {} (font: {:?}) - trying fallback",
                char,
                maybe_get_obj(self.doc, self.font, b"BaseFont")
            );
        }

        // Try fallback from embedded font's cmap table
        if let Some(ref fallback) = self.fallback_unicode {
            if let Some(s) = fallback.get(&char) {
                debug!(
                    "Using embedded font cmap fallback for char {}: {:?}",
                    char, s
                );
                return s.clone();
            }
        }

        // Try width-based fallback from system font
        if let Some(ref width_fallback) = self.width_fallback {
            if let Some(s) = width_fallback.get(&char) {
                debug!("Using width-based fallback for char {}: {:?}", char, s);
                return s.clone();
            }
        }

        // No mapping found in ToUnicode, embedded font cmap, or width fallback
        dlog!("Unknown character {} (no mapping found)", char);
        "".to_string()
    }
}

impl<'a> fmt::Debug for PdfCIDFont<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.font.fmt(f)
    }
}

#[derive(Copy, Clone)]
struct PdfFontDescriptor<'a> {
    desc: &'a Dictionary,
    doc: &'a Document,
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

#[derive(Clone, Debug)]
struct Type0Func {
    domain: Vec<f64>,
    range: Vec<f64>,
    contents: Vec<u8>,
    size: Vec<i64>,
    bits_per_sample: i64,
    encode: Vec<f64>,
    decode: Vec<f64>,
}

#[allow(dead_code)]
fn interpolate(x: f64, x_min: f64, _x_max: f64, y_min: f64, y_max: f64) -> f64 {
    let divisor = x - x_min;
    if divisor != 0. {
        y_min + (x - x_min) * ((y_max - y_min) / divisor)
    } else {
        // (x - x_min) will be 0 which means we want to discard the interpolation
        // and arbitrarily choose y_min to match pdfium
        y_min
    }
}

impl Type0Func {
    #[allow(dead_code)]
    fn eval(&self, _input: &[f64], _output: &mut [f64]) {
        let _n_inputs = self.domain.len() / 2;
        let _n_ouputs = self.range.len() / 2;
    }
}

#[derive(Clone, Debug)]
struct Type2Func {
    c0: Option<Vec<f64>>,
    c1: Option<Vec<f64>>,
    n: f64,
}

#[derive(Clone, Debug)]
enum Function {
    Type0(Type0Func),
    Type2(Type2Func),
    #[allow(dead_code)]
    Type3,
    #[allow(dead_code)]
    Type4(Vec<u8>),
}

impl Function {
    fn new(doc: &Document, obj: &Object) -> Function {
        let dict = match obj {
            &Object::Dictionary(ref dict) => dict,
            &Object::Stream(ref stream) => &stream.dict,
            _ => panic!(),
        };
        let function_type: i64 = get(doc, dict, b"FunctionType");
        let f = match function_type {
            0 => {
                // Sampled function
                let stream = match obj {
                    &Object::Stream(ref stream) => stream,
                    _ => panic!(),
                };
                let range: Vec<f64> = get(doc, dict, b"Range");
                let domain: Vec<f64> = get(doc, dict, b"Domain");
                let contents = get_contents(stream);
                let size: Vec<i64> = get(doc, dict, b"Size");
                let bits_per_sample = get(doc, dict, b"BitsPerSample");
                // We ignore 'Order' like pdfium, poppler and pdf.js

                let encode = get::<Option<Vec<f64>>>(doc, dict, b"Encode");
                // maybe there's some better way to write this.
                let encode = encode.unwrap_or_else(|| {
                    let mut default = Vec::new();
                    for i in &size {
                        default.extend([0., (i - 1) as f64].iter());
                    }
                    default
                });
                let decode =
                    get::<Option<Vec<f64>>>(doc, dict, b"Decode").unwrap_or_else(|| range.clone());

                Function::Type0(Type0Func {
                    domain,
                    range,
                    size,
                    contents,
                    bits_per_sample,
                    encode,
                    decode,
                })
            }
            2 => {
                // Exponential interpolation function
                let c0 = get::<Option<Vec<f64>>>(doc, dict, b"C0");
                let c1 = get::<Option<Vec<f64>>>(doc, dict, b"C1");
                let n = get::<f64>(doc, dict, b"N");
                Function::Type2(Type2Func { c0, c1, n })
            }
            3 => {
                // Stitching function
                Function::Type3
            }
            4 => {
                // PostScript calculator function
                let contents = match obj {
                    &Object::Stream(ref stream) => {
                        let contents = get_contents(stream);
                        warn!("unhandled type-4 function");
                        warn!("Stream: {}", String::from_utf8(contents.clone()).unwrap());
                        contents
                    }
                    _ => {
                        panic!("type 4 functions should be streams")
                    }
                };
                Function::Type4(contents)
            }
            _ => {
                panic!("unhandled function type {}", function_type)
            }
        };
        f
    }
}

fn as_num(o: &Object) -> f64 {
    match o {
        &Object::Integer(i) => i as f64,
        &Object::Real(f) => f.into(),
        _ => {
            panic!("not a number")
        }
    }
}

#[derive(Clone)]
struct TextState<'a> {
    font: Option<Rc<dyn PdfFont + 'a>>,
    font_size: f64,
    character_spacing: f64,
    word_spacing: f64,
    horizontal_scaling: f64,
    leading: f64,
    rise: f64,
    tm: Transform,
}

// XXX: We'd ideally implement this without having to copy the uncompressed data
fn get_contents(contents: &Stream) -> Vec<u8> {
    if contents.filters().is_ok() {
        contents
            .decompressed_content()
            .unwrap_or_else(|_| contents.content.clone())
    } else {
        contents.content.clone()
    }
}

#[derive(Clone)]
struct GraphicsState<'a> {
    ctm: Transform,
    ts: TextState<'a>,
    smask: Option<Dictionary>,
    fill_colorspace: ColorSpace,
    fill_color: Vec<f64>,
    stroke_colorspace: ColorSpace,
    stroke_color: Vec<f64>,
    line_width: f64,
}

fn show_text(
    gs: &mut GraphicsState,
    s: &[u8],
    _tlm: &Transform,
    _flip_ctm: &Transform,
    output: &mut dyn OutputDev,
) -> Result<(), OutputError> {
    let ts = &mut gs.ts;
    let font = ts.font.as_ref().unwrap();
    //let encoding = font.encoding.as_ref().map(|x| &x[..]).unwrap_or(&PDFDocEncoding);
    dlog!("{:?}", font.decode(s));
    dlog!("{:?}", font.decode(s).as_bytes());
    dlog!("{:?}", s);
    output.begin_word()?;

    for (c, length) in font.char_codes(s) {
        // 5.3.3 Text Space Details
        let tsm = Transform2D::row_major(ts.horizontal_scaling, 0., 0., 1.0, 0., ts.rise);
        // Trm = Tsm × Tm × CTM
        let trm = tsm.post_transform(&ts.tm.post_transform(&gs.ctm));
        //dlog!("ctm: {:?} tm {:?}", gs.ctm, tm);
        //dlog!("current pos: {:?}", position);
        // 5.9 Extraction of Text Content

        //dlog!("w: {}", font.widths[&(*c as i64)]);
        let w0 = font.get_width(c) / 1000.;

        let mut spacing = ts.character_spacing;
        // "Word spacing is applied to every occurrence of the single-byte character code 32 in a
        //  string when using a simple font or a composite font that defines code 32 as a
        //  single-byte code. It does not apply to occurrences of the byte value 32 in
        //  multiple-byte codes."
        let is_space = c == 32 && length == 1;
        if is_space {
            spacing += ts.word_spacing
        }

        output.output_character(&trm, w0, spacing, ts.font_size, &font.decode_char(c))?;
        let tj = 0.;
        let ty = 0.;
        let tx = ts.horizontal_scaling * ((w0 - tj / 1000.) * ts.font_size + spacing);
        dlog!(
            "horizontal {} adjust {} {} {} {}",
            ts.horizontal_scaling,
            tx,
            w0,
            ts.font_size,
            spacing
        );
        // dlog!("w0: {}, tx: {}", w0, tx);
        ts.tm = ts
            .tm
            .pre_transform(&Transform2D::create_translation(tx, ty));
        let _trm = ts.tm.pre_transform(&gs.ctm);
        //dlog!("post pos: {:?}", trm);
    }
    output.end_word()?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct MediaBox {
    pub llx: f64,
    pub lly: f64,
    pub urx: f64,
    pub ury: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone)]
pub struct BoundingBox {
    pub top_left: Point,
    pub top_right: Point,
    pub bottom_left: Point,
    pub bottom_right: Point,
}

#[derive(Debug, Clone)]
pub struct TextSpan {
    pub text: String,
    pub bbox: BoundingBox,
    pub font_size: f64,
    pub page_num: u32,
}

impl TextSpan {
    // Standard monospace character width in PDF points.
    // This maps PDF coordinates to a character grid for terminal output.
    // Adjust this constant if the output spacing doesn't match your terminal.
    pub const MONOSPACE_CHAR_WIDTH_POINTS: f64 = 4.0;

    /// Convert a PDF x-coordinate (in points) to a character grid column number.
    /// The grid starts at column 0 for x=0.
    pub fn x_to_col(x: f64) -> usize {
        (x / Self::MONOSPACE_CHAR_WIDTH_POINTS).round() as usize
    }

    /// Get the starting column position for this span in the character grid.
    pub fn start_col(&self) -> usize {
        Self::x_to_col(self.bbox.bottom_left.x)
    }

    /// Get the ending column position for this span in the character grid.
    pub fn end_col(&self) -> usize {
        Self::x_to_col(self.bbox.bottom_right.x)
    }

    /// Get the width of this span in character grid cells.
    /// Returns at least the length of the text to avoid truncation.
    pub fn grid_width(&self) -> usize {
        let bbox_width = self.end_col().saturating_sub(self.start_col());
        bbox_width.max(self.text.chars().count())
    }

    /// Check if this span belongs to a right-aligned column.
    /// Returns true if the span's right edge is close to any of the detected right-aligned positions.
    pub fn is_right_aligned(&self, right_aligned_positions: &[f64], threshold: f64) -> bool {
        let right_x = self.bbox.bottom_right.x;
        right_aligned_positions
            .iter()
            .any(|&pos_x| (right_x - pos_x).abs() < threshold)
    }
}

pub type TextLine = Vec<TextSpan>;
pub type TextPage = Vec<TextLine>;

/// Detect right-aligned columns across all lines by finding clusters of spans
/// with similar right-edge coordinates but varying left-edge coordinates.
/// Returns a vector of x-coordinates (in PDF points) that represent right-aligned column positions.
pub fn detect_right_aligned_columns(lines: &TextPage) -> Vec<f64> {
    use std::collections::HashMap;

    // Threshold for clustering right edges (in PDF points)
    const CLUSTER_THRESHOLD: f64 = 8.0;
    // Minimum number of spans needed to consider a column as right-aligned
    const MIN_SPANS_FOR_COLUMN: usize = 3;
    // Minimum variation in left edges to consider a column as right-aligned (not left-aligned)
    // Must be large enough to exclude justified text columns but catch true right-aligned data
    const MIN_LEFT_VARIATION: f64 = 50.0;
    // Minimum x-position to consider as a table column (not left-margin content)
    const MIN_COLUMN_POSITION: f64 = 200.0;

    #[derive(Clone)]
    struct SpanEdges {
        left_x: f64,
        right_x: f64,
    }

    // Collect all span edges
    let mut all_edges: Vec<SpanEdges> = Vec::new();
    for line in lines {
        for span in line {
            all_edges.push(SpanEdges {
                left_x: span.bbox.bottom_left.x,
                right_x: span.bbox.bottom_right.x,
            });
        }
    }

    if all_edges.is_empty() {
        return Vec::new();
    }

    // Cluster the right edges using a simple agglomerative approach
    let mut clusters: HashMap<usize, Vec<SpanEdges>> = HashMap::new();
    let mut cluster_id = 0;

    for edges in all_edges {
        // Find if this edge belongs to an existing cluster by checking distance to cluster center
        let mut found_cluster = None;
        let mut min_distance = f64::MAX;

        for (id, cluster) in clusters.iter() {
            // Calculate cluster center for right edges
            let center = cluster.iter().map(|e| e.right_x).sum::<f64>() / cluster.len() as f64;
            let distance = (edges.right_x - center).abs();

            if distance < CLUSTER_THRESHOLD && distance < min_distance {
                found_cluster = Some(*id);
                min_distance = distance;
            }
        }

        if let Some(id) = found_cluster {
            clusters.get_mut(&id).unwrap().push(edges);
        } else {
            // Create new cluster
            clusters.insert(cluster_id, vec![edges]);
            cluster_id += 1;
        }
    }

    // Find clusters with enough spans AND varying left edges (indicating right-alignment)
    let mut right_aligned_positions = Vec::new();
    for cluster in clusters.values() {
        if cluster.len() >= MIN_SPANS_FOR_COLUMN {
            // Calculate variation in left edges
            let left_edges: Vec<f64> = cluster.iter().map(|e| e.left_x).collect();
            let min_left = left_edges.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_left = left_edges.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let left_variation = max_left - min_left;

            // Calculate variation in right edges
            let right_edges: Vec<f64> = cluster.iter().map(|e| e.right_x).collect();
            let min_right = right_edges.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_right = right_edges
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max);
            let right_variation = max_right - min_right;

            let avg_right_x = cluster.iter().map(|e| e.right_x).sum::<f64>() / cluster.len() as f64;

            // Only consider it right-aligned if:
            // 1. Left edges vary (not perfectly left-aligned or justified)
            // 2. Right edges are consistent (not left-aligned paragraphs)
            // 3. Position is far enough right to be a table column (not left-margin content)
            // If both left and right are consistent, it's justified text (treat as left-aligned)
            const MAX_RIGHT_VARIATION: f64 = 3.7;
            // Columns far to the right are likely table data (use lower threshold)
            const FAR_RIGHT_POSITION: f64 = 450.0;
            const MIN_LEFT_VARIATION_FAR_RIGHT: f64 = 5.0;

            // Use position-based threshold: columns far to the right need less left variation
            let left_variation_threshold = if avg_right_x >= FAR_RIGHT_POSITION {
                MIN_LEFT_VARIATION_FAR_RIGHT
            } else {
                MIN_LEFT_VARIATION
            };

            if left_variation >= left_variation_threshold
                && right_variation < MAX_RIGHT_VARIATION
                && avg_right_x >= MIN_COLUMN_POSITION
            {
                right_aligned_positions.push(avg_right_x);
            }
        }
    }

    right_aligned_positions
}

fn apply_state(doc: &Document, gs: &mut GraphicsState, state: &Dictionary) {
    for (k, v) in state.iter() {
        let k: &[u8] = k.as_ref();
        match k {
            b"SMask" => match maybe_deref(doc, v) {
                &Object::Name(ref name) => {
                    if name == b"None" {
                        gs.smask = None;
                    } else {
                        panic!("unexpected smask name")
                    }
                }
                &Object::Dictionary(ref dict) => {
                    gs.smask = Some(dict.clone());
                }
                _ => {
                    panic!("unexpected smask type {:?}", v)
                }
            },
            b"Type" => match v {
                &Object::Name(ref name) => {
                    assert_eq!(name, b"ExtGState")
                }
                _ => {
                    panic!("unexpected type")
                }
            },
            _ => {
                dlog!("unapplied state: {:?} {:?}", k, v);
            }
        }
    }
}

#[derive(Debug)]
pub enum PathOp {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    // XXX: is it worth distinguishing the different kinds of curve ops?
    CurveTo(f64, f64, f64, f64, f64, f64),
    Rect(f64, f64, f64, f64),
    Close,
}

#[derive(Debug)]
pub struct Path {
    pub ops: Vec<PathOp>,
}

impl Path {
    fn new() -> Path {
        Path { ops: Vec::new() }
    }
    fn current_point(&self) -> (f64, f64) {
        match self.ops.last().unwrap() {
            &PathOp::MoveTo(x, y) => (x, y),
            &PathOp::LineTo(x, y) => (x, y),
            &PathOp::CurveTo(_, _, _, _, x, y) => (x, y),
            _ => {
                panic!()
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct CalGray {
    white_point: [f64; 3],
    black_point: Option<[f64; 3]>,
    gamma: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct CalRGB {
    white_point: [f64; 3],
    black_point: Option<[f64; 3]>,
    gamma: Option<[f64; 3]>,
    matrix: Option<Vec<f64>>,
}

#[derive(Clone, Debug)]
pub struct Lab {
    white_point: [f64; 3],
    black_point: Option<[f64; 3]>,
    range: Option<[f64; 4]>,
}

#[derive(Clone, Debug)]
pub enum AlternateColorSpace {
    DeviceGray,
    DeviceRGB,
    DeviceCMYK,
    CalRGB(CalRGB),
    CalGray(CalGray),
    Lab(Lab),
    ICCBased(Vec<u8>),
}

#[derive(Clone)]
pub struct Separation {
    name: String,
    alternate_space: AlternateColorSpace,
    tint_transform: Box<Function>,
}

#[derive(Clone)]
pub enum ColorSpace {
    DeviceGray,
    DeviceRGB,
    DeviceCMYK,
    DeviceN,
    Pattern,
    CalRGB(CalRGB),
    CalGray(CalGray),
    Lab(Lab),
    Separation(Separation),
    ICCBased(Vec<u8>),
}

fn make_colorspace<'a>(doc: &'a Document, name: &[u8], resources: &'a Dictionary) -> ColorSpace {
    match name {
        b"DeviceGray" => ColorSpace::DeviceGray,
        b"DeviceRGB" => ColorSpace::DeviceRGB,
        b"DeviceCMYK" => ColorSpace::DeviceCMYK,
        b"Pattern" => ColorSpace::Pattern,
        _ => {
            let colorspaces: &Dictionary = get(&doc, resources, b"ColorSpace");
            let cs: &Object = maybe_get_obj(doc, colorspaces, &name[..])
                .unwrap_or_else(|| panic!("missing colorspace {:?}", &name[..]));
            if let Ok(cs) = cs.as_array() {
                let cs_name = pdf_to_utf8(cs[0].as_name().expect("first arg must be a name"));
                match cs_name.as_ref() {
                    "Separation" => {
                        let name = pdf_to_utf8(cs[1].as_name().expect("second arg must be a name"));
                        let alternate_space = match &maybe_deref(doc, &cs[2]) {
                            Object::Name(name) => match &name[..] {
                                b"DeviceGray" => AlternateColorSpace::DeviceGray,
                                b"DeviceRGB" => AlternateColorSpace::DeviceRGB,
                                b"DeviceCMYK" => AlternateColorSpace::DeviceCMYK,
                                _ => panic!("unexpected color space name"),
                            },
                            Object::Array(cs) => {
                                let cs_name =
                                    pdf_to_utf8(cs[0].as_name().expect("first arg must be a name"));
                                match cs_name.as_ref() {
                                    "ICCBased" => {
                                        let stream = maybe_deref(doc, &cs[1]).as_stream().unwrap();
                                        dlog!("ICCBased {:?}", stream);
                                        // XXX: we're going to be continually decompressing everytime this object is referenced
                                        AlternateColorSpace::ICCBased(get_contents(stream))
                                    }
                                    "CalGray" => {
                                        let dict =
                                            cs[1].as_dict().expect("second arg must be a dict");
                                        AlternateColorSpace::CalGray(CalGray {
                                            white_point: get(&doc, dict, b"WhitePoint"),
                                            black_point: get(&doc, dict, b"BackPoint"),
                                            gamma: get(&doc, dict, b"Gamma"),
                                        })
                                    }
                                    "CalRGB" => {
                                        let dict =
                                            cs[1].as_dict().expect("second arg must be a dict");
                                        AlternateColorSpace::CalRGB(CalRGB {
                                            white_point: get(&doc, dict, b"WhitePoint"),
                                            black_point: get(&doc, dict, b"BackPoint"),
                                            gamma: get(&doc, dict, b"Gamma"),
                                            matrix: get(&doc, dict, b"Matrix"),
                                        })
                                    }
                                    "Lab" => {
                                        let dict =
                                            cs[1].as_dict().expect("second arg must be a dict");
                                        AlternateColorSpace::Lab(Lab {
                                            white_point: get(&doc, dict, b"WhitePoint"),
                                            black_point: get(&doc, dict, b"BackPoint"),
                                            range: get(&doc, dict, b"Range"),
                                        })
                                    }
                                    _ => panic!("Unexpected color space name"),
                                }
                            }
                            _ => panic!("Alternate space should be name or array {:?}", cs[2]),
                        };
                        let tint_transform = Box::new(Function::new(doc, maybe_deref(doc, &cs[3])));

                        dlog!("{:?} {:?} {:?}", name, alternate_space, tint_transform);
                        ColorSpace::Separation(Separation {
                            name,
                            alternate_space,
                            tint_transform,
                        })
                    }
                    "ICCBased" => {
                        let stream = maybe_deref(doc, &cs[1]).as_stream().unwrap();
                        dlog!("ICCBased {:?}", stream);
                        // XXX: we're going to be continually decompressing everytime this object is referenced
                        ColorSpace::ICCBased(get_contents(stream))
                    }
                    "CalGray" => {
                        let dict = cs[1].as_dict().expect("second arg must be a dict");
                        ColorSpace::CalGray(CalGray {
                            white_point: get(&doc, dict, b"WhitePoint"),
                            black_point: get(&doc, dict, b"BackPoint"),
                            gamma: get(&doc, dict, b"Gamma"),
                        })
                    }
                    "CalRGB" => {
                        let dict = cs[1].as_dict().expect("second arg must be a dict");
                        ColorSpace::CalRGB(CalRGB {
                            white_point: get(&doc, dict, b"WhitePoint"),
                            black_point: get(&doc, dict, b"BackPoint"),
                            gamma: get(&doc, dict, b"Gamma"),
                            matrix: get(&doc, dict, b"Matrix"),
                        })
                    }
                    "Lab" => {
                        let dict = cs[1].as_dict().expect("second arg must be a dict");
                        ColorSpace::Lab(Lab {
                            white_point: get(&doc, dict, b"WhitePoint"),
                            black_point: get(&doc, dict, b"BackPoint"),
                            range: get(&doc, dict, b"Range"),
                        })
                    }
                    "Pattern" => ColorSpace::Pattern,
                    "DeviceGray" => ColorSpace::DeviceGray,
                    "DeviceRGB" => ColorSpace::DeviceRGB,
                    "DeviceCMYK" => ColorSpace::DeviceCMYK,
                    "DeviceN" => ColorSpace::DeviceN,
                    _ => {
                        panic!("color_space {:?} {:?} {:?}", name, cs_name, cs)
                    }
                }
            } else if let Ok(cs) = cs.as_name() {
                match pdf_to_utf8(cs).as_ref() {
                    "DeviceRGB" => ColorSpace::DeviceRGB,
                    "DeviceGray" => ColorSpace::DeviceGray,
                    _ => panic!(),
                }
            } else {
                panic!();
            }
        }
    }
}

struct Processor<'a> {
    _none: PhantomData<&'a ()>,
}

impl<'a> Processor<'a> {
    fn new() -> Processor<'a> {
        Processor { _none: PhantomData }
    }

    fn process_stream(
        &mut self,
        doc: &'a Document,
        content: Vec<u8>,
        resources: &'a Dictionary,
        media_box: &MediaBox,
        output: &mut dyn OutputDev,
        page_num: u32,
    ) -> Result<(), OutputError> {
        let content = Content::decode(&content).unwrap();
        let mut font_table = HashMap::new();
        let mut gs: GraphicsState = GraphicsState {
            ts: TextState {
                font: None,
                font_size: std::f64::NAN,
                character_spacing: 0.,
                word_spacing: 0.,
                horizontal_scaling: 100. / 100.,
                leading: 0.,
                rise: 0.,
                tm: Transform2D::identity(),
            },
            fill_color: Vec::new(),
            fill_colorspace: ColorSpace::DeviceGray,
            stroke_color: Vec::new(),
            stroke_colorspace: ColorSpace::DeviceGray,
            line_width: 1.,
            ctm: Transform2D::identity(),
            smask: None,
        };
        //let mut ts = &mut gs.ts;
        let mut gs_stack = Vec::new();
        let mut mc_stack = Vec::new();
        // XXX: replace tlm with a point for text start
        let mut tlm = Transform2D::identity();
        let mut path = Path::new();
        let flip_ctm = Transform2D::row_major(1., 0., 0., -1., 0., media_box.ury - media_box.lly);
        dlog!("MediaBox {:?}", media_box);
        for operation in &content.operations {
            //dlog!("op: {:?}", operation);

            match operation.operator.as_ref() {
                "BT" => {
                    tlm = Transform2D::identity();
                    gs.ts.tm = tlm;
                }
                "ET" => {
                    tlm = Transform2D::identity();
                    gs.ts.tm = tlm;
                }
                "cm" => {
                    assert!(operation.operands.len() == 6);
                    let m = Transform2D::row_major(
                        as_num(&operation.operands[0]),
                        as_num(&operation.operands[1]),
                        as_num(&operation.operands[2]),
                        as_num(&operation.operands[3]),
                        as_num(&operation.operands[4]),
                        as_num(&operation.operands[5]),
                    );
                    gs.ctm = gs.ctm.pre_transform(&m);
                    dlog!("matrix {:?}", gs.ctm);
                }
                "CS" => {
                    let name = operation.operands[0].as_name().unwrap();
                    gs.stroke_colorspace = make_colorspace(doc, name, resources);
                }
                "cs" => {
                    let name = operation.operands[0].as_name().unwrap();
                    gs.fill_colorspace = make_colorspace(doc, name, resources);
                }
                "SC" | "SCN" => {
                    gs.stroke_color = match gs.stroke_colorspace {
                        ColorSpace::Pattern => {
                            dlog!("unhandled pattern color");
                            Vec::new()
                        }
                        _ => operation.operands.iter().map(|x| as_num(x)).collect(),
                    };
                }
                "sc" | "scn" => {
                    gs.fill_color = match gs.fill_colorspace {
                        ColorSpace::Pattern => {
                            dlog!("unhandled pattern color");
                            Vec::new()
                        }
                        _ => operation.operands.iter().map(|x| as_num(x)).collect(),
                    };
                }
                "G" | "g" | "RG" | "rg" | "K" | "k" => {
                    dlog!("unhandled color operation {:?}", operation);
                }
                "TJ" => match operation.operands[0] {
                    Object::Array(ref array) => {
                        for e in array {
                            match e {
                                &Object::String(ref s, _) => {
                                    show_text(&mut gs, s, &tlm, &flip_ctm, output)?;
                                }
                                &Object::Integer(i) => {
                                    let ts = &mut gs.ts;
                                    let w0 = 0.;
                                    let tj = i as f64;
                                    let ty = 0.;
                                    let tx =
                                        ts.horizontal_scaling * ((w0 - tj / 1000.) * ts.font_size);
                                    ts.tm = ts
                                        .tm
                                        .pre_transform(&Transform2D::create_translation(tx, ty));
                                    dlog!("adjust text by: {} {:?}", i, ts.tm);
                                }
                                &Object::Real(i) => {
                                    let ts = &mut gs.ts;
                                    let w0 = 0.;
                                    let tj = i as f64;
                                    let ty = 0.;
                                    let tx =
                                        ts.horizontal_scaling * ((w0 - tj / 1000.) * ts.font_size);
                                    ts.tm = ts
                                        .tm
                                        .pre_transform(&Transform2D::create_translation(tx, ty));
                                    dlog!("adjust text by: {} {:?}", i, ts.tm);
                                }
                                _ => {
                                    dlog!("kind of {:?}", e);
                                }
                            }
                        }
                    }
                    _ => {}
                },
                "Tj" => match operation.operands[0] {
                    Object::String(ref s, _) => {
                        show_text(&mut gs, s, &tlm, &flip_ctm, output)?;
                    }
                    _ => {
                        panic!("unexpected Tj operand {:?}", operation)
                    }
                },
                "Tc" => {
                    gs.ts.character_spacing = as_num(&operation.operands[0]);
                }
                "Tw" => {
                    gs.ts.word_spacing = as_num(&operation.operands[0]);
                }
                "Tz" => {
                    gs.ts.horizontal_scaling = as_num(&operation.operands[0]) / 100.;
                }
                "TL" => {
                    gs.ts.leading = as_num(&operation.operands[0]);
                }
                "Tf" => {
                    let fonts: &Dictionary = get(&doc, resources, b"Font");
                    let name = operation.operands[0].as_name().unwrap();
                    let font = font_table
                        .entry(name.to_owned())
                        .or_insert_with(|| make_font(doc, get::<&Dictionary>(doc, fonts, name)))
                        .clone();
                    {
                        /*let file = font.get_descriptor().and_then(|desc| desc.get_file());
                        if let Some(file) = file {
                            let file_contents = filter_data(file.as_stream().unwrap());
                            let mut cursor = Cursor::new(&file_contents[..]);
                            //let f = Font::read(&mut cursor);
                            //dlog!("font file: {:?}", f);
                        }*/
                    }
                    gs.ts.font = Some(font);

                    gs.ts.font_size = as_num(&operation.operands[1]);
                    dlog!(
                        "font {} size: {} {:?}",
                        pdf_to_utf8(name),
                        gs.ts.font_size,
                        operation
                    );
                }
                "Ts" => {
                    gs.ts.rise = as_num(&operation.operands[0]);
                }
                "Tm" => {
                    assert!(operation.operands.len() == 6);
                    tlm = Transform2D::row_major(
                        as_num(&operation.operands[0]),
                        as_num(&operation.operands[1]),
                        as_num(&operation.operands[2]),
                        as_num(&operation.operands[3]),
                        as_num(&operation.operands[4]),
                        as_num(&operation.operands[5]),
                    );
                    gs.ts.tm = tlm;
                    dlog!("Tm: matrix {:?}", gs.ts.tm);
                    output.end_line()?;
                }
                "Td" => {
                    /* Move to the start of the next line, offset from the start of the current line by (tx , ty ).
                      tx and ty are numbers expressed in unscaled text space units.
                      More precisely, this operator performs the following assignments:
                    */
                    assert!(operation.operands.len() == 2);
                    let tx = as_num(&operation.operands[0]);
                    let ty = as_num(&operation.operands[1]);
                    dlog!("translation: {} {}", tx, ty);

                    tlm = tlm.pre_transform(&Transform2D::create_translation(tx, ty));
                    gs.ts.tm = tlm;
                    dlog!("Td matrix {:?}", gs.ts.tm);
                    output.end_line()?;
                }

                "TD" => {
                    /* Move to the start of the next line, offset from the start of the current line by (tx , ty ).
                      As a side effect, this operator sets the leading parameter in the text state.
                    */
                    assert!(operation.operands.len() == 2);
                    let tx = as_num(&operation.operands[0]);
                    let ty = as_num(&operation.operands[1]);
                    dlog!("translation: {} {}", tx, ty);
                    gs.ts.leading = -ty;

                    tlm = tlm.pre_transform(&Transform2D::create_translation(tx, ty));
                    gs.ts.tm = tlm;
                    dlog!("TD matrix {:?}", gs.ts.tm);
                    output.end_line()?;
                }

                "T*" => {
                    let tx = 0.0;
                    let ty = -gs.ts.leading;

                    tlm = tlm.pre_transform(&Transform2D::create_translation(tx, ty));
                    gs.ts.tm = tlm;
                    dlog!("T* matrix {:?}", gs.ts.tm);
                    output.end_line()?;
                }
                "q" => {
                    gs_stack.push(gs.clone());
                }
                "Q" => {
                    let s = gs_stack.pop();
                    if let Some(s) = s {
                        gs = s;
                    } else {
                        warn!("No state to pop");
                    }
                }
                "gs" => {
                    let ext_gstate: &Dictionary = get(doc, resources, b"ExtGState");
                    let name = operation.operands[0].as_name().unwrap();
                    let state: &Dictionary = get(doc, ext_gstate, name);
                    apply_state(doc, &mut gs, state);
                }
                "i" => {
                    dlog!(
                        "unhandled graphics state flattness operator {:?}",
                        operation
                    );
                }
                "w" => {
                    gs.line_width = as_num(&operation.operands[0]);
                }
                "J" | "j" | "M" | "d" | "ri" => {
                    dlog!("unknown graphics state operator {:?}", operation);
                }
                "m" => path.ops.push(PathOp::MoveTo(
                    as_num(&operation.operands[0]),
                    as_num(&operation.operands[1]),
                )),
                "l" => path.ops.push(PathOp::LineTo(
                    as_num(&operation.operands[0]),
                    as_num(&operation.operands[1]),
                )),
                "c" => path.ops.push(PathOp::CurveTo(
                    as_num(&operation.operands[0]),
                    as_num(&operation.operands[1]),
                    as_num(&operation.operands[2]),
                    as_num(&operation.operands[3]),
                    as_num(&operation.operands[4]),
                    as_num(&operation.operands[5]),
                )),
                "v" => {
                    let (x, y) = path.current_point();
                    path.ops.push(PathOp::CurveTo(
                        x,
                        y,
                        as_num(&operation.operands[0]),
                        as_num(&operation.operands[1]),
                        as_num(&operation.operands[2]),
                        as_num(&operation.operands[3]),
                    ))
                }
                "y" => path.ops.push(PathOp::CurveTo(
                    as_num(&operation.operands[0]),
                    as_num(&operation.operands[1]),
                    as_num(&operation.operands[2]),
                    as_num(&operation.operands[3]),
                    as_num(&operation.operands[2]),
                    as_num(&operation.operands[3]),
                )),
                "h" => path.ops.push(PathOp::Close),
                "re" => path.ops.push(PathOp::Rect(
                    as_num(&operation.operands[0]),
                    as_num(&operation.operands[1]),
                    as_num(&operation.operands[2]),
                    as_num(&operation.operands[3]),
                )),
                "s" | "f*" | "B" | "B*" | "b" => {
                    dlog!("unhandled path op {:?}", operation);
                }
                "S" => {
                    output.stroke(&gs.ctm, &gs.stroke_colorspace, &gs.stroke_color, &path)?;
                    path.ops.clear();
                }
                "F" | "f" => {
                    output.fill(&gs.ctm, &gs.fill_colorspace, &gs.fill_color, &path)?;
                    path.ops.clear();
                }
                "W" | "w*" => {
                    dlog!("unhandled clipping operation {:?}", operation);
                }
                "n" => {
                    dlog!("discard {:?}", path);
                    path.ops.clear();
                }
                "BMC" | "BDC" => {
                    mc_stack.push(operation);
                }
                "EMC" => {
                    mc_stack.pop();
                }
                "Do" => {
                    // `Do` process an entire subdocument, so we do a recursive call to `process_stream`
                    // with the subdocument content and resources
                    let xobject: &Dictionary = get(&doc, resources, b"XObject");
                    let name = operation.operands[0].as_name().unwrap();
                    let xf: &Stream = get(&doc, xobject, name);
                    let resources = maybe_get_obj(&doc, &xf.dict, b"Resources")
                        .and_then(|n| n.as_dict().ok())
                        .unwrap_or(resources);
                    let contents = get_contents(xf);
                    self.process_stream(&doc, contents, resources, &media_box, output, page_num)?;
                }
                _ => {
                    dlog!("unknown operation {:?}", operation);
                }
            }
        }
        Ok(())
    }
}

pub trait OutputDev {
    fn begin_page(
        &mut self,
        page_num: u32,
        media_box: &MediaBox,
        art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError>;
    fn end_page(&mut self) -> Result<(), OutputError>;
    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        spacing: f64,
        font_size: f64,
        char: &str,
    ) -> Result<(), OutputError>;
    fn begin_word(&mut self) -> Result<(), OutputError>;
    fn end_word(&mut self) -> Result<(), OutputError>;
    fn end_line(&mut self) -> Result<(), OutputError>;
    fn stroke(
        &mut self,
        _ctm: &Transform,
        _colorspace: &ColorSpace,
        _color: &[f64],
        _path: &Path,
    ) -> Result<(), OutputError> {
        Ok(())
    }
    fn fill(
        &mut self,
        _ctm: &Transform,
        _colorspace: &ColorSpace,
        _color: &[f64],
        _path: &Path,
    ) -> Result<(), OutputError> {
        Ok(())
    }
}

type ArtBox = (f64, f64, f64, f64);

/*
File doesn't implement std::fmt::Write so we have
to do some gymnastics to accept a File or String
See https://github.com/rust-lang/rust/issues/51305
*/

pub trait ConvertToFmt {
    type Writer: std::fmt::Write;
    fn convert(self) -> Self::Writer;
}

impl<'a> ConvertToFmt for &'a mut String {
    type Writer = &'a mut String;
    fn convert(self) -> Self::Writer {
        self
    }
}

pub struct WriteAdapter<W> {
    f: W,
}

impl<W: std::io::Write> std::fmt::Write for WriteAdapter<W> {
    fn write_str(&mut self, s: &str) -> Result<(), std::fmt::Error> {
        self.f.write_all(s.as_bytes()).map_err(|_| fmt::Error)
    }
}

impl<'a> ConvertToFmt for &'a mut dyn std::io::Write {
    type Writer = WriteAdapter<Self>;
    fn convert(self) -> Self::Writer {
        WriteAdapter { f: self }
    }
}

impl<'a> ConvertToFmt for &'a mut File {
    type Writer = WriteAdapter<Self>;
    fn convert(self) -> Self::Writer {
        WriteAdapter { f: self }
    }
}

pub struct PlainTextOutput<W: ConvertToFmt> {
    writer: W::Writer,
    last_end: f64,
    last_y: f64,
    first_char: bool,
    flip_ctm: Transform,
}

impl<W: ConvertToFmt> PlainTextOutput<W> {
    pub fn new(writer: W) -> PlainTextOutput<W> {
        PlainTextOutput {
            writer: writer.convert(),
            last_end: 100000.,
            first_char: false,
            last_y: 0.,
            flip_ctm: Transform2D::identity(),
        }
    }
}

/* There are some structural hints that PDFs can use to signal word and line endings:
 * however relying on these is not likely to be sufficient. */
impl<W: ConvertToFmt> OutputDev for PlainTextOutput<W> {
    fn begin_page(
        &mut self,
        _page_num: u32,
        media_box: &MediaBox,
        _: Option<ArtBox>,
    ) -> Result<(), OutputError> {
        self.flip_ctm = Transform2D::row_major(1., 0., 0., -1., 0., media_box.ury - media_box.lly);
        Ok(())
    }
    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        char: &str,
    ) -> Result<(), OutputError> {
        let position = trm.post_transform(&self.flip_ctm);
        let transformed_font_size_vec = trm.transform_vector(vec2(font_size, font_size));
        // get the length of one sized of the square with the same area with a rectangle of size (x, y)
        let transformed_font_size =
            (transformed_font_size_vec.x * transformed_font_size_vec.y).sqrt();
        let (x, y) = (position.m31, position.m32);
        use std::fmt::Write;
        //dlog!("last_end: {} x: {}, width: {}", self.last_end, x, width);
        if self.first_char {
            if (y - self.last_y).abs() > transformed_font_size * 1.5 {
                write!(self.writer, "\n")?;
            }

            // we've moved to the left and down
            if x < self.last_end && (y - self.last_y).abs() > transformed_font_size * 0.5 {
                write!(self.writer, "\n")?;
            }

            if x > self.last_end + transformed_font_size * 0.1 {
                dlog!(
                    "width: {}, space: {}, thresh: {}",
                    width,
                    x - self.last_end,
                    transformed_font_size * 0.1
                );
                write!(self.writer, " ")?;
            }
        }
        //let norm = unicode_normalization::UnicodeNormalization::nfkc(char);
        write!(self.writer, "{}", char)?;
        self.first_char = false;
        self.last_y = y;
        self.last_end = x + width * transformed_font_size;
        Ok(())
    }
    fn begin_word(&mut self) -> Result<(), OutputError> {
        self.first_char = true;
        Ok(())
    }
    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn end_line(&mut self) -> Result<(), OutputError> {
        //write!(self.file, "\n");
        Ok(())
    }
}

pub struct BoundingBoxOutput {
    flip_ctm: Transform,
    buf_start_x: f64,
    buf_start_y: f64,
    buf_end_x: f64,
    last_x: f64,
    last_y: f64,
    buf_font_size: f64,
    buf_ctm: Transform,
    buf: String,
    first_char: bool,
    current_page: u32,
    spans: Vec<TextSpan>,
}

impl BoundingBoxOutput {
    // Threshold for breaking to a new line (allows superscripts/subscripts to group with baseline text)
    const LINE_BREAK_THRESHOLD_POINTS: f64 = 8.0;

    // Threshold for inserting blank lines when Y-gap is larger than normal line spacing
    const BLANK_LINE_THRESHOLD_POINTS: f64 = 24.0;

    // Assumed vertical spacing per line in PDF points
    const POINTS_PER_LINE: f64 = 10.0;

    // Character spacing thresholds (as ratio of font size)
    // Gap > this ratio will create a new span (flush buffer)
    const CHAR_FLUSH_THRESHOLD_RATIO: f64 = 1.2;
    // Gap > this ratio will insert a space within the current span
    const CHAR_SPACE_THRESHOLD_RATIO: f64 = 0.15;

    pub fn new() -> BoundingBoxOutput {
        BoundingBoxOutput {
            flip_ctm: Transform2D::identity(),
            buf_start_x: 0.,
            buf_start_y: 0.,
            buf_end_x: 0.,
            last_x: 0.,
            last_y: 0.,
            buf_font_size: 0.,
            buf_ctm: Transform2D::identity(),
            buf: String::new(),
            first_char: false,
            current_page: 0,
            spans: Vec::new(),
        }
    }

    pub fn into_spans(self) -> Vec<TextSpan> {
        self.spans
    }

    pub fn into_lines(mut self) -> Vec<Vec<TextSpan>> {
        if self.spans.is_empty() {
            return Vec::new();
        }

        // Sort spans by page number first, then by Y coordinate (top to bottom)
        // This ensures pages don't get mixed together
        self.spans
            .sort_by(|a, b| match a.page_num.cmp(&b.page_num) {
                std::cmp::Ordering::Equal => a
                    .bbox
                    .top_left
                    .y
                    .partial_cmp(&b.bbox.top_left.y)
                    .unwrap_or(std::cmp::Ordering::Equal),
                other => other,
            });

        let mut lines: Vec<Vec<TextSpan>> = Vec::new();
        let mut current_line: Vec<TextSpan> = Vec::new();
        let mut last_y: Option<f64> = None;
        let mut last_page: Option<u32> = None;

        for span in self.spans {
            // Use baseline (bottom Y) for line grouping so superscripts group with their baseline text
            let span_y = span.bbox.bottom_left.y;
            let span_page = span.page_num;

            // Check if we've moved to a new page
            if let Some(prev_page) = last_page {
                if span_page != prev_page {
                    // Flush current line
                    if !current_line.is_empty() {
                        current_line.sort_by(|a, b| {
                            a.bbox
                                .bottom_left
                                .x
                                .partial_cmp(&b.bbox.bottom_left.x)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                        lines.push(current_line);
                        current_line = Vec::new();
                    }
                    // Insert a blank line as page separator
                    lines.push(Vec::new());
                    last_y = None;
                }
            }

            if let Some(prev_y) = last_y {
                let y_gap = (span_y - prev_y).abs();
                // Use absolute threshold - with baseline grouping, superscripts have ~0 gap
                // Footer rows and table rows have ~6-8pt gaps
                let line_break_threshold = 5.0;

                if y_gap > line_break_threshold {
                    // Start a new line
                    if !current_line.is_empty() {
                        // Sort spans in the line by X coordinate (left to right)
                        current_line.sort_by(|a, b| {
                            a.bbox
                                .bottom_left
                                .x
                                .partial_cmp(&b.bbox.bottom_left.x)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                        lines.push(current_line);
                        current_line = Vec::new();
                    }

                    // If the gap is significantly larger than normal, insert blank lines
                    if y_gap > Self::BLANK_LINE_THRESHOLD_POINTS {
                        // Calculate how many blank lines to insert based on the gap
                        let blank_lines = ((y_gap - Self::POINTS_PER_LINE) / Self::POINTS_PER_LINE)
                            .round() as usize;
                        // for _ in 0..blank_lines {
                        if blank_lines >= 1 {
                            lines.push(Vec::new());
                        }
                    }
                }
            }

            current_line.push(span);
            last_y = Some(span_y);
            last_page = Some(span_page);
        }

        if !current_line.is_empty() {
            // Sort the last line
            current_line.sort_by(|a, b| {
                a.bbox
                    .bottom_left
                    .x
                    .partial_cmp(&b.bbox.bottom_left.x)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            lines.push(current_line);
        }

        lines
    }

    fn flush_string(&mut self) -> Result<(), OutputError> {
        if self.buf.len() != 0 {
            // buf_start_x, buf_end_x, and buf_start_y are already in flipped coordinates
            // (they come from position.m31/m32 where position = trm.post_transform(&self.flip_ctm))
            // So we can use them directly for the bounding box
            // Normalize coordinates so left is always < right (handles RTL text positioning)
            let bottom_left_x = self.buf_start_x.min(self.buf_end_x);
            let bottom_right_x = self.buf_start_x.max(self.buf_end_x);
            let bottom_y = self.buf_start_y;

            // Get the top Y by adding transformed font size
            let transformed_font_size_vec = self
                .buf_ctm
                .transform_vector(euclid::vec2(self.buf_font_size, self.buf_font_size));
            let transformed_font_size =
                (transformed_font_size_vec.x * transformed_font_size_vec.y).sqrt();
            let top_y = self.buf_start_y + transformed_font_size;

            let bbox = BoundingBox {
                top_left: Point {
                    x: bottom_left_x,
                    y: top_y,
                },
                top_right: Point {
                    x: bottom_right_x,
                    y: top_y,
                },
                bottom_left: Point {
                    x: bottom_left_x,
                    y: bottom_y,
                },
                bottom_right: Point {
                    x: bottom_right_x,
                    y: bottom_y,
                },
            };

            self.spans.push(TextSpan {
                text: self.buf.clone(),
                bbox,
                font_size: self.buf_font_size,
                page_num: self.current_page,
            });

            self.buf.clear();
        }
        Ok(())
    }
}

impl OutputDev for BoundingBoxOutput {
    fn begin_page(
        &mut self,
        page_num: u32,
        media_box: &MediaBox,
        _: Option<ArtBox>,
    ) -> Result<(), OutputError> {
        self.current_page = page_num;
        self.flip_ctm = Transform::row_major(1., 0., 0., -1., 0., media_box.ury - media_box.lly);
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        self.flush_string()?;
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        char: &str,
    ) -> Result<(), OutputError> {
        let position = trm.post_transform(&self.flip_ctm);
        let transformed_font_size_vec = trm.transform_vector(vec2(font_size, font_size));
        let transformed_font_size =
            (transformed_font_size_vec.x * transformed_font_size_vec.y).sqrt();
        let (x, y) = (position.m31, position.m32);

        let normalized_char = if char == "\t" { " " } else { char };

        if self.buf.is_empty() {
            // First character of a new span (either very first, or after flush)
            self.buf_start_x = x;
            self.buf_start_y = y;
            self.buf_font_size = font_size;
            self.buf_ctm = *trm;
            self.buf = normalized_char.to_owned();
        } else {
            // Have existing buffer - check if should flush or add to it

            // Fix buf_end_x if previous character had width=0 but actually occupies space
            // (PDF width metric is 0, but character visually extends to where next char starts)
            // Only do this for characters on the SAME line - don't merge across line breaks
            if self.buf_end_x == self.last_x
                && (y - self.last_y).abs() < transformed_font_size * 0.5
            {
                // Previous char had PDF width=0, we're on the same line
                // The actual visual end is where the current character starts
                self.buf_end_x = x;
            }

            let gap = x - self.buf_end_x;
            // Calculate gap ratio normalized by font size
            let gap_ratio = gap / transformed_font_size;

            let y_gap = (y - self.last_y).abs();
            let should_flush = y_gap > transformed_font_size * 1.5
                || (x < self.buf_end_x && y_gap > transformed_font_size * 0.5)
                || (gap_ratio.abs() > Self::CHAR_FLUSH_THRESHOLD_RATIO);

            if should_flush {
                self.flush_string()?;
                self.buf_start_x = x;
                self.buf_start_y = y;
                self.buf_font_size = font_size;
                self.buf_ctm = *trm;
                self.buf = normalized_char.to_owned();
            } else {
                // Don't insert space if the previous character was already whitespace
                let prev_char_is_space =
                    self.buf.chars().last().map_or(false, |c| c.is_whitespace());
                let will_insert_space =
                    !prev_char_is_space && (gap_ratio > Self::CHAR_SPACE_THRESHOLD_RATIO);

                if will_insert_space {
                    self.buf += " ";
                }
                self.buf += normalized_char;
            }
        }

        self.first_char = false;
        self.last_x = x;
        self.last_y = y;
        self.buf_end_x = x + width * transformed_font_size;

        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        self.first_char = true;
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}

pub fn print_metadata(doc: &Document) {
    dlog!("Version: {}", doc.version);
    if let Some(ref info) = get_info(&doc) {
        for (k, v) in *info {
            match v {
                &Object::String(ref s, StringFormat::Literal) => {
                    dlog!("{}: {}", pdf_to_utf8(k), pdf_to_utf8(s));
                }
                _ => {}
            }
        }
    }
    dlog!(
        "Page count: {}",
        get::<i64>(&doc, &get_pages(&doc), b"Count")
    );
    dlog!("Pages: {:?}", get_pages(&doc));
    dlog!(
        "Type: {:?}",
        get_pages(&doc)
            .get(b"Type")
            .and_then(|x| x.as_name())
            .unwrap()
    );
}

/// Extract the text from a pdf at `path` and return a `String` with the results
pub fn extract_text<P: std::convert::AsRef<std::path::Path>>(
    path: P,
) -> Result<String, OutputError> {
    let mut s = String::new();
    {
        let mut output = PlainTextOutput::new(&mut s);
        let mut doc = Document::load(path)?;
        maybe_decrypt(&mut doc)?;
        output_doc(&doc, &mut output)?;
    }
    Ok(s)
}

fn maybe_decrypt(doc: &mut Document) -> Result<(), OutputError> {
    if !doc.is_encrypted() {
        return Ok(());
    }

    if let Err(e) = doc.decrypt("") {
        if let Error::Decryption(DecryptionError::IncorrectPassword) = e {
            error!(
                "Encrypted documents must be decrypted with a password using {{extract_text|extract_text_from_mem|output_doc}}_encrypted"
            )
        }

        return Err(OutputError::PdfError(e));
    }

    Ok(())
}

pub fn extract_text_encrypted<P: std::convert::AsRef<std::path::Path>>(
    path: P,
    password: &str,
) -> Result<String, OutputError> {
    let mut s = String::new();
    {
        let mut output = PlainTextOutput::new(&mut s);
        let mut doc = Document::load(path)?;
        output_doc_encrypted(&mut doc, &mut output, password)?;
    }
    Ok(s)
}

pub fn extract_text_from_mem(buffer: &[u8]) -> Result<String, OutputError> {
    let mut s = String::new();
    {
        let mut output = PlainTextOutput::new(&mut s);
        let mut doc = Document::load_mem(buffer)?;
        maybe_decrypt(&mut doc)?;
        output_doc(&doc, &mut output)?;
    }
    Ok(s)
}

pub fn extract_text_from_mem_encrypted(
    buffer: &[u8],
    password: &str,
) -> Result<String, OutputError> {
    let mut s = String::new();
    {
        let mut output = PlainTextOutput::new(&mut s);
        let mut doc = Document::load_mem(buffer)?;
        output_doc_encrypted(&mut doc, &mut output, password)?;
    }
    Ok(s)
}

fn extract_text_by_page(doc: &Document, page_num: u32) -> Result<String, OutputError> {
    let mut s = String::new();
    {
        let mut output = PlainTextOutput::new(&mut s);
        output_doc_page(doc, &mut output, page_num)?;
    }
    Ok(s)
}

/// Extract the text from a pdf at `path` and return a `Vec<String>` with the results separately by page

pub fn extract_text_by_pages<P: std::convert::AsRef<std::path::Path>>(
    path: P,
) -> Result<Vec<String>, OutputError> {
    let mut v = Vec::new();
    {
        let mut doc = Document::load(path)?;
        maybe_decrypt(&mut doc)?;
        let mut page_num = 1;
        while let Ok(content) = extract_text_by_page(&doc, page_num) {
            v.push(content);
            page_num += 1;
        }
    }
    Ok(v)
}

pub fn extract_text_by_pages_encrypted<P: std::convert::AsRef<std::path::Path>>(
    path: P,
    password: &str,
) -> Result<Vec<String>, OutputError> {
    let mut v = Vec::new();
    {
        let mut doc = Document::load(path)?;
        doc.decrypt(password)?;
        let mut page_num = 1;
        while let Ok(content) = extract_text_by_page(&mut doc, page_num) {
            v.push(content);
            page_num += 1;
        }
    }
    Ok(v)
}

pub fn extract_text_from_mem_by_pages(buffer: &[u8]) -> Result<Vec<String>, OutputError> {
    let mut v = Vec::new();
    {
        let mut doc = Document::load_mem(buffer)?;
        maybe_decrypt(&mut doc)?;
        let mut page_num = 1;
        while let Ok(content) = extract_text_by_page(&doc, page_num) {
            v.push(content);
            page_num += 1;
        }
    }
    Ok(v)
}

pub fn extract_text_from_mem_by_pages_encrypted(
    buffer: &[u8],
    password: &str,
) -> Result<Vec<String>, OutputError> {
    let mut v = Vec::new();
    {
        let mut doc = Document::load_mem(buffer)?;
        doc.decrypt(password)?;
        let mut page_num = 1;
        while let Ok(content) = extract_text_by_page(&doc, page_num) {
            v.push(content);
            page_num += 1;
        }
    }
    Ok(v)
}

pub fn extract_text_with_bounds<P: std::convert::AsRef<std::path::Path>>(
    path: P,
) -> Result<TextPage, OutputError> {
    let mut output = BoundingBoxOutput::new();
    let mut doc = Document::load(path)?;
    maybe_decrypt(&mut doc)?;
    output_doc(&doc, &mut output)?;
    Ok(output.into_lines())
}

pub fn extract_text_with_bounds_encrypted<P: std::convert::AsRef<std::path::Path>>(
    path: P,
    password: &str,
) -> Result<TextPage, OutputError> {
    let mut output = BoundingBoxOutput::new();
    let mut doc = Document::load(path)?;
    output_doc_encrypted(&mut doc, &mut output, password)?;
    Ok(output.into_lines())
}

pub fn extract_text_with_bounds_from_mem(buffer: &[u8]) -> Result<TextPage, OutputError> {
    let mut output = BoundingBoxOutput::new();
    let mut doc = Document::load_mem(buffer)?;
    maybe_decrypt(&mut doc)?;
    output_doc(&doc, &mut output)?;
    Ok(output.into_lines())
}

pub fn extract_text_with_bounds_from_mem_encrypted(
    buffer: &[u8],
    password: &str,
) -> Result<TextPage, OutputError> {
    let mut output = BoundingBoxOutput::new();
    let mut doc = Document::load_mem(buffer)?;
    output_doc_encrypted(&mut doc, &mut output, password)?;
    Ok(output.into_lines())
}

fn get_inherited<'a, T: FromObj<'a>>(
    doc: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<T> {
    let o: Option<T> = get(doc, dict, key);
    if let Some(o) = o {
        Some(o)
    } else {
        let parent = dict
            .get(b"Parent")
            .and_then(|parent| parent.as_reference())
            .and_then(|id| doc.get_dictionary(id))
            .ok()?;
        get_inherited(doc, parent, key)
    }
}

pub fn output_doc_encrypted(
    doc: &mut Document,
    output: &mut dyn OutputDev,
    password: &str,
) -> Result<(), OutputError> {
    doc.decrypt(password)?;
    output_doc(doc, output)
}

/// Parse a given document and output it to `output`
pub fn output_doc(doc: &Document, output: &mut dyn OutputDev) -> Result<(), OutputError> {
    if doc.is_encrypted() {
        error!(
            "Encrypted documents must be decrypted with a password using {{extract_text|extract_text_from_mem|output_doc}}_encrypted"
        );
    }
    let empty_resources = Dictionary::new();
    let pages = doc.get_pages();
    let mut p = Processor::new();
    for dict in pages {
        let page_num = dict.0;
        let object_id = dict.1;
        output_doc_inner(page_num, object_id, doc, &mut p, output, &empty_resources)?;
    }
    Ok(())
}

pub fn output_doc_page(
    doc: &Document,
    output: &mut dyn OutputDev,
    page_num: u32,
) -> Result<(), OutputError> {
    if doc.is_encrypted() {
        error!(
            "Encrypted documents must be decrypted with a password using {{extract_text|extract_text_from_mem|output_doc}}_encrypted"
        );
    }
    let empty_resources = Dictionary::new();
    let pages = doc.get_pages();
    let object_id = pages
        .get(&page_num)
        .ok_or(lopdf::Error::PageNumberNotFound(page_num))?;
    let mut p = Processor::new();
    output_doc_inner(page_num, *object_id, doc, &mut p, output, &empty_resources)?;
    Ok(())
}

fn output_doc_inner<'a>(
    page_num: u32,
    object_id: ObjectId,
    doc: &'a Document,
    p: &mut Processor<'a>,
    output: &mut dyn OutputDev,
    empty_resources: &'a Dictionary,
) -> Result<(), OutputError> {
    let page_dict = doc.get_object(object_id).unwrap().as_dict().unwrap();
    dlog!("page {} {:?}", page_num, page_dict);
    // XXX: Some pdfs lack a Resources directory
    let resources = get_inherited(doc, page_dict, b"Resources").unwrap_or(empty_resources);
    dlog!("resources {:?}", resources);
    // pdfium searches up the page tree for MediaBoxes as needed
    let media_box: Vec<f64> = get_inherited(doc, page_dict, b"MediaBox").expect("MediaBox");
    let media_box = MediaBox {
        llx: media_box[0],
        lly: media_box[1],
        urx: media_box[2],
        ury: media_box[3],
    };
    let art_box =
        get::<Option<Vec<f64>>>(&doc, page_dict, b"ArtBox").map(|x| (x[0], x[1], x[2], x[3]));
    output.begin_page(page_num, &media_box, art_box)?;
    p.process_stream(
        &doc,
        doc.get_page_content(object_id).unwrap(),
        resources,
        &media_box,
        output,
        page_num,
    )?;
    output.end_page()?;
    Ok(())
}
