# Sidebar glide: the sidebar's thumb eases to follow the current page

Date: 2026-11-03. Agreed with the user.

## Intent and why

When the current page changes and its thumbnail is out of view, the
sidebar scrolls the least it can to show it (`Column::reveal`), all at
once. With a wide sidebar a thumbnail is a few hundred points tall, so
while paging through the document the sidebar's scroll bar thumb moves in
big jumps. The thumb should catch up smoothly instead, in about a quarter
of a second. The thumbnails themselves still jump: a first version eased
the whole column, and that is not what was wanted.

## What changes

Only the thumb of a solid bar (macOS with "Show scroll bars: Always",
Windows), which noido paints itself. Overlay bars are egui's own and keep
jumping; they mostly hide while the sidebar isn't used anyway.

1. **Easing, in core.** An `Eased` value is what is drawn: when the value
   moved by following, it catches up on a critically damped spring, from
   the frame it moved, keeping its speed when the value moves on again and
   never overshooting; when it moved any other way, it is drawn there at
   once. (A first version restarted a fixed 0.15 s ease-out on each move:
   it stood still on the first frame, then lurched, and pulsed with a held
   key.)
2. **The thumb.** The sidebar's thumb is placed for its eased offset. It
   eases when the sidebar followed the current page that frame, and is
   drawn where the thumbnails are when the reader scrolls (wheel, drag,
   bar): a glide stops at once. A new follow mid-glide goes on from where
   the thumb is drawn at the speed it has, so a held PageDown runs evenly.
   With the pointer over the groove the thumb is drawn in place: egui takes
   presses on its own, invisible thumb, which is there.
3. **Two paces.** After a key press it catches up quickly, in a quarter
   second. While the pages scroll on by wheel, flick or drag (egui moved
   them itself within the last quarter second), the page changes come
   one after another, and a quick thumb stops between them and makes a
   last small jump after the flick has all but stopped; then it follows
   gently, slow enough to still be moving when the next page comes, there
   about 0.6 s after the last.
4. **Still jumps:** when the sidebar opens, when it comes back from the
   list of matches, when it is resized, and when the OS asks for reduced
   motion.
5. **Reduced motion.** `Theme` keeps native-theme's
   `accessibility.reduce_motion` (NSWorkspace on macOS; the portal, else
   `enable-animations`, on GNOME; `kdeglobals` on KDE; Windows' setting),
   read whenever the theme is read: at start, and on the UI thread when it
   changes. The existing fades don't honour it yet; out of scope.

For a quarter second the thumb lags the thumbnails, which are already
there.

## Tests

Core: `Eased` draws at once when not following; when following it moves
from the first frame, never back or past, and is there within a third of a
second; it snaps when moved otherwise; following on keeps its speed; a held
key moves it evenly; gently it takes longer, gets there, and keeps moving
through a flick. Harness, with solid bars: on a PageDown that makes the
sidebar follow, the thumbnails are there at once while the thumb is on its
way from that frame, there within a third of a second, asking for frames
until then; with reduced motion it jumps; a wheel scroll mid-glide puts it
with the thumbnails; the pointer over the groove puts it in place; back
from the matches it is in place at once; following a wheel scroll of the
pages it is still on its way when the quick pace would have stopped, and
there within a second.

## By hand

On a Mac with "Show scroll bars: Always" and a wide sidebar, PageDown
slowly and held: the sidebar's thumb eases, the thumbnails jump. Flick
through the pages: the thumb glides on without stopping between pages. With
Reduce motion on (Accessibility → Display) the thumb jumps too.
