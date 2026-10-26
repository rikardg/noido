# PageUp/PageDown step: 98%

Date: 2026-10-26 · Status: small change, asked for in conversation

PageUp/PageDown scrolls 98% of the visible height instead of 90%:
`SCREEN_OVERLAP` goes from 0.1 to 0.02. The screen edge flash marks
where the old view ended, so little overlap is needed to keep one's place.
95% and 99% were tried by hand; 98% was kept. Paged mode uses the same
step within a spread.
