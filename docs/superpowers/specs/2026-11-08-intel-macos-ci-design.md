# Intel macOS in CI and snapshots

Date: 2026-11-08. Agreed with the user. A small change: Intel Macs become a
platform like the others, checked on every push and shipped in snapshots.

## Intent and why

Nothing builds or tests noido on an Intel Mac today. The first idea was an
optional job, started by hand, because Intel runners are slower. Weighed
against that cost, the simpler thing is good enough to try: an ordinary
platform, with no special handling.

## Goals

- `ci.yml` runs the full set (format, lint, tests, build) on Intel for every
  push, as it does on the other platforms.
- `snapshot.yml` builds and ships `x86_64-apple-darwin`.
- An Intel failure counts like any other: CI goes red and a snapshot is not
  published.

## Non-goals

- A manual trigger, an allowed-to-fail job, or a note in the release when
  the Intel build is missing. Treating Intel as ordinary makes them needless.
- The planned dist workflow, the Homebrew formula and the macOS app.

## Approaches

- **Ordinary matrix entries (chosen).** One entry in each workflow's list,
  and easy to remove again.
- **A separate workflow started by hand, allowed to fail in snapshots.**
  Keeps pushes quick, but repeats the setup steps and adds a state where a
  release can quietly lack a platform.
- **Cross-compile on Apple Silicon, run only the tests on Intel.** Needs two
  linked jobs, a test archive passed between them and a second way to run the
  tests, to save part of a job that nothing waits for. `just snapshot` also
  runs the binary it built to read the version.

## Design

- The runner is `macos-26-intel`, the same OS as the Apple Silicon `macos-26`.
  It is a standard runner, free and unlimited on a public repository.
- **Time.** Other public Rust projects' Intel jobs take about 2–3 times as
  long as their Apple Silicon ones, which would make noido's about 5–13
  minutes on a push with a warm cache and 12–18 in a snapshot. This is an
  estimate until the first runs. The first Intel push has no cache and takes
  longer, so judge from the second. On a push nothing waits for the job but
  the overall result; a snapshot waits for it before publishing.
- **Lifetime.** GitHub's notices give `macos-15-intel` until August or "Fall"
  2027 and said Intel support would end with it. `macos-26-intel` came later
  and has no published end date. Expect to delete both entries around then.

## Left to try

The user tries it on the branch and judges how it feels. If it is too slow,
it moves to a job started by hand.
