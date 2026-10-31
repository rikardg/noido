# list recipes
default:
    @just --list

# nextest if installed, else cargo test
test:
    #!/usr/bin/env sh
    if cargo nextest --version >/dev/null 2>&1; then
        cargo nextest run --workspace --locked
    else
        cargo test --workspace --locked
    fi

# clippy
lint:
    cargo clippy --locked --workspace --all-targets -- -D warnings

# format in place: the fix for `fmt-check`
fmt:
    cargo fmt --all

# check formatting without rewriting any file
fmt-check:
    cargo fmt --all -- --check

# fmt, lint, test: what has to pass before a commit
check: fmt-check lint test

# release build
build:
    cargo build --release --locked

# run the release build with a scratch state dir
run file: build
    XDG_STATE_HOME="$(mktemp -d)" target/release/noido "{{file}}"

# update ~/.cargo/bin/noido
install:
    cargo install --locked --path crates/frontends/gui-egui
