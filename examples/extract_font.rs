use lopdf::{Document, Object};
use std::env;
use std::fs::File;
use std::io::Write;

fn main() {
    let file = env::args().nth(1).expect("Usage: extract_font <pdf_file>");

    let doc = Document::load(&file).expect("Failed to load PDF");

    for (object_id, object) in doc.objects.iter() {
        if let Object::Dictionary(dict) = object {
            if let Ok(Object::Name(subtype)) = dict.get(b"Subtype") {
                if subtype == b"Type0" {
                    if let Ok(Object::Name(base_font)) = dict.get(b"BaseFont") {
                        println!(
                            "Found Type0 font: {:?} at {:?}",
                            std::str::from_utf8(base_font),
                            object_id
                        );

                        if let Ok(Object::Array(descendants)) = dict.get(b"DescendantFonts") {
                            if let Some(Object::Reference(desc_ref)) = descendants.first() {
                                if let Ok(Object::Dictionary(desc_dict)) = doc.get_object(*desc_ref)
                                {
                                    if let Ok(Object::Reference(fd_ref)) =
                                        desc_dict.get(b"FontDescriptor")
                                    {
                                        if let Ok(Object::Dictionary(fd_dict)) =
                                            doc.get_object(*fd_ref)
                                        {
                                            if let Ok(Object::Reference(ff_ref)) =
                                                fd_dict.get(b"FontFile2")
                                            {
                                                if let Ok(Object::Stream(stream)) =
                                                    doc.get_object(*ff_ref)
                                                {
                                                    let font_data = stream
                                                        .decompressed_content()
                                                        .expect("Failed to decompress");
                                                    let filename = format!(
                                                        "/tmp/extracted_font_{:?}.ttf",
                                                        object_id
                                                    );
                                                    let mut out_file = File::create(&filename)
                                                        .expect("Failed to create file");
                                                    out_file
                                                        .write_all(&font_data)
                                                        .expect("Failed to write");
                                                    println!("  Extracted to: {}", filename);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
