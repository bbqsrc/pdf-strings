use std::collections::HashMap;
use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <pdf_file>", args[0]);
        process::exit(1);
    }

    let path = &args[1];

    let output = match pdf_strings::from_path(path) {
        Ok(output) => output,
        Err(e) => {
            eprintln!("Error extracting text from PDF: {}", e);
            process::exit(1);
        }
    };

    let mut font_counts: HashMap<String, usize> = HashMap::new();

    for line in output.lines() {
        for span in line {
            *font_counts.entry(span.font_name.clone()).or_insert(0) += 1;
        }
    }

    if font_counts.is_empty() {
        println!("No fonts found in document.");
        return;
    }

    let mut fonts: Vec<_> = font_counts.iter().collect();
    fonts.sort_by_key(|(name, _)| name.as_str());

    println!("Fonts found in {}:\n", path);
    for (font, count) in fonts {
        println!("  {} (used in {} spans)", font, count);
    }
}
