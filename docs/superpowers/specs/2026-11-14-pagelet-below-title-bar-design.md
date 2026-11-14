# The pagelet stays below the title bar

Date: 2026-11-14. Agreed with the user. A small change: the pagelet can
no longer be dragged or resized over the title bar.

## Intent and why

The pagelet opens below the title bar, but a drag (egui's
`constrain(true)`, the whole window) and a resize (the whole window
less the padding) let it cover the title bar. A panel over the window's
own controls feels wrong.

## Goals

- The pagelet's area is the window below the title bar, the sidebar
  included, less a small gap (`PAGELET_PADDING`) on every side.
- A drag stops at that area's edges; so does a resize.
- Opening and reopening place it in the same area, so a pagelet saved
  over the title bar comes back below it. The first pagelet sits a gap
  below the document view's top instead of flush against it; it stays
  flush against the sidebar.

## Non-goals

- Keeping it off the sidebar: it may sit there, as now.

## Design

One method gives the area: from the window's left edge and the
document view's top (below the title bar) to the window's bottom-right,
shrunk by `PAGELET_PADDING`. The pagelet window gets
`constrain_to(area)`, the resize handles clamp to it, and
`open_pagelet` is given it wherever it is called.

## Testing

Harness: dragging the pagelet far up leaves its top a gap below the
document view's top; dragging its top handle far up does the same; a
first pagelet sits a gap below the view's top, flush with its left.
