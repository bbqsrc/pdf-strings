extern crate pdf_extract;
extern crate simple_logger;

use std::env;

fn main() {
    simple_logger::SimpleLogger::new().init().unwrap();

    let file = env::args().nth(1).expect("Usage: extract_bounds <pdf_file>");

    match pdf_extract::extract_text_with_bounds(&file) {
        Ok(lines) => {
            println!("Found {} lines:", lines.len());
            for (line_idx, line) in lines.iter().enumerate() {
                println!("\nLine {}:", line_idx);
                for (span_idx, span) in line.iter().enumerate() {
                    println!("  Span {}: {:?}", span_idx, span.text);
                    print!("    ({:.2}, {:.2})", span.bbox.top_left.x, span.bbox.top_left.y);
                    print!(" ({:.2}, {:.2})", span.bbox.top_right.x, span.bbox.top_right.y);
                    print!(" ({:.2}, {:.2})", span.bbox.bottom_left.x, span.bbox.bottom_left.y);
                    println!(" ({:.2}, {:.2})", span.bbox.bottom_right.x, span.bbox.bottom_right.y);
                }
            }

            println!("\n\n=== Plain Text Output ===\n");

            // Detect right-aligned columns across all lines
            let right_aligned_positions = pdf_extract::detect_right_aligned_columns(&lines);
            // Increased threshold to catch headers which may have different widths than data
            const ALIGNMENT_THRESHOLD: f64 = 16.0;

            for line in lines.iter() {
                if line.is_empty() {
                    // Print blank line
                    println!();
                    continue;
                }

                let mut line_output = String::new();
                let mut cursor_col = 0; // Track current column position in the character grid

                for span in line.iter() {
                    let text_len = span.text.chars().count();

                    if span.is_right_aligned(&right_aligned_positions, ALIGNMENT_THRESHOLD) {
                        // For right-aligned spans, find which cluster position it belongs to
                        // and align to that cluster position (not the span's own end position)
                        let span_right_x = span.bbox.bottom_right.x;
                        let cluster_position = right_aligned_positions.iter()
                            .find(|&&pos_x| (span_right_x - pos_x).abs() < ALIGNMENT_THRESHOLD)
                            .expect("span should match a cluster position");

                        let target_end_col = pdf_extract::TextSpan::x_to_col(*cluster_position);
                        let target_start_col = target_end_col.saturating_sub(text_len);

                        // Pad to the target starting position, ensuring at least 1 space between spans
                        if target_start_col > cursor_col {
                            let padding = target_start_col - cursor_col;
                            for _ in 0..padding {
                                line_output.push(' ');
                            }
                            cursor_col = target_start_col;
                        } else if cursor_col > 0 {
                            // Spans touch or overlap - add at least one space to separate them
                            line_output.push(' ');
                            cursor_col += 1;
                        }

                        // Output the text
                        line_output.push_str(&span.text);
                        cursor_col += text_len;
                    } else {
                        // For left-aligned spans, use the existing logic
                        let span_start = span.start_col();

                        // Pad with spaces to reach the span's starting column, ensuring at least 1 space between spans
                        if span_start > cursor_col {
                            let padding = span_start - cursor_col;
                            for _ in 0..padding {
                                line_output.push(' ');
                            }
                            cursor_col = span_start;
                        } else if cursor_col > 0 {
                            // Spans touch or overlap - add at least one space to separate them
                            line_output.push(' ');
                            cursor_col += 1;
                        }

                        // Output the text
                        line_output.push_str(&span.text);

                        // Update cursor position: move by actual text length
                        // (text may be longer than grid_width if it overflows the bbox)
                        cursor_col += text_len;
                    }
                }

                println!("{}", line_output);
            }
        }
        Err(e) => {
            eprintln!("Error extracting text: {}", e);
            std::process::exit(1);
        }
    }
}
