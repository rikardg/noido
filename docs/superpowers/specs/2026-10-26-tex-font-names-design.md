# TeX font names

Date: 2026-10-26 · Status: small change, agreed in conversation

## Intent

PDFs from classic TeX name their fonts by Knuth's abbreviations: `CMR9`,
`CMMI9`, `CMSY8`. The document info page shows them as they are, which
says little. Show them as the typefaces they are.

## Approaches (agreed: 1)

1. **A table of the TeX abbreviations** in `noido-core/src/fonts.rs`, next
   to the other naming rules. Small and pure; helps only TeX fonts.
   Chosen.
2. Read the family name from the embedded font program (Type 1, CFF,
   TrueType). Works for every embedded font and would also fix run-together
   names, but means parsing three formats. To the backlog.

## Behaviour

- A name that is a known abbreviation followed by a design size (`CMR9`,
  `CMMIB10`, `SFRM1000`), after the subset prefix, becomes its family and
  style:
  - Computer Modern (`CM…`, and CM-Super's `SF…`): Roman, Bold, Bold
    Extended, Italic, Slanted, Math Italic, Symbol, Math Extension,
    Typewriter, Sans Serif, Caps and Small Caps, and their kin.
  - AMS Symbols (`MSAM`, `MSBM`): A, B.
  - Euler (`EUFM`/`EUFB` Fraktur, `EURM`/`EURB` Roman, `EUSM`/`EUSB`
    Script, `EUEX` Extension), as families of their own.
- The design size is dropped (my call): it is an optical size, and the
  hover over a family still shows the raw names.
- Unknown abbreviations, and names without the size, stay as they are.
- Latin Modern (`LMRoman10-Regular`) already reads well; unchanged.
