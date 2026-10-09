//! Apple Vision-based OCR fallback for image-only PDF pages.
//!
//! Uses PDFKit to render each target page to a `CGImage`, then submits it
//! to `VNRecognizeTextRequest`. Observation bounding boxes (in normalized
//! image coordinates, origin at bottom-left, y-up) are scaled to PDF
//! user-space points so the resulting `TextSpan`s drop into the same
//! output pipeline as the content-stream extractor.

use core::ptr;

use objc2::AllocAnyThread;
use objc2::rc::{Retained, autoreleasepool};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage,
    CGImageAlphaInfo,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
use objc2_pdf_kit::{PDFDisplayBox, PDFDocument};
use objc2_vision::{
    VNDetectDocumentSegmentationRequest, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest,
    VNRequestTextRecognitionLevel,
};
use rayon::prelude::*;

use crate::error::OutputError;
use crate::types::{BoundingBox, TextSpan};

/// Horizontal and vertical pixels per PDF point when rasterising pages for
/// OCR. 600 DPI doubles the linear resolution over the typical 300-DPI
/// document sweet spot — expensive (4× bitmap memory) but gives Vision
/// enough pixels to disambiguate stroke-level ambiguities like `b` vs
/// `o` on smudged/bent scans where deskewing can't help.
const RENDER_DPI: f64 = 600.0;
const POINTS_PER_INCH: f64 = 72.0;

/// Run OCR on the given 1-indexed page numbers of a PDF held in memory.
///
/// Returns all recognised spans across the target pages. Failure on an
/// individual page is logged (via `tracing`) and skipped rather than
/// surfaced, so a single bad page doesn't poison the whole run.
pub(crate) fn run(
    pdf_bytes: &[u8],
    target_pages: &[u32],
    languages: &[String],
    deskew: bool,
    dump_dir: Option<&std::path::Path>,
) -> Result<Vec<Vec<TextSpan>>, OutputError> {
    if target_pages.is_empty() {
        return Ok(Vec::new());
    }

    let lines: Vec<Vec<TextSpan>> = target_pages
        .par_iter()
        .flat_map_iter(|&page_num| ocr_page(pdf_bytes, page_num, languages, deskew, dump_dir))
        .collect();

    Ok(lines)
}

/// Returns one `Vec<TextSpan>` per Vision observation — i.e. one entry per
/// recognised line of text — so callers can skip re-grouping. Vision's
/// line boundaries are always more reliable than a y-threshold heuristic,
/// especially on deskewed renders where anti-aliasing inflates bbox
/// heights.
fn ocr_page(
    pdf_bytes: &[u8],
    page_num: u32,
    languages: &[String],
    deskew: bool,
    dump_dir: Option<&std::path::Path>,
) -> Vec<Vec<TextSpan>> {
    if page_num == 0 {
        return Vec::new();
    }
    autoreleasepool(|_| -> Vec<Vec<TextSpan>> {
        let data = NSData::with_bytes(pdf_bytes);
        let Some(document) = (unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) })
        else {
            tracing::warn!(
                "ocr: PDFDocument failed to load from {} bytes",
                pdf_bytes.len()
            );
            return Vec::new();
        };
        let Some(page) = (unsafe { document.pageAtIndex((page_num - 1) as usize) }) else {
            return Vec::new();
        };
        let bounds = unsafe { page.boundsForBox(PDFDisplayBox::MediaBox) };
        let page_w = bounds.size.width;
        let page_h = bounds.size.height;
        if page_w <= 0.0 || page_h <= 0.0 {
            return Vec::new();
        }

        let Some(straight) = render_page(&page, page_w, page_h, 0.0) else {
            tracing::warn!("ocr: render failed for page {page_num}");
            return Vec::new();
        };

        if let Some(dir) = dump_dir {
            let path = dir.join(format!("page-{page_num}-pre.pgm"));
            if let Err(e) = dump_cgimage_pgm(&straight, &path) {
                tracing::warn!("ocr: failed to dump pre-deskew page {page_num}: {e}");
            }
        }

        // If the page is skewed, detect the angle on the straight render
        // and re-render the PDF directly with a rotated CTM. Going
        // through CIImage for the rotation inflates Vision's bbox heights
        // on rotated images and breaks our line-grouping heuristic; a
        // second PDFKit render produces a clean, axis-aligned bitmap.
        let (image, page_w, page_h) = if deskew {
            match detect_skew_angle(&straight) {
                Some(angle) if angle.abs() >= DESKEW_MIN_ANGLE_RAD => {
                    // `angle` is the angle of the document's top edge in
                    // Vision's y-up normalized space. Rotate the render
                    // by `-angle` to counter-rotate and axis-align the
                    // page. Note: document-segmentation detects the
                    // *paper outline*, not text baselines — on scans
                    // with torn/bent corners this can mislead.
                    match render_page(&page, page_w, page_h, -angle) {
                        Some(rotated) => {
                            let scale = RENDER_DPI / POINTS_PER_INCH;
                            let pw = CGImage::width(Some(&rotated)) as f64 / scale;
                            let ph = CGImage::height(Some(&rotated)) as f64 / scale;
                            (rotated, pw, ph)
                        }
                        None => (straight, page_w, page_h),
                    }
                }
                _ => (straight, page_w, page_h),
            }
        } else {
            (straight, page_w, page_h)
        };

        if let Some(dir) = dump_dir {
            let path = dir.join(format!("page-{page_num}-post.pgm"));
            if let Err(e) = dump_cgimage_pgm(&image, &path) {
                tracing::warn!("ocr: failed to dump post-deskew page {page_num}: {e}");
            }
        }

        let observations = match recognize(&image, languages) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("ocr: recognition failed for page {page_num}: {e}");
                return Vec::new();
            }
        };

        let mut lines: Vec<Vec<TextSpan>> = Vec::new();
        for obs in observations {
            let norm = unsafe { obs.boundingBox() };
            let candidates = obs.topCandidates(1);
            let Some(candidate) = candidates.iter().next() else {
                continue;
            };
            let text = candidate.string().to_string();
            if text.is_empty() {
                continue;
            }
            // Vision uses normalized coordinates with origin bottom-left
            // (y-up, matching PDF user space). pdf-extract's TextSpan
            // uses y-down page coordinates (small t = top of page), so
            // flip y against the page height.
            let l = (norm.origin.x * page_w) as f32;
            let r = ((norm.origin.x + norm.size.width) * page_w) as f32;
            let t = (page_h - (norm.origin.y + norm.size.height) * page_h) as f32;
            let b = (page_h - norm.origin.y * page_h) as f32;
            lines.push(vec![TextSpan {
                text,
                bbox: BoundingBox { t, r, b, l },
                font_name: "ocr:vision".to_string(),
                font_size: (b - t).max(1.0),
                page_num,
            }]);
        }
        lines
    })
}

fn render_page(
    page: &objc2_pdf_kit::PDFPage,
    page_w: f64,
    page_h: f64,
    rotation_rad: f64,
) -> Option<CFRetained<CGImage>> {
    let scale = RENDER_DPI / POINTS_PER_INCH;
    // When rotating, the rendered bitmap needs to be big enough to hold
    // the rotated page. For angle θ the axis-aligned bounding box of a
    // w×h rectangle rotated about its centre is
    // (|w cos θ| + |h sin θ|) × (|w sin θ| + |h cos θ|).
    let (out_w, out_h) = if rotation_rad == 0.0 {
        (page_w, page_h)
    } else {
        let c = rotation_rad.cos().abs();
        let s = rotation_rad.sin().abs();
        (page_w * c + page_h * s, page_w * s + page_h * c)
    };
    let px_w = (out_w * scale).round() as usize;
    let px_h = (out_h * scale).round() as usize;
    if px_w == 0 || px_h == 0 {
        return None;
    }
    // 8-bit grayscale. Vision converts to grayscale internally for text
    // recognition anyway, and scans are essentially monochrome — using
    // DeviceGray cuts the bitmap to 1 byte per pixel (¼ of RGBA).
    let bytes_per_row = px_w;
    let color_space = CGColorSpace::new_device_gray()?;
    let bitmap_info = CGImageAlphaInfo::None.0;
    let ctx = unsafe {
        CGBitmapContextCreate(
            ptr::null_mut(),
            px_w,
            px_h,
            8,
            bytes_per_row,
            Some(&color_space),
            bitmap_info,
        )
    }?;

    // White background — PDFKit draws transparently over whatever is in
    // the bitmap, and OCR on a transparent background is unreliable.
    CGContext::set_gray_fill_color(Some(&ctx), 1.0, 1.0);
    CGContext::fill_rect(
        Some(&ctx),
        CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: px_w as f64,
                height: px_h as f64,
            },
        },
    );
    CGContext::scale_ctm(Some(&ctx), scale, scale);
    if rotation_rad != 0.0 {
        // Rotate about the centre of the output canvas: translate centre
        // to origin, rotate, translate back by the *original* page
        // centre so the page still lands inside the (larger) canvas.
        CGContext::translate_ctm(Some(&ctx), out_w / 2.0, out_h / 2.0);
        CGContext::rotate_ctm(Some(&ctx), rotation_rad);
        CGContext::translate_ctm(Some(&ctx), -page_w / 2.0, -page_h / 2.0);
    }
    unsafe {
        page.drawWithBox_toContext(PDFDisplayBox::MediaBox, &ctx);
    }

    CGBitmapContextCreateImage(Some(&ctx))
}

/// Minimum rotation magnitude (radians) worth correcting. Below this
/// threshold the re-render cost outweighs any accuracy gain —
/// Vision's internal tolerance already covers residuals ≲ 0.5°.
const DESKEW_MIN_ANGLE_RAD: f64 = 0.0087;

/// Detect page skew by asking Vision for the document rectangle and
/// returning the rotation angle (radians, counter-clockwise positive)
/// of the page's top edge relative to the image frame. Returns `None`
/// when no page quad is detected.
///
/// `VNDetectHorizonRequest` would be the natural pick, but it only
/// finds horizons in landscape imagery — document scans return zero
/// observations. Document segmentation is the page-detector equivalent
/// used by the Continuity Camera document-scan pipeline.
fn detect_skew_angle(image: &CGImage) -> Option<f64> {
    let request = unsafe { VNDetectDocumentSegmentationRequest::new() };
    let options: Retained<NSDictionary<objc2_vision::VNImageOption, objc2::runtime::AnyObject>> =
        NSDictionary::new();
    let handler = unsafe {
        VNImageRequestHandler::initWithCGImage_options(
            VNImageRequestHandler::alloc(),
            image,
            &options,
        )
    };
    let request_super: Retained<VNRequest> = request.clone().into_super().into_super();
    let requests: Retained<NSArray<VNRequest>> = NSArray::from_retained_slice(&[request_super]);
    handler.performRequests_error(&requests).ok()?;
    let results = unsafe { request.results() }?;
    let observation = results.iter().next()?;
    let tl = unsafe { observation.topLeft() };
    let tr = unsafe { observation.topRight() };
    let dx = (tr.x - tl.x) as f64;
    let dy = (tr.y - tl.y) as f64;
    // atan2 in Vision's y-up normalized space.
    Some(dy.atan2(dx))
}

fn recognize(
    image: &CGImage,
    languages: &[String],
) -> Result<Retained<NSArray<objc2_vision::VNRecognizedTextObservation>>, String> {
    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    request.setUsesLanguageCorrection(true);
    // Use the newest revision this OS supports rather than hard-pinning
    // Revision 3. Revision 4 (macOS 14+) and 5 (macOS 15+) both improve
    // recognition quality; earlier OSes fall back to their default.
    unsafe {
        request.setRevision(VNRequest::currentRevision());
    }
    // An explicit language list narrows the model and is faster than
    // auto-detect. An empty list falls back to Vision's language
    // identification (revision 3+).
    if languages.is_empty() {
        request.setAutomaticallyDetectsLanguage(true);
    } else {
        request.setAutomaticallyDetectsLanguage(false);
        let ns_strings: Vec<Retained<NSString>> =
            languages.iter().map(|s| NSString::from_str(s)).collect();
        let ns_array: Retained<NSArray<NSString>> = NSArray::from_retained_slice(&ns_strings);
        request.setRecognitionLanguages(&ns_array);
    }

    let options: Retained<NSDictionary<objc2_vision::VNImageOption, objc2::runtime::AnyObject>> =
        NSDictionary::new();
    let handler = unsafe {
        VNImageRequestHandler::initWithCGImage_options(
            VNImageRequestHandler::alloc(),
            image,
            &options,
        )
    };

    // Upcast VNRecognizeTextRequest → VNImageBasedRequest → VNRequest.
    let request_super: Retained<VNRequest> = request.clone().into_super().into_super();
    let requests: Retained<NSArray<VNRequest>> = NSArray::from_retained_slice(&[request_super]);

    handler
        .performRequests_error(&requests)
        .map_err(|e| format!("{e:?}"))?;

    Ok(request
        .results()
        .unwrap_or_else(|| NSArray::from_retained_slice(&[])))
}

/// Write a grayscale `CGImage` to disk as a PGM (`P5`) file. PGM is a
/// zero-dependency binary format that Preview, `convert`, and most image
/// viewers open natively, making it a convenient debug dump of the
/// exact bitmap Vision saw.
fn dump_cgimage_pgm(image: &CGImage, path: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;

    let w = CGImage::width(Some(image));
    let h = CGImage::height(Some(image));
    let stride = CGImage::bytes_per_row(Some(image));
    let provider = CGImage::data_provider(Some(image))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "no data provider"))?;
    let cf_data = objc2_core_graphics::CGDataProvider::data(Some(&provider))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "no data"))?;
    let len = cf_data.length() as usize;
    let ptr = cf_data.byte_ptr();
    if ptr.is_null() || len < stride.saturating_mul(h) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "short pixel buffer",
        ));
    }
    // SAFETY: CFData guarantees the byte range [ptr, ptr+len) is valid
    // for read for the lifetime of `cf_data`.
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    write!(f, "P5\n{w} {h}\n255\n")?;
    // Strip any row padding CoreGraphics may have added.
    for row in 0..h {
        let start = row * stride;
        f.write_all(&bytes[start..start + w])?;
    }
    f.flush()
}
