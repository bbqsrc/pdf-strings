"""
Python bindings for pdf-strings

Usage:
    from pdf_strings import from_path

    output = from_path("document.pdf")

    string = str(output) # or .to_string(), Plain text output
    pretty_string = f"{output:#}" # or .to_string_pretty(), Pretty formatted output

    # Get the spans and their bounding boxes
    for line in output.lines:
        for span in line:
            print(f"{span.text} at {span.bbox}")
"""

import ctypes
import sys
from pathlib import Path
from typing import List, Optional
from dataclasses import dataclass

# Locate the shared library
# First check if it's in the package directory (installed wheel)
# Then fall back to the dev path (editable install/development)
_package_dir = Path(__file__).parent
_dev_lib_path = _package_dir.parent.parent / "target" / "release"

if sys.platform == "darwin":
    _lib_name = "libpdf_strings_ffi.dylib"
elif sys.platform == "win32":
    _lib_name = "pdf_strings_ffi.dll"
else:
    _lib_name = "libpdf_strings_ffi.so"

# Try package directory first (installed)
_lib_file = _package_dir / _lib_name
if not _lib_file.exists():
    # Fall back to dev path
    _lib_file = _dev_lib_path / _lib_name

_lib = ctypes.CDLL(str(_lib_file))

# Define C structures
class _FfiBoundingBox(ctypes.Structure):
    _fields_ = [
        ("t", ctypes.c_float),
        ("r", ctypes.c_float),
        ("b", ctypes.c_float),
        ("l", ctypes.c_float),
    ]

class _FfiTextSpan(ctypes.Structure):
    _fields_ = [
        ("text", ctypes.c_void_p),
        ("bbox", _FfiBoundingBox),
        ("font_size", ctypes.c_float),
        ("page", ctypes.c_uint32),
    ]

# Define function signatures
_lib.pdf_extract_from_path.argtypes = [ctypes.c_char_p]
_lib.pdf_extract_from_path.restype = ctypes.c_void_p

_lib.pdf_extract_from_path_with_password.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
_lib.pdf_extract_from_path_with_password.restype = ctypes.c_void_p

_lib.pdf_extract_from_bytes.argtypes = [ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t]
_lib.pdf_extract_from_bytes.restype = ctypes.c_void_p

_lib.pdf_extract_from_bytes_with_password.argtypes = [ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t, ctypes.c_char_p]
_lib.pdf_extract_from_bytes_with_password.restype = ctypes.c_void_p

_lib.pdf_line_count.argtypes = [ctypes.c_void_p]
_lib.pdf_line_count.restype = ctypes.c_size_t

_lib.pdf_line_span_count.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
_lib.pdf_line_span_count.restype = ctypes.c_size_t

_lib.pdf_get_span.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t, ctypes.POINTER(_FfiTextSpan)]
_lib.pdf_get_span.restype = ctypes.c_int

_lib.pdf_output_to_string.argtypes = [ctypes.c_void_p]
_lib.pdf_output_to_string.restype = ctypes.c_void_p

_lib.pdf_output_to_string_pretty.argtypes = [ctypes.c_void_p]
_lib.pdf_output_to_string_pretty.restype = ctypes.c_void_p

_lib.pdf_string_free.argtypes = [ctypes.c_void_p]
_lib.pdf_string_free.restype = None

_lib.pdf_span_text_free.argtypes = [ctypes.c_void_p]
_lib.pdf_span_text_free.restype = None

_lib.pdf_output_free.argtypes = [ctypes.c_void_p]
_lib.pdf_output_free.restype = None

_lib.pdf_last_error.argtypes = []
_lib.pdf_last_error.restype = ctypes.c_char_p


@dataclass
class BoundingBox:
    """Bounding box coordinates for a text span"""
    top: float
    right: float
    bottom: float
    left: float

    def __str__(self) -> str:
        return f"({self.top:.1f}, {self.right:.1f}, {self.bottom:.1f}, {self.left:.1f})"

@dataclass
class TextSpan:
    """A span of text with position and metadata"""
    text: str
    bbox: BoundingBox
    font_size: float
    page: int


@dataclass
class TextOutput:
    """Extracted text output with structured lines and spans"""
    lines: List[List[TextSpan]]
    _handle: int | None = None

    def __repr__(self) -> str:
        return f"TextOutput(<{len(self.lines)} lines>)"

    def __str__(self) -> str:
        return self.to_string()

    def __format__(self, format_spec: str) -> str:
        """Format output. Use '#' for pretty formatting: f'{output:#}'"""
        if '#' in format_spec:
            return self.to_string_pretty()
        return str(self)
    
    def to_string(self) -> str:
        """Get plain text output"""
        if not self._handle:
            raise RuntimeError("TextOutput has no handle")
        str_ptr = _lib.pdf_output_to_string(self._handle)
        if not str_ptr:
            error_ptr = _lib.pdf_last_error()
            error_msg = ctypes.c_char_p(error_ptr).value.decode('utf-8') if error_ptr else "Unknown error"
            raise RuntimeError(f"Failed to get text: {error_msg}")
        text = ctypes.c_char_p(str_ptr).value.decode('utf-8')
        _lib.pdf_string_free(str_ptr)
        return text

    def to_string_pretty(self) -> str:
        """Get pretty formatted text output (preserves spatial layout)"""
        if not self._handle:
            raise RuntimeError("TextOutput has no handle")
        str_ptr = _lib.pdf_output_to_string_pretty(self._handle)
        if not str_ptr:
            error_ptr = _lib.pdf_last_error()
            error_msg = ctypes.c_char_p(error_ptr).value.decode('utf-8') if error_ptr else "Unknown error"
            raise RuntimeError(f"Failed to get text: {error_msg}")
        text = ctypes.c_char_p(str_ptr).value.decode('utf-8')
        _lib.pdf_string_free(str_ptr)
        return text


class _PdfHandle:
    def __init__(self, handle: int):
        if not handle:
            error_ptr = _lib.pdf_last_error()
            error_msg = ctypes.c_char_p(error_ptr).value.decode('utf-8') if error_ptr else "Unknown error"
            raise RuntimeError(f"Failed to extract PDF: {error_msg}")
        self.handle = handle

    @property
    def line_count(self) -> int:
        return _lib.pdf_line_count(self.handle)

    def line_span_count(self, line_idx: int) -> int:
        return _lib.pdf_line_span_count(self.handle, line_idx)

    def get_span(self, line_idx: int, span_idx: int) -> TextSpan:
        ffi_span = _FfiTextSpan()
        result = _lib.pdf_get_span(self.handle, line_idx, span_idx, ctypes.byref(ffi_span))

        if result != 0:
            error_ptr = _lib.pdf_last_error()
            error_msg = ctypes.c_char_p(error_ptr).value.decode('utf-8') if error_ptr else "Unknown error"
            raise RuntimeError(f"Failed to get span: {error_msg}")

        text_ptr = ffi_span.text
        text = ctypes.c_char_p(text_ptr).value.decode('utf-8')
        _lib.pdf_span_text_free(text_ptr)

        bbox = BoundingBox(
            top=ffi_span.bbox.t,
            right=ffi_span.bbox.r,
            bottom=ffi_span.bbox.b,
            left=ffi_span.bbox.l,
        )

        return TextSpan(
            text=text,
            bbox=bbox,
            font_size=ffi_span.font_size,
            page=ffi_span.page,
        )

    def to_output(self) -> TextOutput:
        lines: List[List[TextSpan]] = []
        for i in range(self.line_count):
            line: List[TextSpan] = []
            span_count = self.line_span_count(i)
            for j in range(span_count):
                line.append(self.get_span(i, j))
            lines.append(line)
        return TextOutput(lines=lines, _handle=self.handle)

    def close(self):
        if self.handle:
            _lib.pdf_output_free(self.handle)
            self.handle = None

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()


def from_path(path: str, *, password: Optional[str] = None) -> TextOutput:
    """
    Extract text from a PDF file.

    Args:
        path: Path to the PDF file
        password: Optional password for encrypted PDFs

    Returns:
        TextOutput containing structured lines and spans with bounding boxes

    Raises:
        RuntimeError: If PDF extraction fails
    """
    path_bytes = path.encode('utf-8')
    if password:
        password_bytes = password.encode('utf-8')
        handle = _lib.pdf_extract_from_path_with_password(path_bytes, password_bytes)
    else:
        handle = _lib.pdf_extract_from_path(path_bytes)
    pdf = _PdfHandle(handle)
    return pdf.to_output()


def from_bytes(data: bytes, *, password: Optional[str] = None) -> TextOutput:
    """
    Extract text from PDF bytes.

    Args:
        data: PDF file contents as bytes
        password: Optional password for encrypted PDFs

    Returns:
        TextOutput containing structured lines and spans with bounding boxes

    Raises:
        RuntimeError: If PDF extraction fails
    """
    arr = (ctypes.c_uint8 * len(data)).from_buffer_copy(data)
    if password:
        password_bytes = password.encode('utf-8')
        handle = _lib.pdf_extract_from_bytes_with_password(arr, len(data), password_bytes)
    else:
        handle = _lib.pdf_extract_from_bytes(arr, len(data))
    pdf = _PdfHandle(handle)
    return pdf.to_output()


__all__ = ["from_path", "from_bytes", "BoundingBox", "TextSpan", "TextOutput"]
