# Paper colour: white, sepia, grey

Date: 2026-10-27 · Status: small change, agreed in conversation

**Why.** A softer page than bright white for long reading.

**What.** One global setting, `paper`: white (the original), sepia
(`#F4ECD8`) or grey (`#D4D4D4`, light enough that black text keeps about
13:1 contrast). Colours are my call, to adjust after a look. Added the
same day, asked for: bisque (CSS `#FFE4C4`), warmer and a little darker
than sepia, shown after it.

**How.** Pages are drawn with the paper colour as the texture tint instead
of white: egui multiplies, so white paper takes the colour and black text
stays black. No re-render, no engine change; switching is instant. The tint
applies wherever a page is drawn: page images, the placeholder before a
page has rendered, sidebar thumbnails and their placeholders. Search
highlights on top are unchanged. Coloured content takes a slight cast;
that is inherent to tinting.

**Settings.** `settings.toml` gets `paper = "white" | "sepia" | "grey"`;
missing or unknown is white. The settings page gets a "Paper" row at the
top, above "New documents open with:" (it is not a new-document default):
three small squares filled with the colours, the chosen one framed in the
selection colour, the name ("Original", "Sepia", "Grey") on hover. A click
chooses and saves, and the document behind the dialog shows it at once.

**Approaches considered.**
- Tint at draw time (chosen): free, instant, can only darken.
- Change hayro's clear colour: misses every PDF that paints its own white
  background, and every scan.
- Remap pixels after rendering: needed for a dark mode with light text;
  deferred, see the backlog.

**Not now.** Global vs per-document is undecided; global for now. Dark
paper with inverted text goes to the backlog.
