# `-y` / `--yes` for `noido --install`

## Intent

`noido --install` and `--uninstall` print the steps they will take and then
ask `Proceed? [y/N]` on stdin. That is right when a person runs it, and in
the way when a script or a setup step runs it. A `-y` / `--yes` flag says
"carry out the steps" without the question.

## Goals

- `noido --install -y` and `noido --uninstall -y` do the work with no
  input, on Linux and on macOS.
- The steps are still printed, so the output says what happened.
- `-y` on its own is a usage error, not a silently ignored flag.

## Non-goals

- Changing the prompt, its default, or what counts as a yes.
- A `--no` or `--dry-run` counterpart.
- `--app-bundle DIR`, which never asks.

## Design

- `Args` gains `yes: bool`, `-y` / `--yes`, "Don't ask for
  confirmation", `requires` one of `install` / `uninstall`.
- `desktop::install(yes)` and `desktop::uninstall(yes)` pass it to
  `proceed(plan, yes)`; `app_bundle::install(yes)` / `uninstall(yes)` do
  the same on macOS. `proceed` skips `ask_to_proceed` when `yes` is set
  and carries the steps out.
- `ask_to_proceed` and `confirmed` keep their signatures and behaviour;
  the printing `ask_to_proceed` did now lives in `print_steps`, which
  `proceed` prints when `yes` is set.

## Tests

- clap: `-y` and `--yes` parse with `--install` and with `--uninstall`;
  `noido -y` alone and `noido -y a.pdf` are errors.
- `proceed` with `yes` carries out the steps without reading stdin (a
  plan whose steps write into a temp directory).
