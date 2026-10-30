# list recipes
default:
    @just --list

# nextest if installed, else cargo test
test:
    #!/usr/bin/env sh
    if cargo nextest --version >/dev/null 2>&1; then
        cargo nextest run --workspace
    else
        cargo test --workspace
    fi

# clippy
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# formatter
fmt:
    cargo fmt

# fmt, lint, test: what has to pass before a commit
check: fmt lint test

# release build
build:
    cargo build --release

# run the release build with a scratch state dir
run file: build
    XDG_STATE_HOME="$(mktemp -d)" target/release/noido "{{file}}"

# update ~/.cargo/bin/noido
install:
    cargo install --locked --path crates/frontends/gui-egui
