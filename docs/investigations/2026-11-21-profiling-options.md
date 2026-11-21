# Profiling the chain: which tools, on which platform, in what order

Date: 2026-11-21

Status: survey. Nothing is built or measured on noido yet; the tool
claims below come from current documentation, and the two that matter
most (samply's macOS support, gungraun's rename and limits) were checked
against the projects' own pages. No number in this document is a noido
measurement. The recommended first measurement is at the end.

## Question

Every part of the chain should be profiled on its own, so each can be
optimised and watched for regressions: process and window startup,
opening each format (PDF, EPUB, markdown, CBZ), typesetting, full-page
and tile rendering, thumbnails, text extraction and search, and memory.
Which tools give that on this machine (macOS 15 arm64), on the daily
driver (Linux, GNOME/Wayland), and in CI, and what is worth building
into the repo?

## Answer

A layered setup, cheapest first. Each layer is useful without the next,
and none of them needs synthetic input (which the repo forbids).

1. **One profiling build profile.** `[profile.profiling]` inheriting
   `release`, with `debug = "line-tables-only"` and no stripping. Release
   timings are the only timings that mean anything, and without debug
   info there are no inline frames and no source view.
2. **Whole-app sampling with samply.** It is the only sampler that
   covers macOS arm64 and Linux with one tool and one UI (Firefox
   Profiler). On macOS it samples both on- and off-CPU, which is what
   makes a threaded pipeline legible: "the UI thread waited 40 ms" is a
   sampling result, and it is invisible to on-CPU-only profilers.
   `perf`, dtrace and the Valgrind family are not available on macOS
   (details below), and `cargo flamegraph` there needs full Xcode.
3. **Spans, for per-phase attribution.** Two mechanisms, deliberately
   separate:
   - egui already emits `profiling` crate scopes; turning on
     `profiling/profile-with-puffin` in the binary and viewing through
     `puffin_http` + `puffin_viewer` gives egui's layout/paint spans for
     free, out of process. Do not use `puffin_egui`: the crates.io
     release pins egui 0.33, four minors behind the 0.37 this app uses.
   - `tracing` spans in `noido-render` and the engines, exported with
     `tracing-chrome`, give one lane per thread (render, thumbnails,
     text, search, UI) in a Chrome trace opened in Firefox Profiler or
     Perfetto. This is the layer that answers "which phase of typesetting
     blocked the UI", and it is the only layer that joins a UI request
     to the worker job that served it (carry a job id as a field).
4. **Per-crate benches, with instruction counts as the CI gate.**
   `gungraun` (the 2025 rename of `iai-callgrind`) runs Callgrind and
   DHAT on Linux and reports instructions and allocations, not
   milliseconds. That is the only kind of gate that survives shared CI:
   `criterion` and `divan` compare wall clock for a human but do not
   fail a build, and GitHub's `macos-15` runners are 3-core M1s.
5. **Memory with `dhat`.** The crate, not Valgrind: a global allocator
   plus a guard writes `dhat-heap.json` anywhere, including macOS. The
   repo has already measured memory this way with a counting allocator;
   `dhat` adds call sites and peak. `heaptrack`, `bytehound` and massif
   are Linux-only; the arm64 macOS Valgrind fork marks macOS 15
   experimental, which is this machine.
6. **Startup, split by the app itself.** A hidden `--trace-startup` that
   prints timestamped JSON lines and exits, measured with `hyperfine`,
   is the only honest way to get "process → settings → window → first
   frame → document open → first tile" for a GUI app that never exits.
   `sample` and Instruments' App Launch template explain a number; they
   cannot track one.
7. **GPU last, and only if asked.** `wgpu-profiler` (if the adapter has
   `TIMESTAMP_QUERY`) or a one-off Metal frame capture in Xcode.

What not to reach for: `perf` (does not exist on macOS), dtrace (SIP
blocks protected processes), Valgrind on arm64 macOS 15 (experimental
fork), `cargo flamegraph` on macOS without full Xcode, `puffin_egui`
(pinned to egui 0.33), `coz` (Rust binding last published 2020), and
criterion as a gate (it reports, it does not fail).

## What each layer answers, mapped to the chain

| Question | Tool | Where the seam is |
| --- | --- | --- |
| Where does a real session spend time, incl. waiting? | samply | the whole binary, no code |
| Which thread blocks on what? | samply (macOS off-CPU) | render/thumbnail/text threads |
| How much of a frame is egui vs. our painting? | `profiling` + puffin | `ui()` / paint path |
| Which phase of typeset/open/render took how long? | `tracing` spans | `noido-typeset::typeset`, `Document` calls, render jobs |
| Where did instructions and bytes go in a pure function? | gungraun (Linux) | `noido-text`, `noido-core`, `noido-typeset` |
| Did memory regress? | `dhat` now, gungraun-DHAT in CI | open + read-all-pages scenario |
| Is startup slower than yesterday? | `--trace-startup` + hyperfine | `main`, window creation, first frame, open |
| Is the GPU the bottleneck? | `wgpu-profiler` / Metal capture | eframe renderer |

## The seams in this repo

The chain already has clean boundaries; profiling should use them rather
than add new ones:

- **Startup**: `frontends/gui-egui/src/main.rs` (`main`, `run`,
  `run_app`), `document::open` for the first document, and eframe's
  `CreationContext` / first `ui()` pass. Note that the headless
  `Harness` in `app.rs` is `#[cfg(test)]`, so it cannot be driven from a
  bench or an example; profiling the real windowed binary is the honest
  path, and `--trace-startup` is the way to make it exit.
- **Open per format**: `document.rs` dispatches on magic bytes (PDF →
  hayro, ZIP → EPUB or comic, otherwise typeset). Timing `PdfEngine::open`
  and `page_count` per fixture gives one number per format.
- **Typeset**: `noido_typeset::typeset` / `typeset_epub` are already
  library entry points, so a bench or example can call them with bytes
  from `fixtures/` and no GUI.
- **Render**: `noido_render::Renderer::start` and its job threads; the
  per-job work is `Document::render_tile` / `render_page`. The existing
  throwaway `docs/investigations/2026-11-14-tiled-rendering/tilebench.rs`
  is the precedent for measuring this outside the app.
- **Text and search**: `noido-text` (lines, words, folding, matching)
  and the text thread's `extract`/`search` jobs; both are pure enough
  for instruction-count benches.
- **Memory**: the open-plus-read-every-page scenario from the text and
  search memory investigation, with `dhat` in place of the counting
  allocator.

## Setup

### The profile

```toml
# Cargo.toml (workspace)
[profile.profiling]
inherits = "release"
debug = "line-tables-only"
strip = "none"
```

```sh
cargo build --profile profiling
xcrun dwarfdump --uuid target/profiling/noido    # macOS: symbols exist
```

Keep `lto` and `codegen-units` identical to the shipping release
profile, or the profile is of a different program. `RUSTFLAGS="-C
force-frame-pointers=yes"` improves stack walking but does not recover
inlined frames; measure its cost before making it routine.

### Sampling

```sh
samply record ./target/profiling/noido fixtures/<doc>.pdf
samply setup                                   # once per samply update
samply record --attach "$(pgrep -x noido)" --duration 30
```

On Linux, samply needs perf events; relax `perf_event_paranoid` for the
session. Do not build the workflow on `perf` + `cargo flamegraph`: with
lld (Rust's default linker) it needs `-Wl,--no-rosegment`, and it does
not exist on the Mac. The zero-install fallback for "what is the main
thread doing" is `sample "$(pgrep -x noido)" 5 1 -file /tmp/noido.txt`
(and `spindump` for a hang).

### Spans

- `profiling` + `puffin` behind a `--profile` flag: enable scopes, start
  `puffin_http::Server`, call `GlobalProfiler::lock().new_frame()` once
  per frame, view with `puffin_viewer --url 127.0.0.1:8585`. egui's own
  scopes arrive because the `profiling` feature is unified into the
  build.
- `tracing` + `tracing-chrome` behind a `profile-spans` feature: spans
  around typeset, each `Document` call, and each render job; open the
  JSON in Firefox Profiler. Keep the dependency behind the feature so
  the shipped binary does not change.

Puffin's unit is the frame, so a worker job that straddles frames reads
badly there; use tracing/Tracy for worker timelines. If tracing is not
enough, `tracing-tracy` turns the same spans into a Tracy capture, at
the cost of a client and a GUI to match versions with.

### Benches

Add `benches/` per crate with `harness = false`, using only `fixtures/`.
`gungraun` for the pure-CPU crates, run on `ubuntu-latest`; criterion
or divan locally for the render/open paths where the work size is part
of the question. Fixture choice matters: a text page, an image-heavy
scan, and a vector page exercise the three render costs differently
(the tiled-rendering investigation already found 7–9× differences).

### Startup and memory

`--trace-startup` prints monotonic deltas at: `main` entry, settings
loaded, eframe `CreationContext` reached, end of first `ui()`, first
frame presented (egui 0.37 has `Context::set_repaint_observer`), open
complete, first tile done. Then `hyperfine --warmup 2 --runs 20` on it;
`DYLD_PRINT_STATISTICS=1` separates dyld's share on macOS. `dhat` goes
behind a feature in the same binary and in the open bench: allocator
swap plus `Profiler::new_heap()`, viewer in `dh_view.html`.

## What this machine has

Checked 2026-11-21, on macOS 15.8.2 arm64:

- Xcode 26.3 with `xctrace`, `sample` and Instruments.app (inside
  Xcode), so the Instruments route is available without installing
  anything.
- `hyperfine`.
- No `samply`, no `perf`, no `valgrind` (`cargo install samply` is the
  only install the recommended path needs), no heaptrack, no coz.

## Survey table

| Tool | macOS arm64 | Linux | Maintained | Measures |
| --- | --- | --- | --- | --- |
| samply | yes, on + off CPU | yes, on-CPU | release 2025-02, repo active | sampling, all threads, Firefox Profiler |
| `perf` + flamegraph | no | yes, needs paranoid + `--no-rosegment` | yes | on-CPU sampling, SVG |
| cargo-flamegraph (macOS) | via `xctrace`, full Xcode | — | yes | on-CPU, SVG |
| xctrace / Instruments | yes, full Xcode | no | Apple | sampling, app launch, GPU, allocations |
| `sample` / `spindump` | yes, no install | no | Apple | text call tree, all threads |
| Valgrind | no arm64 (fork marks 15 experimental) | yes | yes | callgrind, cachegrind, DHAT, massif |
| gungraun (ex-iai-callgrind) | no | yes | yes, renamed 0.17 (2025-09) | instructions, cycles, DHAT allocs |
| criterion / divan | yes | yes | yes | wall clock, statistics |
| dhat (crate) | yes | yes | stable | heap, peak, call sites |
| puffin + puffin_http + puffin_viewer | yes | yes | yes | frame-scoped spans, remote viewer |
| tracing + tracing-chrome | yes | yes | tracing-chrome last release 2024-03 | span timeline, per-thread lanes |
| tracing-tracy / Tracy | yes | yes | yes | spans, locks, plots |
| wgpu-profiler | yes, if `TIMESTAMP_QUERY` | yes | yes | GPU pass timings |
| hotpath | yes (CPU via samply) | yes (callgrind) | very active | timing + allocs; CPU mode experimental |
| coz | needs private kperf | yes | dead since 2020 | causal profiling |

Confidence notes: samply's platform support and gungraun's rename were
checked against the projects' pages. The egui/`profiling` integration
since eframe 0.30, `puffin_egui`'s egui 0.33 pin, and the 3-core
`macos-15` runner spec come from the projects' release metadata and
GitHub's runner reference respectively; they should be re-checked when
the versions move. The arm64 Valgrind status is the fork's own table,
not upstream's.

## Traps

1. Profiling a debug build. Release-inherited only.
2. `debug = 0`: samply runs, but inline frames and source view are gone.
3. Rust's shipped std has no debug info; `Vec`/`BTreeMap` frames stay
   opaque however the app is built.
4. Frame pointers do not recover inlining; only DWARF does.
5. Profiling a different `lto`/`codegen-units` setting than release.
6. samply cannot profile system binaries (`/usr/bin/*`) because their
   signature blocks `DYLD_INSERT_LIBRARIES`; noido itself is fine.
7. `samply record cargo test`: following children is version-dependent;
   `cargo test --no-run` and profile the test binary directly.
8. criterion does not fail on a regression; a gate needs gungraun or a
   threshold script.
9. Wall-clock gates on `macos-15` runners: 3 cores, 7 GB, shared.
10. puffin's frame model and worker jobs that span frames.
11. `puffin_egui` at egui 0.37: do not; use the HTTP viewer.
12. Using xdotool/ydotool to detect a window: forbidden here, and the
    reason `--trace-startup` exists.
13. Valgrind/DHAT/heaptrack/massif recipes copied from Linux onto the
    Mac.

## Next steps

In order, each cheap enough to do before committing to the next:

1. `cargo install samply`, add `[profile.profiling]`, and record one
   "open a PDF and scroll" session. That single profile decides whether
   the next week goes into the UI thread, the render thread, or open.
2. Add `--trace-startup` and take the first startup numbers; keep the
   JSON as the seed of a CI metric.
3. Only then choose between puffin and tracing first: if the sample is
   dominated by egui/wgpu, puffin is the cheap win; if it is dominated
   by hayro/typeset on a worker thread, tracing is.
4. After one measured hotspot, add the gungraun bench for the crate it
   lives in, so the fix is protected before the next fix.

## Sources

- samply: <https://github.com/mstange/samply> (platforms, off-CPU,
  `samply setup`, profile recipe).
- cargo-flamegraph: <https://github.com/flamegraph-rs/flamegraph>
  (macOS `xctrace` backend, Linux `--no-rosegment`).
- gungraun migration and release notes:
  <https://gungraun.github.io/gungraun/latest/html/migration/iai-callgrind-to-gungraun.html>,
  <https://github.com/gungraun/gungraun>.
- criterion command line:
  <https://bheisler.github.io/criterion.rs/book/user_guide/command_line_options.html>.
- Rust Performance Book, profiling:
  <https://nnethercote.github.io/perf-book/profiling.html> (stale on
  `cargo flamegraph`/macOS).
- Cargo profiles:
  <https://doc.rust-lang.org/cargo/reference/profiles.html>.
- egui/eframe changelogs and the puffin example:
  <https://github.com/emilk/egui/blob/master/crates/eframe/CHANGELOG.md>,
  <https://github.com/emilk/egui/blob/master/examples/puffin_profiler/src/main.rs>.
- puffin: <https://github.com/EmbarkStudios/puffin>.
- dhat: <https://docs.rs/dhat>.
- valgrind-macos status table:
  <https://github.com/LouisBrunner/valgrind-macos>.
- GitHub-hosted runners:
  <https://docs.github.com/en/actions/reference/runners/github-hosted-runners>.
