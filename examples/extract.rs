use std::env;

fn main() {
    simple_logger::SimpleLogger::new().init().unwrap();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <pdf_file> [password]", args[0]);
        std::process::exit(1);
    }

    let path = &args[1];
    let password = args.get(2);

    let output = if let Some(pwd) = password {
        // With password
        pdf_strings::PdfExtractor::builder()
            .password(pwd)
            .build()
            .from_path(path)
    } else {
        // Without password (convenience function)
        pdf_strings::from_path(path)
    };

    match output {
        Ok(text_output) => {
            println!("=== Plain Text (via Display) ===\n");
            println!("{}", text_output);

            println!("\n=== Pretty Formatted (preserves layout) ===\n");
            println!("{}", text_output.to_string_pretty());

            println!("\n=== Structured Data Access ===\n");
            for (line_idx, line) in text_output.lines().iter().enumerate().take(5) {
                println!("Line {}:", line_idx);
                for span in line {
                    println!("  Text: {:?}, BBox: {}", span.text, span.bbox);
                }
            }
        }
        Err(e) => {
            eprintln!("Error extracting text: {}", e);
            std::process::exit(1);
        }
    }
}
