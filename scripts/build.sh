#!/usr/bin/env bash
# Builds weather-wasm and assembles the deployed site into _site/.
#
# Rewrites the ?v= build tag to ENGINE_BUILD on every loading URL
# (web/weather-client.js) and checks the built wasm carries no local build
# path.
#
# Requires wasm-pack and wasm-bindgen-cli (version matching Cargo.lock's
# wasm-bindgen exactly: `cargo install wasm-bindgen-cli --version <ver>
# --locked`).
#
# Usage: ENGINE_BUILD=<tag> scripts/build.sh
# ENGINE_BUILD defaults to "dev" for local builds; CI passes the commit sha.
set -euo pipefail

ENGINE_BUILD="${ENGINE_BUILD:-dev}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "==> Building weather-wasm for ENGINE_BUILD=$ENGINE_BUILD"
RUSTFLAGS="--remap-path-prefix=$HOME=/home" \
  wasm-pack build weather-wasm --target web --release

WASM="weather-wasm/pkg/weather_wasm_bg.wasm"

# --- Local-path / user-name leak check on built wasm ---
LEAKS=$(strings "$WASM" | grep -F -e "$HOME" -e "Code/" -e ".claude/" -e "/Users/" || true)
USER_HITS=$(strings "$WASM" | grep -Fw -e "$(id -un)" || true)
if [ -n "$LEAKS$USER_HITS" ]; then
    echo "error: $WASM contains local paths or the user name:" >&2
    printf '%s\n%s\n' "$LEAKS" "$USER_HITS" | grep -v '^$' | head -20 >&2
    exit 1
fi
echo "==> no local paths in built wasm"

# --- Assemble the deployed site into _site/ ---
echo "==> Assembling _site"
rm -rf _site
mkdir -p _site
cp -R web _site/web
rm -rf _site/web/pkg
mkdir -p _site/web/pkg
cp weather-wasm/pkg/weather_wasm.js weather-wasm/pkg/weather_wasm_bg.wasm _site/web/pkg/
[ -f weather-wasm/pkg/package.json ] && cp weather-wasm/pkg/package.json _site/web/pkg/

# --- Rewrite the ?v= build tag to ENGINE_BUILD on every loading URL ---
echo "==> Rewriting ?v= build tag to $ENGINE_BUILD"
grep -rl '?v=' _site/web | xargs sed -i.bak -E "s/\?v=[0-9a-zA-Z]+/?v=$ENGINE_BUILD/g"
find _site/web -name '*.bak' -delete

COUNT="$(grep -rEo '\?v=[0-9a-zA-Z]+' _site/web | wc -l | tr -d ' ')"
if [ "$COUNT" -eq 0 ]; then
    echo "error: no ?v= build-tag URLs found in the assembled site" >&2
    exit 1
fi

BAD="$(grep -rEo '\?v=[0-9a-zA-Z]+' _site/web | grep -v ":?v=$ENGINE_BUILD\$" || true)"
if [ -n "$BAD" ]; then
    echo "error: found ?v= URLs not rewritten to $ENGINE_BUILD:" >&2
    echo "$BAD" >&2
    exit 1
fi

echo "==> Wrote _site"
