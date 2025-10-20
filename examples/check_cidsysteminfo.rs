extern crate lopdf;

use lopdf::{Document, Object};
use std::env;

fn main() {
    let file = env::args()
        .nth(1)
        .expect("Usage: check_cidsysteminfo <pdf_file>");
    let doc = Document::load(&file).expect("Failed to load PDF");

    for (object_id, object) in doc.objects.iter() {
        if let Object::Dictionary(dict) = object {
            // Check for Type0 fonts
            if let Ok(Object::Name(subtype)) = dict.get(b"Subtype") {
                if subtype == b"Type0" {
                    if let Ok(Object::Name(base_font)) = dict.get(b"BaseFont") {
                        println!(
                            "\n=== Type0 Font: {:?} at {:?} ===",
                            std::str::from_utf8(base_font).unwrap(),
                            object_id
                        );
                    }

                    // Check Encoding
                    if let Ok(encoding) = dict.get(b"Encoding") {
                        println!("Encoding: {:?}", encoding);
                    }

                    // Check DescendantFonts
                    if let Ok(Object::Array(descendants)) = dict.get(b"DescendantFonts") {
                        if let Some(Object::Reference(desc_ref)) = descendants.first() {
                            if let Ok(Object::Dictionary(desc_dict)) = doc.get_object(*desc_ref) {
                                println!("\nDescendantFont at {:?}:", desc_ref);

                                // Check CIDSystemInfo
                                if let Ok(cidsysinfo_ref) = desc_dict.get(b"CIDSystemInfo") {
                                    match cidsysinfo_ref {
                                        Object::Reference(r) => {
                                            if let Ok(Object::Dictionary(csi_dict)) =
                                                doc.get_object(*r)
                                            {
                                                println!("  CIDSystemInfo:");
                                                if let Ok(Object::String(registry, _)) =
                                                    csi_dict.get(b"Registry")
                                                {
                                                    println!(
                                                        "    Registry: {:?}",
                                                        String::from_utf8_lossy(registry)
                                                    );
                                                }
                                                if let Ok(Object::String(ordering, _)) =
                                                    csi_dict.get(b"Ordering")
                                                {
                                                    println!(
                                                        "    Ordering: {:?}",
                                                        String::from_utf8_lossy(ordering)
                                                    );
                                                }
                                                if let Ok(Object::Integer(supplement)) =
                                                    csi_dict.get(b"Supplement")
                                                {
                                                    println!("    Supplement: {}", supplement);
                                                }
                                            }
                                        }
                                        Object::Dictionary(csi_dict) => {
                                            println!("  CIDSystemInfo:");
                                            if let Ok(Object::String(registry, _)) =
                                                csi_dict.get(b"Registry")
                                            {
                                                println!(
                                                    "    Registry: {:?}",
                                                    String::from_utf8_lossy(registry)
                                                );
                                            }
                                            if let Ok(Object::String(ordering, _)) =
                                                csi_dict.get(b"Ordering")
                                            {
                                                println!(
                                                    "    Ordering: {:?}",
                                                    String::from_utf8_lossy(ordering)
                                                );
                                            }
                                            if let Ok(Object::Integer(supplement)) =
                                                csi_dict.get(b"Supplement")
                                            {
                                                println!("    Supplement: {}", supplement);
                                            }
                                        }
                                        _ => {}
                                    }
                                }

                                // Check CIDToGIDMap
                                if let Ok(cidtogid) = desc_dict.get(b"CIDToGIDMap") {
                                    println!("  CIDToGIDMap: {:?}", cidtogid);
                                }

                                // Check DW (default width)
                                if let Ok(Object::Integer(dw)) = desc_dict.get(b"DW") {
                                    println!("  DW (default width): {}", dw);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
