# XDG_STATE_HOME and XDG_CONFIG_HOME place noido's files on every platform

Date: 2026-11-04. Agreed with the user.

## Why

`just run` and the repo's rule for launching noido in a check both give
it a scratch `XDG_STATE_HOME`, so the user's own state is not touched.
On macOS (and Windows) that variable is ignored: `Store::open_default`
and `SettingsFile::open_default` use `ProjectDirs`, which there means
`~/Library/Application Support/noido`. Checks on macOS wrote into the
user's real store.

## What changes

- `noido-state`: `Store::open_default` uses `$XDG_STATE_HOME/noido` when
  `XDG_STATE_HOME` is set to an absolute path, on every platform;
  otherwise as now.
- `noido-settings`: `SettingsFile::open_default` likewise with
  `$XDG_CONFIG_HOME/noido`.
- Linux: no change (that is already where `ProjectDirs` puts them, and
  a relative or empty value is ignored there too, per the XDG spec).
- The directory choice is a pure function of the variable's value and
  the `ProjectDirs` fallback, tested without touching the environment.

## Not in this change

`just run` keeps setting only `XDG_STATE_HOME`, as on Linux: settings
stay the user's own, state is scratch.
