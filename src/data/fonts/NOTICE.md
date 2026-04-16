# Embedded fonts

These OpenType fonts are bundled to recover Unicode from bitmap Type3
CharProcs in dvips-era LaTeX PDFs. Each font is used only as a reference
for shape matching — we rasterise glyphs at runtime and compare against
extracted CharProc bitmaps.

## NewCM10-Regular.otf, NewCM10-Bold.otf, NewCM10-Italic.otf, NewCM10-BoldItalic.otf, NewCMMath-Regular.otf

**New Computer Modern.** A faithful OpenType conversion of Donald E.
Knuth's original Computer Modern family by Antonis Tsolomitis. This is
the Unicode-complete successor to the typefaces dvips rendered in 1996
— Latin Modern and cm-unicode are alternate derivatives, but New CM
stays closest to the original Metafont outlines.

- Upstream: <https://ctan.org/pkg/newcomputermodern>
- License: GUST Font License (LPPL-compatible)

The GUST Font License permits redistribution in derivative works
including binary software distributions. No naming restriction
conflicts with use here as reference data.
