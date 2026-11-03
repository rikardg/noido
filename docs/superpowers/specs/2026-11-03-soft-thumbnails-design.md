# Soft stand-ins and a fade to the sharp page

Date: 2026-11-03 · Status: small change, agreed in conversation

## Intent

While the page renderer can't keep up (scrolling, jumping), the main view
shows the page's 120 px thumbnail stretched over it, which looks blocky.
Blurred, it reads as a soft preview instead. And when the sharp render
arrives, it fades in over the stand-in rather than replacing it in one
frame. The sidebar gets the same: its sharp renders stay sharp, the
thumbnail it falls back to while scrolled quickly is the soft one, and its
sharp render fades in over it. Tried in a quick experiment first; these
are the settings that stuck.

## Behaviour

- **Soft thumbnails.** Each thumbnail from the thumbnailer is blurred once,
  on the UI thread, when it arrives; that one texture is the stand-in in
  both the main view and the sidebar. The blur is a single [1, 2, 3, 2, 1]
  pass per direction, which away from the edges is two box passes of
  radius 1 (the experiment's); edge pixels repeat outwards. Premultiplied
  pixels average as they are. Rounded once, at the end.
- **Fade.** When a page gets a full render while it had none (the main
  view's `textures`, the sidebar's `sharp`), the render fades in over
  150 ms, drawn over the stand-in with its tint's alpha rising from 0 to 1.
  The window repaints until no fade is running.
  - Only over a stand-in: a page with no thumbnail draws its render at once.
  - A render replacing an earlier full render (a zoom) swaps at once, as
    today: there is no stand-in between them.
- No settings: the blur radius and fade length are constants.

## Code shape

- `gui-egui/image.rs`: `pub(crate) fn soften(&Rgba8Image) -> Rgba8Image`,
  written so the compiler vectorises it: horizontal pass into a `u16`
  buffer over padded rows, vertical pass over whole rows (about 30 µs for
  a 120 × 156 thumbnail, against 270 µs for a naive per-pixel version).
  And `Fades`: when each page's render arrived, `start(page, now)`,
  `alpha(page, now) -> Option<f32>` while running, `forget_finished(now)`,
  with `FADE_SECONDS = 0.15`.
- `app.rs`: `update_thumbnails` softens before uploading; `receive_renders`
  starts a fade when `textures` had no image of the page; the page loop
  draws the thumbnail then the render with the faded tint, and asks for a
  repaint while any fade runs.
- `sidebar.rs`: the same in `receive` and the thumbnail loop.

## Tests

- `soften`: a uniform image is unchanged; a single bright pixel spreads to
  its neighbours with the 1-2-3-2-1 weights; edges repeat outwards; it
  matches a slow reference (the kernel applied directly in 2D, edges
  clamped, unrounded) to the nearest level.
- `Fades`: alpha rises from 0 to 1 and ends; finished ones are forgotten.
- Harness, main view: during a fade the page is drawn with the thumbnail
  and the render; after it, with the render alone. A re-render of a page
  that already had one draws no thumbnail.
- Harness, sidebar: the same for a sharp render over the soft thumbnail.
