extern crate lopdf;
extern crate pdf_extract;

use lopdf::Document;
use std::env;

fn main() {
    let file = env::args().nth(1).expect("Usage: dump_content <pdf_file>");

    let doc = Document::load(&file).expect("Failed to load PDF");

    // Get first page
    let pages = doc.get_pages();
    let first_page_id = *pages.get(&1).expect("No page 1");

    // Get page object
    let page = doc.get_object(first_page_id).expect("Failed to get page");
    let page_dict = page.as_dict().expect("Page is not a dict");

    // Get Contents
    if let Ok(contents_obj) = page_dict.get(b"Contents") {
        println!("Contents object: {:?}", contents_obj);

        let contents = match contents_obj {
            lopdf::Object::Reference(r) => doc.get_object(*r).expect("Failed to deref"),
            obj => obj,
        };

        if let lopdf::Object::Stream(stream) = contents {
            let decoded = stream.decompressed_content().expect("Failed to decompress");
            let content_str = String::from_utf8_lossy(&decoded);

            // Find lines containing relevant text
            for (idx, line) in content_str.lines().enumerate() {
                if line.contains("01") || line.contains("CBDC") || line.contains("Tj") {
                    println!("Line {}: {}", idx, line);
                }
            }
        }
    }
}
