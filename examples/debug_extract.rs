use std::env;

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(true)
        .with_line_number(true)
        .init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <pdf_file> [password]", args[0]);
        std::process::exit(1);
    }

    let path = &args[1];
    let password = args.get(2);

    let output = if let Some(pwd) = password {
        pdf_strings::PdfExtractor::builder()
            .password(pwd)
            .build()
            .from_path(path)
    } else {
        pdf_strings::from_path(path)
    };

    match output {
        Ok(text_output) => {
            println!("\n=== EXTRACTED TEXT ===\n");
            println!("{}", text_output.to_string_pretty());
        }
        Err(e) => {
            eprintln!("Error extracting text: {}", e);
            std::process::exit(1);
        }
    }
}
