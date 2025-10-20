use std::fmt;

use euclid::Transform2D;

use crate::utils::detect_right_aligned_columns;

pub struct Space;
pub type Transform = Transform2D<f64, Space, Space>;

#[derive(Debug, Clone, Copy)]
pub struct MediaBox {
    pub llx: f64,
    pub lly: f64,
    pub urx: f64,
    pub ury: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone)]
pub struct BoundingBox {
    pub t: f64,
    pub r: f64,
    pub b: f64,
    pub l: f64,
}

impl BoundingBox {
    pub fn top_left(&self) -> Point {
        Point {
            x: self.l,
            y: self.t,
        }
    }

    pub fn top_right(&self) -> Point {
        Point {
            x: self.r,
            y: self.t,
        }
    }

    pub fn bottom_left(&self) -> Point {
        Point {
            x: self.l,
            y: self.b,
        }
    }

    pub fn bottom_right(&self) -> Point {
        Point {
            x: self.r,
            y: self.b,
        }
    }
}

impl fmt::Display for BoundingBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "(t: {:.1}, r: {:.1}, b: {:.1}, l: {:.1})",
            self.t, self.r, self.b, self.l
        )
    }
}

#[derive(Debug, Clone)]
pub struct TextSpan {
    pub text: String,
    pub bbox: BoundingBox,
    pub font_size: f64,
    pub page_num: u32,
}

impl TextSpan {
    // Standard monospace character width in PDF points.
    // This maps PDF coordinates to a character grid for terminal output.
    // Adjust this constant if the output spacing doesn't match your terminal.
    pub const MONOSPACE_CHAR_WIDTH_POINTS: f64 = 4.0;

    /// Convert a PDF x-coordinate (in points) to a character grid column number.
    /// The grid starts at column 0 for x=0.
    pub fn x_to_col(x: f64) -> usize {
        (x / Self::MONOSPACE_CHAR_WIDTH_POINTS).round() as usize
    }

    /// Get the starting column position for this span in the character grid.
    pub fn start_col(&self) -> usize {
        Self::x_to_col(self.bbox.l)
    }

    /// Get the ending column position for this span in the character grid.
    pub fn end_col(&self) -> usize {
        Self::x_to_col(self.bbox.r)
    }

    /// Get the width of this span in character grid cells.
    /// Returns at least the length of the text to avoid truncation.
    pub fn grid_width(&self) -> usize {
        let bbox_width = self.end_col().saturating_sub(self.start_col());
        bbox_width.max(self.text.chars().count())
    }

    /// Check if this span belongs to a right-aligned column.
    /// Returns true if the span's right edge is close to any of the detected right-aligned positions.
    pub fn is_right_aligned(&self, right_aligned_positions: &[f64], threshold: f64) -> bool {
        let right_x = self.bbox.r;
        right_aligned_positions
            .iter()
            .any(|&pos_x| (right_x - pos_x).abs() < threshold)
    }
}

pub type TextLine = Vec<TextSpan>;
pub type TextPage = Vec<TextLine>;

/// Output of PDF text extraction.
/// Contains structured text data (lines with positioned spans) that can be
/// converted to plain text or formatted text.
#[derive(Debug, Clone)]
pub struct TextOutput {
    lines: Vec<Vec<TextSpan>>,
}

impl TextOutput {
    /// Get a reference to the lines of text spans.
    pub fn lines(&self) -> &[Vec<TextSpan>] {
        &self.lines
    }

    /// Consume self and return the lines of text spans.
    pub fn into_lines(self) -> Vec<Vec<TextSpan>> {
        self.lines
    }

    /// Convert to a pretty-formatted string using character grid positioning.
    /// This preserves the spatial layout of the PDF, including right-aligned columns.
    pub fn to_string_pretty(&self) -> String {
        const ALIGNMENT_THRESHOLD: f64 = 16.0;

        let right_aligned_positions = detect_right_aligned_columns(&self.lines);
        let mut output = String::new();

        for line in &self.lines {
            if line.is_empty() {
                output.push('\n');
                continue;
            }

            let mut line_output = String::new();
            let mut cursor_col = 0;

            for span in line {
                let text_len = span.text.chars().count();

                if span.is_right_aligned(&right_aligned_positions, ALIGNMENT_THRESHOLD) {
                    let span_right_x = span.bbox.r;
                    if let Some(&cluster_position) = right_aligned_positions
                        .iter()
                        .find(|&&pos_x| (span_right_x - pos_x).abs() < ALIGNMENT_THRESHOLD)
                    {
                        let target_end_col = TextSpan::x_to_col(cluster_position);
                        let target_start_col = target_end_col.saturating_sub(text_len);

                        if target_start_col > cursor_col {
                            let padding = target_start_col - cursor_col;
                            for _ in 0..padding {
                                line_output.push(' ');
                            }
                            cursor_col = target_start_col;
                        } else if cursor_col > 0 {
                            line_output.push(' ');
                            cursor_col += 1;
                        }

                        line_output.push_str(&span.text);
                        cursor_col += text_len;
                    }
                } else {
                    let span_start = span.start_col();

                    if span_start > cursor_col {
                        let padding = span_start - cursor_col;
                        for _ in 0..padding {
                            line_output.push(' ');
                        }
                        cursor_col = span_start;
                    } else if cursor_col > 0 {
                        line_output.push(' ');
                        cursor_col += 1;
                    }

                    line_output.push_str(&span.text);
                    cursor_col += text_len;
                }
            }

            output.push_str(&line_output);
            output.push('\n');
        }

        output
    }
}

impl fmt::Display for TextOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for line in &self.lines {
            for (i, span) in line.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", span.text)?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

impl From<Vec<Vec<TextSpan>>> for TextOutput {
    fn from(lines: Vec<Vec<TextSpan>>) -> Self {
        TextOutput { lines }
    }
}
