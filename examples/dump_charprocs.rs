use lopdf::{Document, Object};
use std::env;

fn main() {
    let file = env::args()
        .nth(1)
        .expect("Usage: dump_charprocs <pdf_file> [glyph]");
    let glyph_filter = env::args().nth(2);

    let doc = Document::load(&file).expect("Failed to load PDF");

    for (obj_id, obj) in doc.objects.iter() {
        let Object::Dictionary(dict) = obj else {
            continue;
        };
        let Ok(Object::Name(subtype)) = dict.get(b"Subtype") else {
            continue;
        };
        if subtype != b"Type3" {
            continue;
        }

        let name = dict
            .get(b"Name")
            .ok()
            .and_then(|o| o.as_name().ok())
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_else(|| "<no name>".into());
        println!("==== Type3 font obj {:?} name={:?} ====", obj_id, name);

        let Ok(char_procs_obj) = dict.get(b"CharProcs") else {
            println!("  no CharProcs");
            continue;
        };
        let char_procs = match char_procs_obj {
            Object::Dictionary(d) => d,
            Object::Reference(r) => doc
                .get_object(*r)
                .and_then(|o| o.as_dict())
                .expect("CharProcs deref"),
            _ => panic!("unexpected CharProcs type"),
        };

        for (glyph_name, proc_obj) in char_procs.iter() {
            let glyph_name_s = String::from_utf8_lossy(glyph_name);
            if let Some(ref f) = glyph_filter {
                if glyph_name_s != *f {
                    continue;
                }
            }
            let stream = match proc_obj {
                Object::Stream(s) => s,
                Object::Reference(r) => doc
                    .get_object(*r)
                    .and_then(|o| o.as_stream())
                    .expect("CharProc deref"),
                _ => continue,
            };
            let content = stream
                .decompressed_content()
                .unwrap_or_else(|_| stream.content.clone());
            let as_str = String::from_utf8_lossy(&content);

            println!("  --- /{} ({} bytes) ---", glyph_name_s, content.len());
            println!("{}", as_str);

            if glyph_filter.is_some() {
                return;
            }
        }
        break;
    }
}
