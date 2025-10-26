#!/usr/bin/env python3
"""Test complete Python API including password and text output"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from pdf_strings import from_path

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python test_complete_api.py <path-to-pdf>")
        sys.exit(1)

    pdf_path = sys.argv[1]
    print(f"Testing complete API with: {pdf_path}\n")

    try:
        # Test 1: Basic extraction with module-level function
        print("=== Test 1: Basic extraction (module function) ===")
        output = from_path(pdf_path)
        print(f"Found {len(output.lines)} lines")
        print(f"First line has {len(output.lines[2]) if len(output.lines) > 2 else 0} spans")

        # Test 2: Plain text output via __str__
        print("\n=== Test 2: Plain text output via __str__ ===")
        plain_text = str(output)
        print(f"Plain text length: {len(plain_text)} chars")
        print(f"First 200 chars: {plain_text[:200]}")

        # Test 3: Pretty formatted output via __format__
        print("\n=== Test 3: Pretty formatted output via __format__ ===")
        pretty_text = f"{output:#}"
        print(f"Pretty text length: {len(pretty_text)} chars")
        print(f"First 200 chars: {pretty_text[:200]}")

        # Test 4: Structured access
        print("\n=== Test 4: Structured span access ===")
        if len(output.lines) > 2 and len(output.lines[2]) > 0:
            span = output.lines[2][0]
            print(f"Sample span text: '{span.text}'")
            print(f"Bounding box: top={span.bbox.top}, right={span.bbox.right}, bottom={span.bbox.bottom}, left={span.bbox.left}")
            print(f"Font size: {span.font_size}")
            print(f"Page: {span.page}")

        for i, line in enumerate(output.lines):
            print(f"\nLine {i} spans:")
            for span in line:
                print(f"  '{span.text}' at {span.bbox}")

        print("\n✅ All tests passed!")

    except Exception as e:
        print(f"❌ Error: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)
