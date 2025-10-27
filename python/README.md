# pdf-strings

Extract text from PDFs with position data.

## Installation

```bash
pip install pdf-strings
```

## Quick Start

```python
from pdf_strings import from_path

# Extract text from a PDF
output = from_path("document.pdf")
print(output)  # Plain text
```

## API Reference

### Functions

#### `from_path(path: str, *, password: str | None = None) -> TextOutput`

Extract text from a PDF file.

**Parameters:**
- `path` (str): Path to the PDF file
- `password` (str, optional): Password for encrypted PDFs

**Returns:** `TextOutput` object containing structured lines and spans

**Example:**
```python
from pdf_strings import from_path

# Basic usage
output = from_path("document.pdf")

# With password
output = from_path("encrypted.pdf", password="secret")
```

#### `from_bytes(data: bytes, *, password: str | None = None) -> TextOutput`

Extract text from PDF bytes.

**Parameters:**
- `data` (bytes): PDF file contents as bytes
- `password` (str, optional): Password for encrypted PDFs

**Returns:** `TextOutput` object containing structured lines and spans

**Example:**
```python
from pdf_strings import from_bytes

with open("document.pdf", "rb") as f:
    data = f.read()

output = from_bytes(data)
```

### Classes

#### `TextOutput`

Container for extracted text with structured data.

**Attributes:**
- `lines` (List[List[TextSpan]]): Lines of text, each containing multiple spans

**Methods:**

##### `to_string() -> str`

Get plain text output (concatenates all text with spaces).

```python
output = from_path("document.pdf")
plain_text = output.to_string()
# or simply:
plain_text = str(output)
```

##### `to_string_pretty() -> str`

Get formatted text that preserves spatial layout using a character grid.

```python
output = from_path("document.pdf")
formatted_text = output.to_string_pretty()
# or using format spec:
formatted_text = f"{output:#}"
```

**Magic Methods:**
- `__str__()`: Returns plain text (same as `to_string()`)
- `__format__(format_spec)`: Use `#` for pretty formatting: `f"{output:#}"`

#### `TextSpan`

A span of text with position and metadata.

**Attributes:**
- `text` (str): The text content
- `bbox` (BoundingBox): Bounding box coordinates
- `font_size` (float): Font size in points
- `page` (int): Page number (0-indexed)

**Example:**
```python
output = from_path("document.pdf")
for line in output.lines:
    for span in line:
        print(f"'{span.text}' at size {span.font_size}pt on page {span.page}")
        print(f"  Position: {span.bbox}")
```

#### `BoundingBox`

Bounding box coordinates for a text span.

**Attributes:**
- `top` (float): Top coordinate
- `right` (float): Right coordinate
- `bottom` (float): Bottom coordinate
- `left` (float): Left coordinate

**String representation:** `(top, right, bottom, left)` following HTML margin convention.

**Example:**
```python
bbox = span.bbox
print(f"Top-left: ({bbox.left}, {bbox.top})")
print(f"Width: {bbox.right - bbox.left}")
print(f"Height: {bbox.top - bbox.bottom}")
```

## Usage Examples

### Extract all text

```python
from pdf_strings import from_path

output = from_path("document.pdf")
print(output.to_string())
```

### Preserve layout

```python
from pdf_strings import from_path

output = from_path("invoice.pdf")
# Character grid rendering preserves columns and spacing
print(output.to_string_pretty())
```

### Access structured data

```python
from pdf_strings import from_path

output = from_path("document.pdf")

for line_idx, line in enumerate(output.lines):
    print(f"Line {line_idx}:")
    for span in line:
        print(f"  {span.text}")
        print(f"    Font size: {span.font_size}")
        print(f"    Position: ({span.bbox.left}, {span.bbox.top})")
        print(f"    Page: {span.page}")
```

### Find text in specific regions

```python
from pdf_strings import from_path

output = from_path("document.pdf")

# Find text in the top-right corner
for line in output.lines:
    for span in line:
        if span.bbox.top < 100 and span.bbox.left > 400:
            print(f"Top-right text: {span.text}")
```

### Extract tables by position

```python
from pdf_strings import from_path

output = from_path("table.pdf")

# Group spans by their vertical position (rows)
rows = {}
for line in output.lines:
    for span in line:
        row_key = round(span.bbox.top / 10) * 10  # Group by ~10pt vertical bands
        if row_key not in rows:
            rows[row_key] = []
        rows[row_key].append((span.bbox.left, span.text))

# Print rows sorted by vertical position
for y_pos in sorted(rows.keys(), reverse=True):
    # Sort spans in each row by horizontal position
    row_spans = sorted(rows[y_pos], key=lambda x: x[0])
    print(" | ".join(text for _, text in row_spans))
```

## Features

- Plain text extraction
- Spatial layout preservation via character grid
- Bounding box coordinates for every text span
- Font size and page information
- Password-protected PDF support
- Handles complex fonts, rotated text, and multi-column layouts
- Works with all Python 3.11+ versions

## License

MIT