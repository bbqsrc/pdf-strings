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
#[command(about = "Extract text from PDF or dvips PostScript files", long_about = None)]
struct Args {
    /// Input file (.pdf or .ps). Dispatch is by extension.
    #[arg(value_name = "FILE")]
    file: PathBuf,

    /// Password for encrypted PDFs (ignored for .ps inputs)
    #[arg(short, long)]
    password: Option<String>,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Plain)]
    format: OutputFormat,
}

fn main() {
    let args = Args::parse();

    let is_ps = args
        .file
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("ps"))
        .unwrap_or(false);

    // Dispatch: .ps → PostScript path (password ignored),
    //          everything else → PDF path (honours password).
    let result = if is_ps {
        pdf_strings::from_ps_path(&args.file)
    } else {
        let extractor = if let Some(password) = args.password {
            pdf_strings::PdfExtractor::builder()
                .password(password)
                .build()
        } else {
            pdf_strings::PdfExtractor::default()
        };
        extractor.from_path(&args.file)
    };

    let output = match result {
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
