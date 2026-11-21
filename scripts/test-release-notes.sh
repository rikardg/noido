#!/bin/sh
# Exercises `just notes` against release/snapshot-notes.md.in. No network,
# no gh: it is the body and the values, not the release.
# Run from anywhere: sh scripts/test-release-notes.sh
set -eu

here=$(cd "$(dirname "$0")/.." && pwd)
cd "$here"

tmp=$(mktemp -d "${TMPDIR:-/tmp}/notes-test.XXXXXX")
trap 'rm -rf "$tmp"' EXIT INT TERM

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

short=abc12345
tag="snapshot-20260101-120000-$short"
release_url="https://github.com/rikardg/noido/releases/download/$tag"
source_url="https://github.com/rikardg/noido/tree/$short"
linux="noido-x86_64-unknown-linux-gnu-0.1.0+$short.tar.gz"
mac_arm="noido-aarch64-apple-darwin-0.1.0+$short.tar.gz"
mac_intel="noido-x86_64-apple-darwin-0.1.0+$short.tar.gz"
app_arm="noido-aarch64-apple-darwin-0.1.0+$short.app.zip"
app_intel="noido-x86_64-apple-darwin-0.1.0+$short.app.zip"

# 1. A render with the real values names every archive by its download URL.
#    The tap's updater greps the body for the two .app.zip URLs and fails
#    its run without them, so they are the contract, not the wording.
if ! SHORT=$short \
    RELEASE_URL=$release_url \
    SOURCE_URL=$source_url \
    LINUX=$linux \
    MAC_ARM=$mac_arm \
    MAC_INTEL=$mac_intel \
    APP_ARM=$app_arm \
    APP_INTEL=$app_intel \
    just notes >"$tmp/notes.md"
then
    fail "just notes exited non-zero"
fi

grep -qF "Snapshot of commit \`$short\`" "$tmp/notes.md" || fail "commit line"
grep -qF "$release_url/$linux" "$tmp/notes.md" || fail "Linux download URL"
grep -qF "$release_url/$mac_arm" "$tmp/notes.md" || fail "macOS tarball URL (arm)"
grep -qF "$release_url/$mac_intel" "$tmp/notes.md" || fail "macOS tarball URL (intel)"
grep -qF "$release_url/$app_arm" "$tmp/notes.md" || fail "app bundle URL (arm)"
grep -qF "$release_url/$app_intel" "$tmp/notes.md" || fail "app bundle URL (intel)"
grep -qF "$source_url" "$tmp/notes.md" || fail "source URL"
if grep -qF '{{' "$tmp/notes.md"; then
    fail "a placeholder reached the body"
fi

# 2. With nothing set it renders mock values: that is the local preview,
#    and it has to stay readable without a release.
just notes >"$tmp/mock.md" || fail "just notes with no values set failed"
grep -qF 'deadbeef' "$tmp/mock.md" || fail "the preview does not show the mock commit"
grep -qF 'releases/download/snapshot-' "$tmp/mock.md" || fail "the preview has no download URL"
if grep -qF '{{' "$tmp/mock.md"; then
    fail "a placeholder reached the preview"
fi

# 3. A placeholder nothing fills fails the render. `--strict` is the whole
#    reason for a template engine here: without it the release would ship a
#    body with a blank where an asset name should be.
sed 's/{{ linux }}/{{ linuks }}/' release/snapshot-notes.md.in >"$tmp/typo.md"
grep -qF '{{ linuks }}' "$tmp/typo.md" || fail "the typo template was not written"
if just notes "$tmp/typo.md" >"$tmp/typo.out" 2>"$tmp/typo.err"; then
    fail "an unknown placeholder rendered instead of failing"
fi
if [ -s "$tmp/typo.out" ]; then
    fail "a failed render still wrote a body"
fi

echo "ok: notes"
