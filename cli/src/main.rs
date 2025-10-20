use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    /// Simple plain text output (concatenates text with spaces)
    Plain,
    /// Pretty formatted output preserving spatial layout
    Pretty,
    /// Debug output showing structured data with bounding boxes
    Debug,
}

#[derive(Parser)]
#[command(name = "pdf-strings")]
#[command(about = "Extract text from PDF files", long_about = None)]
struct Args {
    /// PDF file to extract text from
    #[arg(value_name = "FILE")]
    file: PathBuf,

    /// Password for encrypted PDFs
    #[arg(short, long)]
    password: Option<String>,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Plain)]
    format: OutputFormat,
}

fn main() {
    let args = Args::parse();

    // Build extractor with optional password
    let extractor = if let Some(password) = args.password {
        pdf_strings::PdfExtractor::builder()
            .password(password)
            .build()
    } else {
        pdf_strings::PdfExtractor::default()
    };

    // Extract text
    let output = match extractor.from_path(&args.file) {
        Ok(output) => output,
        Err(e) => {
            eprintln!("Error extracting text from {:?}: {}", args.file, e);
            std::process::exit(1);
        }
    };

    // Print in requested format
    match args.format {
        OutputFormat::Plain => {
            print!("{}", output);
        }
        OutputFormat::Pretty => {
            print!("{}", output.to_string_pretty());
        }
        OutputFormat::Debug => {
            for (line_idx, line) in output.lines().iter().enumerate() {
                if line.is_empty() {
                    println!("Line {}: (empty)", line_idx);
                    continue;
                }

                println!("Line {}:", line_idx);
                for (span_idx, span) in line.iter().enumerate() {
                    println!("  Span {}: {:?}", span_idx, span.text);
                    println!("    BBox: {}", span.bbox);
                    println!("    Font size: {:.1}", span.font_size);
                }
            }
        }
    }
}
