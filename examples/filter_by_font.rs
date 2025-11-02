use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: {} <pdf_file> <font_filter>", args[0]);
        eprintln!("\nExample: {} document.pdf Bold", args[0]);
        eprintln!("         {} document.pdf Helvetica", args[0]);
        process::exit(1);
    }

    let path = &args[1];
    let filter = &args[2];

    let output = match pdf_strings::from_path(path) {
        Ok(output) => output,
        Err(e) => {
            eprintln!("Error extracting text from PDF: {}", e);
            process::exit(1);
        }
    };

    let mut matched_spans = 0;
    let mut matched_text = Vec::new();

    for line in output.lines() {
        for span in line {
            if span
                .font_name
                .to_lowercase()
                .contains(&filter.to_lowercase())
            {
                matched_spans += 1;
                matched_text.push(span.text.clone());
            }
        }
    }

    if matched_spans == 0 {
        eprintln!("No spans found with font matching '{}'", filter);
        return;
    }

    for text in matched_text {
        println!("{}", text);
    }
}
