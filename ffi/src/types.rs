use std::os::raw::c_char;

#[repr(C)]
pub struct FfiBoundingBox {
    pub t: f32,
    pub r: f32,
    pub b: f32,
    pub l: f32,
}

impl From<&pdf_strings::BoundingBox> for FfiBoundingBox {
    fn from(bbox: &pdf_strings::BoundingBox) -> Self {
        FfiBoundingBox {
            t: bbox.t,
            r: bbox.r,
            b: bbox.b,
            l: bbox.l,
        }
    }
}

#[repr(C)]
pub struct FfiTextSpan {
    pub text: *mut c_char,
    pub bbox: FfiBoundingBox,
    pub font_size: f32,
    pub page: u32,
}

impl From<&pdf_strings::TextSpan> for FfiTextSpan {
    fn from(span: &pdf_strings::TextSpan) -> Self {
        let text = std::ffi::CString::new(span.text.clone())
            .unwrap_or_else(|_| std::ffi::CString::new("").unwrap());

        FfiTextSpan {
            text: text.into_raw(),
            bbox: FfiBoundingBox::from(&span.bbox),
            font_size: span.font_size,
            page: span.page_num,
        }
    }
}

impl FfiTextSpan {
    pub fn from_span(span: &pdf_strings::TextSpan) -> Self {
        let text = std::ffi::CString::new(span.text.clone())
            .unwrap_or_else(|_| std::ffi::CString::new("").unwrap());

        FfiTextSpan {
            text: text.into_raw(),
            bbox: FfiBoundingBox::from(&span.bbox),
            font_size: span.font_size,
            page: span.page_num,
        }
    }
}
