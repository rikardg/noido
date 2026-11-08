# CI saves its build cache from main only

Date: 2026-11-08. Agreed with the user. A small change, following on from
[Intel macOS in CI](2026-11-08-intel-macos-ci-design.md).

## Intent and why

The Actions cache was over its 10 GB limit on 2026-11-08: 11.2 GB in 9 entries,
of which only 2.5 GB was the current cache on `main`. Another 4.8 GB was older
caches on `main` and 3.9 GB was copies saved from two `push-*` branches. Each
new branch can save a set of its own, pushing out entries that are still
useful, and Intel is a third platform in every set.

## Goals

- `ci.yml` saves its cache only for a run on `main`. Every other run still
  restores.
- A failed run saves nothing. An incomplete cache, say from a cold run that
  failed before the release build, would otherwise stay until the key changes.

## Non-goals

- Staying under the limit as such. A three-platform set is about 3.5 GB, so
  `main` alone can go over when its dependencies change often. Nothing breaks:
  older entries are evicted first.
- `snapshot.yml`. It has its own cache key and rarely runs, and its entries
  are not what fills the cache today.
- Skipping lint on Intel. It would save little space, and Intel would stop
  being like the other platforms.

## Approaches

- **Save from `main` only (chosen).** Removes the branch copies, for one input
  on one step.
- **Leave it to eviction.** Nothing to change, and old entries go first, but
  branch copies keep pushing out entries that are still useful.

## Design

`cache-save-if: ${{ github.ref == 'refs/heads/main' }}` on
`setup-rust-toolchain`, which passes it to rust-cache as `save-if`, and
`cache-on-failure: false` beside it.

## What it costs

A branch's fix-and-push loop stays fast. The cache holds dependencies, not the
workspace's own crates, which rebuild on every push either way, and a branch
restores from `main`.

- A branch that changes a crate's manifest or the lockfile restores the newest
  cache on `main` and rebuilds what changed on each push until it merges.
- A branch that bumps the toolchain matches nothing on `main`, so each of its
  pushes is fully cold.
- `main` has no Intel cache until its first run after merging, which is cold.
  This branch keeps the Intel cache it has already saved until that expires.
- GitHub removes a cache that goes 7 days unused. After a quiet week, branches
  find nothing to restore and can't save, so every push is cold until something
  runs on `main`. Run CI on `main` by hand to refill it.
- A failed cold run on `main` leaves the next run cold too.
