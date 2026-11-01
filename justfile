# The commit `noido --version` carries. A release gets it from the workflow;
# anywhere else it is read from the working copy, so a build cannot keep the
# commit of the last time the build script ran (jj moves the commit without
# touching a file, and Cargo would not rerun the script).
sha := `git rev-parse --short=8 HEAD 2>/dev/null || true`
export NOIDO_GIT_SHA := env_var_or_default("NOIDO_GIT_SHA", sha)

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

# package a snapshot for one target: dist/noido-<target>-<version>.tar.gz
snapshot target="x86_64-unknown-linux-gnu":
    #!/usr/bin/env sh
    set -eu
    cargo build --release --locked --target {{target}}
    release="target/{{target}}/release"
    version="$("$release/noido" --version | awk '{print $2}')"
    case "$version" in
        # Neither the environment nor git could name the commit: a build
        # that cannot say what it is does not get published.
        *+unknown) echo "$release/noido does not know its commit" >&2; exit 1 ;;
        *+*) ;;
        *) echo "no version from $release/noido: '$version'" >&2; exit 1 ;;
    esac
    name="noido-{{target}}-$version"
    staging="dist/$name"
    mkdir -p "$staging"
    cp "$release/noido" LICENSE "$staging/"
    # Stripping a macOS binary would drop the signature the linker gave it.
    case "{{target}}" in
        *apple*) ;;
        *) strip "$staging/noido" ;;
    esac
    tar -czf "dist/$name.tar.gz" -C dist "$name"
    rm -rf "$staging"
    # Hashed from inside dist/, so the file names the archive the way it is
    # downloaded, and `sha256sum -c` works beside it.
    if command -v sha256sum >/dev/null 2>&1; then
        (cd dist && sha256sum "$name.tar.gz") > "dist/$name.tar.gz.sha256"
    else
        (cd dist && shasum -a 256 "$name.tar.gz") > "dist/$name.tar.gz.sha256"
    fi
    echo "dist/$name.tar.gz"

# update ~/.cargo/bin/noido
install:
    cargo install --locked --path crates/frontends/gui-egui
