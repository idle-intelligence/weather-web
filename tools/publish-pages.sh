#!/usr/bin/env bash
# Build weather-wasm and publish the committed HEAD's web/ demo to an orphan
# `gh-pages` branch, following the layout used by ../tts-web and ../t0-web
# (repo root = the served tree).
#
# web/weather-client.js resolves the wasm module at ./pkg/weather_wasm.js,
# relative to itself; that is the only wasm build weather-web ships (no
# WebGPU/CPU split -- the estimator has no GPU path).
#
# web/data (a local copy of stations.json for the `?local=1` dev switch) is
# gitignored and never published; the page fetches the station list from the
# idle-intelligence/metar-stations Hugging Face dataset instead.
#
# Never checks out gh-pages in the main working tree; never pushes.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

WORKTREE_DIR="$(mktemp -d)/gh-pages-worktree"
EXPORT_DIR="$(mktemp -d)/web-export"

cleanup() {
    git worktree remove --force "$WORKTREE_DIR" >/dev/null 2>&1 || true
    rm -rf "$EXPORT_DIR"
}
trap cleanup EXIT

echo "==> Building weather-wasm"
wasm-pack build weather-wasm --target web --release

PKG_SRC="$REPO_ROOT/weather-wasm/pkg"
if [ ! -f "$PKG_SRC/weather_wasm.js" ] || [ ! -f "$PKG_SRC/weather_wasm_bg.wasm" ]; then
    echo "error: expected build output not found in $PKG_SRC" >&2
    exit 1
fi

echo "==> Exporting committed HEAD's web/ into $EXPORT_DIR"
mkdir -p "$EXPORT_DIR"
git archive HEAD web | tar -x -C "$EXPORT_DIR"

echo "==> Placing the wasm build at web/pkg"
rm -rf "$EXPORT_DIR/web/pkg"
mkdir -p "$EXPORT_DIR/web/pkg"
cp "$PKG_SRC/weather_wasm.js" "$PKG_SRC/weather_wasm_bg.wasm" "$EXPORT_DIR/web/pkg/"
[ -f "$PKG_SRC/package.json" ] && cp "$PKG_SRC/package.json" "$EXPORT_DIR/web/pkg/"

# web/data is gitignored (a local dev copy of stations.json); the published
# page fetches the station list from the Hugging Face dataset instead.
if [ -d "$EXPORT_DIR/web/data" ]; then
    echo "error: unexpected web/data in export -- refusing to publish a local station copy" >&2
    exit 1
fi

echo "==> Preparing orphan gh-pages worktree at $WORKTREE_DIR"
mkdir -p "$(dirname "$WORKTREE_DIR")"
if git show-ref --verify --quiet refs/heads/gh-pages; then
    git worktree add "$WORKTREE_DIR" gh-pages
else
    git worktree add --detach "$WORKTREE_DIR" HEAD
    git -C "$WORKTREE_DIR" checkout --orphan gh-pages
    git -C "$WORKTREE_DIR" rm -rf . >/dev/null 2>&1 || true
fi

find "$WORKTREE_DIR" -mindepth 1 -maxdepth 1 ! -name '.git' -exec rm -rf {} +
cp -R "$EXPORT_DIR/web" "$WORKTREE_DIR/web"

cd "$WORKTREE_DIR"
git add -A
if git diff --cached --quiet; then
    echo "==> No changes; gh-pages already up to date"
else
    git commit -q -m "Publish web/ demo to GitHub Pages"
fi
cd "$REPO_ROOT"

echo "==> gh-pages tip: $(git rev-parse gh-pages)"
