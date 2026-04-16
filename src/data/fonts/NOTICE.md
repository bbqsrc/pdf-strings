# Embedded fonts

These OpenType fonts are bundled to recover Unicode from bitmap Type3
CharProcs in dvips-era LaTeX PDFs. Each font is used only as a reference
for shape matching — we rasterise glyphs at runtime and compare against
extracted CharProc bitmaps.

## cmunrm.otf, cmunbx.otf, cmunti.otf

**Computer Modern Unicode.** Converted from METAFONT sources by
Andrey V. Panov.

- Upstream: <https://sourceforge.net/projects/cm-unicode/>
- License: SIL Open Font License 1.1 (OFL)

## lmroman10-regular.otf, lmroman10-bold.otf, lmroman10-italic.otf, lmromanslant10-regular.otf, lmsans10-regular.otf, lmsans10-bold.otf, lmmono10-regular.otf

**Latin Modern.** Maintained by Bogusław Jackowski and Janusz M. Nowacki
on behalf of the TeX user groups (CSTUG, DANTE eV, GUST, GUTenberg, NTG,
TUG). Derived from Donald E. Knuth's Computer Modern.

- Upstream: <https://www.gust.org.pl/projects/e-foundry/latin-modern>
- License: GUST Font License (LPPL-compatible)

## latinmodern-math.otf

**Latin Modern Math.** Companion math font for the Latin Modern family.
Provides Greek, blackboard bold, fraktur, script, and the full range of
math operators and symbols used in technical typesetting.

- Upstream: <https://www.gust.org.pl/projects/e-foundry/lm-math>
- License: GUST Font License (LPPL-compatible)

Both licenses permit redistribution in derivative works including binary
software distributions. Neither imposes naming restrictions that conflict
with their use here as reference data.
