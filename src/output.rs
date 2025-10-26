use euclid::{Transform2D, vec2};

use crate::error::OutputError;
use crate::types::{BoundingBox, MediaBox, TextSpan, Transform};

type ArtBox = (f32, f32, f32, f32);

pub(crate) struct BoundingBoxOutput {
    flip_ctm: Transform,
    buf_start_x: f32,
    buf_start_y: f32,
    buf_end_x: f32,
    last_x: f32,
    last_y: f32,
    buf_font_size: f32,
    buf_ctm: Transform,
    buf: String,
    first_char: bool,
    current_page: u32,
    spans: Vec<TextSpan>,
}

impl BoundingBoxOutput {
    // Threshold for inserting blank lines when Y-gap is larger than normal line spacing
    const BLANK_LINE_THRESHOLD_POINTS: f32 = 24.0;

    // Assumed vertical spacing per line in PDF points
    const POINTS_PER_LINE: f32 = 10.0;

    // Character spacing thresholds (as ratio of font size)
    // Gap > this ratio will create a new span (flush buffer)
    const CHAR_FLUSH_THRESHOLD_RATIO: f32 = 1.2;
    // Gap > this ratio will insert a space within the current span
    const CHAR_SPACE_THRESHOLD_RATIO: f32 = 0.15;

    pub(crate) fn new() -> BoundingBoxOutput {
        BoundingBoxOutput {
            flip_ctm: Transform2D::identity(),
            buf_start_x: 0.,
            buf_start_y: 0.,
            buf_end_x: 0.,
            last_x: 0.,
            last_y: 0.,
            buf_font_size: 0.,
            buf_ctm: Transform2D::identity(),
            buf: String::new(),
            first_char: false,
            current_page: 0,
            spans: Vec::new(),
        }
    }

    pub fn into_lines(mut self) -> Vec<Vec<TextSpan>> {
        if self.spans.is_empty() {
            return Vec::new();
        }

        // Sort spans by page number first, then by Y coordinate (top to bottom)
        // This ensures pages don't get mixed together
        self.spans
            .sort_by(|a, b| match a.page_num.cmp(&b.page_num) {
                std::cmp::Ordering::Equal => a
                    .bbox
                    .t
                    .partial_cmp(&b.bbox.t)
                    .unwrap_or(std::cmp::Ordering::Equal),
                other => other,
            });

        let mut lines: Vec<Vec<TextSpan>> = Vec::new();
        let mut current_line: Vec<TextSpan> = Vec::new();
        let mut last_y: Option<f32> = None;
        let mut last_page: Option<u32> = None;

        for span in self.spans {
            // Use baseline (bottom Y) for line grouping so superscripts group with their baseline text
            let span_y = span.bbox.b;
            let span_page = span.page_num;

            // Check if we've moved to a new page
            if let Some(prev_page) = last_page {
                if span_page != prev_page {
                    // Flush current line
                    if !current_line.is_empty() {
                        current_line.sort_by(|a, b| {
                            a.bbox
                                .l
                                .partial_cmp(&b.bbox.l)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                        lines.push(current_line);
                        current_line = Vec::new();
                    }
                    // Insert a blank line as page separator
                    lines.push(Vec::new());
                    last_y = None;
                }
            }

            if let Some(prev_y) = last_y {
                let y_gap = (span_y - prev_y).abs();
                // Use absolute threshold - with baseline grouping, superscripts have ~0 gap
                // Footer rows and table rows have ~6-8pt gaps
                let line_break_threshold = 5.0;

                if y_gap > line_break_threshold {
                    // Start a new line
                    if !current_line.is_empty() {
                        // Sort spans in the line by X coordinate (left to right)
                        current_line.sort_by(|a, b| {
                            a.bbox
                                .l
                                .partial_cmp(&b.bbox.l)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                        lines.push(current_line);
                        current_line = Vec::new();
                    }

                    // If the gap is significantly larger than normal, insert blank lines
                    if y_gap > Self::BLANK_LINE_THRESHOLD_POINTS {
                        // Calculate how many blank lines to insert based on the gap
                        let blank_lines = ((y_gap - Self::POINTS_PER_LINE) / Self::POINTS_PER_LINE)
                            .round() as usize;
                        // for _ in 0..blank_lines {
                        if blank_lines >= 1 {
                            lines.push(Vec::new());
                        }
                    }
                }
            }

            current_line.push(span);
            last_y = Some(span_y);
            last_page = Some(span_page);
        }

        if !current_line.is_empty() {
            // Sort the last line
            current_line.sort_by(|a, b| {
                a.bbox
                    .l
                    .partial_cmp(&b.bbox.l)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            lines.push(current_line);
        }

        lines
    }

    fn flush_string(&mut self) -> Result<(), OutputError> {
        if self.buf.len() != 0 {
            // buf_start_x, buf_end_x, and buf_start_y are already in flipped coordinates
            // (they come from position.m31/m32 where position = trm.post_transform(&self.flip_ctm))
            // So we can use them directly for the bounding box
            // Normalize coordinates so left is always < right (handles RTL text positioning)
            let bottom_left_x = self.buf_start_x.min(self.buf_end_x);
            let bottom_right_x = self.buf_start_x.max(self.buf_end_x);
            let bottom_y = self.buf_start_y;

            // Get the top Y by adding transformed font size
            let transformed_font_size_vec = self
                .buf_ctm
                .transform_vector(euclid::vec2(self.buf_font_size, self.buf_font_size));
            let transformed_font_size =
                (transformed_font_size_vec.x * transformed_font_size_vec.y).sqrt();
            let top_y = self.buf_start_y + transformed_font_size;

            let bbox = BoundingBox {
                t: top_y,
                r: bottom_right_x,
                b: bottom_y,
                l: bottom_left_x,
            };

            self.spans.push(TextSpan {
                text: self.buf.clone(),
                bbox,
                font_size: self.buf_font_size,
                page_num: self.current_page,
            });

            self.buf.clear();
        }
        Ok(())
    }

    pub(crate) fn begin_page(
        &mut self,
        page_num: u32,
        media_box: &MediaBox,
        _: Option<ArtBox>,
    ) -> Result<(), OutputError> {
        self.current_page = page_num;
        self.flip_ctm = Transform::new(1., 0., 0., -1., 0., media_box.ury - media_box.lly);
        Ok(())
    }

    pub(crate) fn end_page(&mut self) -> Result<(), OutputError> {
        self.flush_string()?;
        Ok(())
    }

    pub(crate) fn output_character(
        &mut self,
        trm: &Transform,
        width: f32,
        _spacing: f32,
        font_size: f32,
        char: &str,
    ) -> Result<(), OutputError> {
        let position = trm.then(&self.flip_ctm);
        let transformed_font_size_vec = trm.transform_vector(vec2(font_size, font_size));
        let transformed_font_size =
            (transformed_font_size_vec.x * transformed_font_size_vec.y).sqrt();
        let (x, y) = (position.m31, position.m32);

        let normalized_char = if char == "\t" { " " } else { char };

        if self.buf.is_empty() {
            // First character of a new span (either very first, or after flush)
            self.buf_start_x = x;
            self.buf_start_y = y;
            self.buf_font_size = font_size;
            self.buf_ctm = *trm;
            self.buf = normalized_char.to_owned();
        } else {
            // Have existing buffer - check if should flush or add to it

            // Fix buf_end_x if previous character had width=0 but actually occupies space
            // (PDF width metric is 0, but character visually extends to where next char starts)
            // Only do this for characters on the SAME line - don't merge across line breaks
            if self.buf_end_x == self.last_x
                && (y - self.last_y).abs() < transformed_font_size * 0.5
            {
                // Previous char had PDF width=0, we're on the same line
                // The actual visual end is where the current character starts
                self.buf_end_x = x;
            }

            let gap = x - self.buf_end_x;
            // Calculate gap ratio normalized by font size
            let gap_ratio = gap / transformed_font_size;

            let y_gap = (y - self.last_y).abs();
            let should_flush = y_gap > transformed_font_size * 1.5
                || (x < self.buf_end_x && y_gap > transformed_font_size * 0.5)
                || (gap_ratio.abs() > Self::CHAR_FLUSH_THRESHOLD_RATIO);

            if should_flush {
                self.flush_string()?;
                self.buf_start_x = x;
                self.buf_start_y = y;
                self.buf_font_size = font_size;
                self.buf_ctm = *trm;
                self.buf = normalized_char.to_owned();
            } else {
                // Don't insert space if the previous character was already whitespace
                let prev_char_is_space =
                    self.buf.chars().last().map_or(false, |c| c.is_whitespace());
                let will_insert_space =
                    !prev_char_is_space && (gap_ratio > Self::CHAR_SPACE_THRESHOLD_RATIO);

                if will_insert_space {
                    self.buf += " ";
                }
                self.buf += normalized_char;
            }
        }

        self.first_char = false;
        self.last_x = x;
        self.last_y = y;
        self.buf_end_x = x + width * transformed_font_size;

        Ok(())
    }

    pub(crate) fn begin_word(&mut self) -> Result<(), OutputError> {
        self.first_char = true;
        Ok(())
    }

    pub(crate) fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    pub(crate) fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}
