#!/usr/bin/env python3
"""Quick test of Python bindings"""

import sys
from pathlib import Path

# Add parent to path so we can import pdf_strings
sys.path.insert(0, str(Path(__file__).parent))

from pdf_strings import extract_text

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python test_python.py <path-to-pdf>")
        sys.exit(1)

    pdf_path = sys.argv[1]
    print(f"Extracting text from: {pdf_path}")

    try:
        output = extract_text(pdf_path)
        print(f"Found {len(output.lines)} lines")

        # Print first few lines
        for i, line in enumerate(output.lines[:10]):
            if not line:
                print(f"Line {i}: (empty)")
                continue

            texts = [span.text for span in line]
            print(f"Line {i}: {' '.join(texts)}")

    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
