extern crate lopdf;
extern crate pdf_extract;

use lopdf::{Document, Object};
use std::env;

fn main() {
    let file = env::args()
        .nth(1)
        .expect("Usage: dump_tounicode <pdf_file>");

    let doc = Document::load(&file).expect("Failed to load PDF");

    for (object_id, object) in doc.objects.iter() {
        if let Object::Dictionary(ref dict) = object {
            if let Ok(Object::Name(subtype)) = dict.get(b"Subtype") {
                if subtype == b"Type0" || subtype == b"CIDFontType0" || subtype == b"CIDFontType2" {
                    if let Ok(Object::Reference(ref font_ref)) = dict
                        .get(b"ToUnicode")
                        .or_else(|_| dict.get(b"DescendantFonts"))
                    {
                        println!("Found font at object {:?}", object_id);
                    }
                }
            }

            if let Ok(to_unicode_obj) = dict.get(b"ToUnicode") {
                if let Object::Reference(ref_id) = to_unicode_obj {
                    if let Ok(Object::Stream(ref stream)) = doc.get_object(*ref_id) {
                        let contents = stream.decompressed_content().expect("Failed to decompress");
                        let text = String::from_utf8_lossy(&contents);

                        println!("=== ToUnicode CMap at object {:?} ===", object_id);

                        let mut in_range = false;
                        let mut lines_after_055e = 0;
                        for line in text.lines() {
                            if line.contains("055E") {
                                in_range = true;
                            }
                            if in_range {
                                println!("{}", line);
                                lines_after_055e += 1;
                                if lines_after_055e > 10 {
                                    in_range = false;
                                    lines_after_055e = 0;
                                }
                            }
                            if line.contains("beginbfchar") {
                                in_range = true;
                            }
                            if line.contains("endbfchar") || line.contains("endbfrange") {
                                if in_range {
                                    println!("{}", line);
                                }
                                in_range = false;
                            }
                        }
                        println!();
                    }
                }
            }
        }
    }
}
