# The commit `noido --version` carries. A release gets it from the workflow;
# anywhere else it is read from the working copy, so a build cannot keep the
# commit of the last time the build script ran (jj moves the commit without
# touching a file, and Cargo would not rerun the script).
sha := `git rev-parse --short=8 HEAD 2>/dev/null || true`
export NOIDO_GIT_SHA := env_var_or_default("NOIDO_GIT_SHA", sha)

# The developer tools `just tools` installs, at the versions the checks were
# written against. Bump one here; the recipe takes it from there.
nextest := "0.9.148"
minijinja := "3.1.0"

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

# render the snapshot release notes to stdout; mock values unless set
notes template="release/snapshot-notes.md.in":
    @minijinja-cli --strict --no-include \
        -D short="${SHORT:-deadbeef}" \
        -D release_url="${RELEASE_URL:-https://github.com/rikardg/noido/releases/download/snapshot-20260101-120000-deadbeef}" \
        -D source_url="${SOURCE_URL:-https://github.com/rikardg/noido/tree/deadbeef}" \
        -D linux="${LINUX:-noido-x86_64-unknown-linux-gnu-0.1.0+deadbeef.tar.gz}" \
        -D mac_arm="${MAC_ARM:-noido-aarch64-apple-darwin-0.1.0+deadbeef.tar.gz}" \
        -D mac_intel="${MAC_INTEL:-noido-x86_64-apple-darwin-0.1.0+deadbeef.tar.gz}" \
        -D app_arm="${APP_ARM:-noido-aarch64-apple-darwin-0.1.0+deadbeef.app.zip}" \
        -D app_intel="${APP_INTEL:-noido-x86_64-apple-darwin-0.1.0+deadbeef.app.zip}" \
        "{{ template }}"

# render the notes and check the body the tap reads
notes-check:
    @sh scripts/test-release-notes.sh

# fmt, lint, test: what has to pass before a commit
check: fmt-check lint test notes-check

# install the pinned developer tools
tools:
    #!/usr/bin/env sh
    set -eu
    # `just` itself is the bootstrap (`cargo binstall just`), so it is not in
    # the list. Binstall skips a crate it already installed at that version,
    # so a second run is quick and a changed pin installs.
    if ! command -v cargo-binstall >/dev/null 2>&1; then
        echo "no cargo-binstall: cargo install cargo-binstall" >&2
        exit 1
    fi
    cargo binstall --no-confirm --locked \
        cargo-nextest@{{nextest}} \
        minijinja-cli@{{minijinja}}
    # A record can outlive the binary it names, and binstall then skips the
    # crate as installed. Ask each tool to run, and print the line that
    # forces the reinstall; the pin is what keeps it from becoming latest.
    for crate in cargo-nextest@{{nextest}} minijinja-cli@{{minijinja}}; do
        name=${crate%@*}
        if ! "$name" --version >/dev/null 2>&1; then
            echo "$name does not run, and binstall's record says it is installed." >&2
            hint="Run: cargo binstall --force --no-confirm --locked $crate"
            # Yellow, and only to a terminal: a redirected or piped log
            # should not carry escape codes.
            if [ -t 2 ]; then
                printf '\033[33m%s\033[0m\n' "$hint" >&2
            else
                echo "$hint" >&2
            fi
            exit 1
        fi
    done
    # What binstall owns, so a tool from brew or `cargo install` is visible
    # beside it.
    cargo binstall --list

# release build
build:
    cargo build --release --locked

# run the release build with a scratch state dir
run file: build
    XDG_STATE_HOME="$(mktemp -d)" target/release/noido --foreground "{{file}}"

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

# package the macOS app bundle for one target:
# dist/noido-<target>-<version>.app.zip and its .sha256
app-zip target="aarch64-apple-darwin":
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
    mkdir -p dist
    "$release/noido" --app-bundle "dist/$name"
    # ditto keeps the bundle's structure; --sequesterRsrc keeps the bundle's
    # extended attributes out of its own directories (they would land in
    # `__MACOSX/`, which Homebrew's unzip deletes) — otherwise the seal
    # breaks: `codesign --verify` reports "a sealed resource is missing or
    # invalid"; --keepParent puts `noido.app` at the top of the zip, which
    # is what a cask's `app "noido.app"` expects.
    ditto -c -k --sequesterRsrc --keepParent "dist/$name/noido.app" "dist/$name.app.zip"
    # The staging directory must not stay in dist/: the matrix legs upload
    # it to the same artifact and both bundles are called noido.app.
    rm -rf "dist/$name"
    if command -v sha256sum >/dev/null 2>&1; then
        (cd dist && sha256sum "$name.app.zip") > "dist/$name.app.zip.sha256"
    else
        (cd dist && shasum -a 256 "$name.app.zip") > "dist/$name.app.zip.sha256"
    fi
    echo "dist/$name.app.zip"

# update ~/.cargo/bin/noido
install:
    cargo install --locked --path crates/frontends/gui-egui

# build noido.app into target/app (macOS)
app: build
    target/release/noido --app-bundle target/app
