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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OcrMode {
    /// Run OCR on pages that produced no text from the content stream
    /// (scanned/image-only pages). Requires macOS.
    Auto,
    /// Never run OCR.
    Off,
}

impl From<OcrMode> for pdf_strings::OcrPolicy {
    fn from(m: OcrMode) -> Self {
        match m {
            OcrMode::Auto => pdf_strings::OcrPolicy::Auto,
            OcrMode::Off => pdf_strings::OcrPolicy::Off,
        }
    }
}

#[cfg(target_os = "macos")]
const DEFAULT_OCR: OcrMode = OcrMode::Auto;
#[cfg(not(target_os = "macos"))]
const DEFAULT_OCR: OcrMode = OcrMode::Off;

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

    /// OCR fallback for scanned/image-only pages. Default `auto` on
    /// macOS (uses Apple Vision), `off` elsewhere.
    #[arg(long, value_enum, default_value_t = DEFAULT_OCR)]
    ocr: OcrMode,

    /// BCP-47 language tags for OCR (comma-separated, e.g. `en-US,da-DK`).
    /// Pass `auto` to let Vision detect the language. Macos only.
    #[arg(short = 'l', long, value_delimiter = ',', default_value = "en-US")]
    lang: Vec<String>,

    /// Enable deskewing of scanned pages before OCR. Uses Vision's
    /// document-segmentation to detect page rotation and re-renders
    /// straightened. Off by default — the detector follows the paper
    /// outline, not text baselines, so it misfires on scans with bent
    /// corners. macOS only.
    #[arg(long)]
    deskew: bool,

    /// Write the final grayscale bitmap Vision sees for every OCR'd page
    /// to this directory as `page-<N>.pgm`. macOS only.
    #[arg(long, value_name = "DIR")]
    dump_images: Option<PathBuf>,
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
        // `auto` is the sentinel that maps to empty-list = Vision
        // automatic language detection.
        let languages: Vec<String> = if args.lang.iter().any(|s| s.eq_ignore_ascii_case("auto")) {
            Vec::new()
        } else {
            args.lang.clone()
        };
        let mut builder = pdf_strings::PdfExtractor::builder()
            .ocr(args.ocr.into())
            .ocr_languages(languages)
            .deskew(args.deskew);
        if let Some(dir) = args.dump_images.clone() {
            builder = builder.ocr_dump_dir(dir);
        }
        if let Some(password) = args.password {
            builder = builder.password(password);
        }
        builder.build().from_path(&args.file)
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
