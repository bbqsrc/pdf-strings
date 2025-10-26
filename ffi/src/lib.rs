mod types;

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

pub use types::{FfiBoundingBox, FfiTextSpan};

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = RefCell::new(None);
}

fn set_last_error(err: String) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = CString::new(err).ok();
    });
}

pub struct PdfOutputHandle {
    output: pdf_strings::TextOutput,
}

#[no_mangle]
pub extern "C" fn pdf_extract_from_path(path: *const c_char) -> *mut PdfOutputHandle {
    if path.is_null() {
        set_last_error("Path pointer is null".to_string());
        return ptr::null_mut();
    }

    let path_str = unsafe {
        match CStr::from_ptr(path).to_str() {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("Invalid UTF-8 in path: {}", e));
                return ptr::null_mut();
            }
        }
    };

    match pdf_strings::from_path(path_str) {
        Ok(output) => {
            let handle = Box::new(PdfOutputHandle { output });
            Box::into_raw(handle)
        }
        Err(e) => {
            set_last_error(format!("Failed to extract PDF: {}", e));
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn pdf_extract_from_bytes(data: *const u8, len: usize) -> *mut PdfOutputHandle {
    if data.is_null() {
        set_last_error("Data pointer is null".to_string());
        return ptr::null_mut();
    }

    let bytes = unsafe { std::slice::from_raw_parts(data, len) };

    match pdf_strings::from_bytes(bytes) {
        Ok(output) => {
            let handle = Box::new(PdfOutputHandle { output });
            Box::into_raw(handle)
        }
        Err(e) => {
            set_last_error(format!("Failed to extract PDF: {}", e));
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn pdf_line_count(handle: *const PdfOutputHandle) -> usize {
    if handle.is_null() {
        set_last_error("Handle is null".to_string());
        return 0;
    }

    let handle = unsafe { &*handle };
    handle.output.lines().len()
}

#[no_mangle]
pub extern "C" fn pdf_line_span_count(handle: *const PdfOutputHandle, line_idx: usize) -> usize {
    if handle.is_null() {
        set_last_error("Handle is null".to_string());
        return 0;
    }

    let handle = unsafe { &*handle };

    match handle.output.lines().get(line_idx) {
        Some(line) => line.len(),
        None => {
            set_last_error(format!("Line index {} out of bounds", line_idx));
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn pdf_get_span(
    handle: *const PdfOutputHandle,
    line_idx: usize,
    span_idx: usize,
    out: *mut FfiTextSpan,
) -> i32 {
    if handle.is_null() {
        set_last_error("Handle is null".to_string());
        return -1;
    }

    if out.is_null() {
        set_last_error("Output pointer is null".to_string());
        return -1;
    }

    let handle = unsafe { &*handle };

    let line = match handle.output.lines().get(line_idx) {
        Some(line) => line,
        None => {
            set_last_error(format!("Line index {} out of bounds", line_idx));
            return -1;
        }
    };

    let span = match line.get(span_idx) {
        Some(span) => span,
        None => {
            set_last_error(format!("Span index {} out of bounds", span_idx));
            return -1;
        }
    };

    unsafe {
        *out = FfiTextSpan::from_span(span);
    }

    0
}

#[no_mangle]
pub extern "C" fn pdf_extract_from_path_with_password(
    path: *const c_char,
    password: *const c_char,
) -> *mut PdfOutputHandle {
    if path.is_null() {
        set_last_error("Path pointer is null".to_string());
        return ptr::null_mut();
    }

    let path_str = unsafe {
        match CStr::from_ptr(path).to_str() {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("Invalid UTF-8 in path: {}", e));
                return ptr::null_mut();
            }
        }
    };

    let password_str = if password.is_null() {
        None
    } else {
        unsafe {
            match CStr::from_ptr(password).to_str() {
                Ok(s) => Some(s.to_string()),
                Err(e) => {
                    set_last_error(format!("Invalid UTF-8 in password: {}", e));
                    return ptr::null_mut();
                }
            }
        }
    };

    let extractor = if let Some(pwd) = password_str {
        pdf_strings::PdfExtractor::builder().password(pwd).build()
    } else {
        pdf_strings::PdfExtractor::default()
    };

    match extractor.from_path(path_str) {
        Ok(output) => {
            let handle = Box::new(PdfOutputHandle { output });
            Box::into_raw(handle)
        }
        Err(e) => {
            set_last_error(format!("Failed to extract PDF: {}", e));
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn pdf_extract_from_bytes_with_password(
    data: *const u8,
    len: usize,
    password: *const c_char,
) -> *mut PdfOutputHandle {
    if data.is_null() {
        set_last_error("Data pointer is null".to_string());
        return ptr::null_mut();
    }

    let bytes = unsafe { std::slice::from_raw_parts(data, len) };

    let password_str = if password.is_null() {
        None
    } else {
        unsafe {
            match CStr::from_ptr(password).to_str() {
                Ok(s) => Some(s.to_string()),
                Err(e) => {
                    set_last_error(format!("Invalid UTF-8 in password: {}", e));
                    return ptr::null_mut();
                }
            }
        }
    };

    let extractor = if let Some(pwd) = password_str {
        pdf_strings::PdfExtractor::builder().password(pwd).build()
    } else {
        pdf_strings::PdfExtractor::default()
    };

    match extractor.from_bytes(bytes) {
        Ok(output) => {
            let handle = Box::new(PdfOutputHandle { output });
            Box::into_raw(handle)
        }
        Err(e) => {
            set_last_error(format!("Failed to extract PDF: {}", e));
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn pdf_output_to_string(handle: *const PdfOutputHandle) -> *mut c_char {
    if handle.is_null() {
        set_last_error("Handle is null".to_string());
        return ptr::null_mut();
    }

    let handle = unsafe { &*handle };
    let text = handle.output.to_string();

    match CString::new(text) {
        Ok(c_str) => c_str.into_raw(),
        Err(e) => {
            set_last_error(format!("Failed to convert text to C string: {}", e));
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn pdf_output_to_string_pretty(handle: *const PdfOutputHandle) -> *mut c_char {
    if handle.is_null() {
        set_last_error("Handle is null".to_string());
        return ptr::null_mut();
    }

    let handle = unsafe { &*handle };
    let text = handle.output.to_string_pretty();

    match CString::new(text) {
        Ok(c_str) => c_str.into_raw(),
        Err(e) => {
            set_last_error(format!("Failed to convert text to C string: {}", e));
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn pdf_string_free(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            drop(CString::from_raw(s));
        }
    }
}

#[no_mangle]
pub extern "C" fn pdf_span_text_free(text: *mut c_char) {
    if !text.is_null() {
        unsafe {
            drop(CString::from_raw(text));
        }
    }
}

#[no_mangle]
pub extern "C" fn pdf_output_free(handle: *mut PdfOutputHandle) {
    if !handle.is_null() {
        unsafe {
            drop(Box::from_raw(handle));
        }
    }
}

#[no_mangle]
pub extern "C" fn pdf_last_error() -> *const c_char {
    LAST_ERROR.with(|e| match e.borrow().as_ref() {
        Some(err) => err.as_ptr(),
        None => ptr::null(),
    })
}
