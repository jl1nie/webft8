#!/bin/bash
# Deploy ft8-web/www/ → docs/ and uvpacket-web/www/ → docs/uvpacket/
# for GitHub Pages. Both apps end up on the same Pages site:
#
#   https://<owner>.github.io/<repo>/             — WebFT8
#   https://<owner>.github.io/<repo>/uvpacket/    — uvpacket signed-QSL
#
# wasm-pack must be installed (`cargo install wasm-pack`).
set -euo pipefail
cd "$(dirname "$0")"

# Ensure pre-commit hook is always active (self-healing: safe to run repeatedly)
git config core.hooksPath .githooks 2>/dev/null || true

# ──────────────────────── WebFT8 (existing) ─────────────────────────────

SRC=ft8-web/www
DST=docs

# Extract version from Cargo.toml (single source of truth)
VERSION=$(grep '^version' ft8-desktop/src-tauri/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')

# Copy all JS and HTML (skip WASM binary — built separately)
for f in "$SRC"/*.js "$SRC"/*.html "$SRC"/*.json; do
  [ -f "$f" ] || continue
  base=$(basename "$f")
  cp "$f" "$DST/$base"
done

# Rewrite WASM import path: ../pkg/ft8_web.js → ./ft8_web.js (all JS files)
sed -i "s|from '../pkg/ft8_web.js'|from './ft8_web.js'|g" "$DST/app.js"
sed -i "s|from '../pkg/ft8_web.js'|from './ft8_web.js'|g" "$DST/decode-worker.js"

# Inject version from Cargo.toml into docs/app.js
sed -i "s|APP_VERSION = '__VERSION__'|APP_VERSION = '$VERSION'|" "$DST/app.js"

# Bump service worker cache name so Tauri WebView2 discards stale cache
if [ -f "$DST/sw.js" ]; then
  sed -i "s|CACHE_NAME = 'webft8-[^']*'|CACHE_NAME = 'webft8-v$VERSION'|" "$DST/sw.js"
fi

echo "Deployed WebFT8 to docs/ (v$VERSION)"

# ──────────────────────── uvpacket-web ──────────────────────────────────

UV_SRC=uvpacket-web/www
UV_PKG=uvpacket-web/pkg
UV_DST=docs/uvpacket

UV_VERSION=$(grep '^version' uvpacket-web/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')

mkdir -p "$UV_DST"

# Force a fresh WASM build. mfsk-core lives in a sibling crate that is
# pulled in via [patch.crates-io] path = "../mfsk-core/mfsk-core" — the
# Cargo incremental fingerprint for path-patched deps has bitten us
# (commit c83e433): a local mfsk-core source change DID NOT trigger a
# rebuild, so we shipped a wasm bundle that bound the new wasm-bindgen
# layer to the OLD mfsk-core. Manifested as the noise-rejection sync
# gate being absent from the deployed bundle even though the source
# tree had it.
#
# `cargo clean -p mfsk-core` doesn't recognise the path-patched dep
# (it's not a workspace member), reports "Removed 0 files", and is
# useless here. So we surgically remove mfsk-core's compiled artefacts
# in target/wasm32-unknown-unknown/release/deps/ instead, plus the
# wasm-pack output dir. Cargo then rebuilds mfsk-core from source and
# wasm-bindgen relinks the cdylib against it. Cost: ~3-4 s of
# mfsk-core recompile vs the corruption risk of an incremental miss.
echo "Cleaning stale mfsk-core artefacts (path-patch incremental safety)…"
rm -f target/wasm32-unknown-unknown/release/deps/*mfsk_core* \
      target/wasm32-unknown-unknown/release/deps/*mfsk_fec*
rm -rf uvpacket-web/pkg

echo "Building uvpacket-web WASM…"
wasm-pack build --target web --out-dir pkg uvpacket-web

# Copy JS/HTML and the wasm-pack output into docs/uvpacket/.
for f in "$UV_SRC"/*.js "$UV_SRC"/*.html "$UV_SRC"/*.json; do
  [ -f "$f" ] || continue
  base=$(basename "$f")
  cp "$f" "$UV_DST/$base"
done
cp "$UV_PKG"/uvpacket_web.js "$UV_DST"/
cp "$UV_PKG"/uvpacket_web_bg.wasm "$UV_DST"/

# Inject version into app.js.
sed -i "s|APP_VERSION = '__VERSION__'|APP_VERSION = '$UV_VERSION'|" "$UV_DST/app.js"

# Sanity: hash the produced wasm and surface it in the deploy log so
# stale-bundle issues are noticed at deploy time rather than in the
# field. If the hash hasn't changed across a substantive code edit,
# the cargo-clean above didn't take effect for some reason and we
# should investigate before pushing.
WASM_HASH=$(md5sum "$UV_DST/uvpacket_web_bg.wasm" | cut -d' ' -f1)

# Update ft8-web's service worker cache name with BOTH versions so an
# uvpacket-only release also forces SW reinstall, defeating the cache-
# served-stale-wasm scenario (sw.js's scope covers /uvpacket/).
if [ -f "$DST/sw.js" ]; then
  sed -i "s|CACHE_NAME = 'webft8-[^']*'|CACHE_NAME = 'webft8-v$VERSION-uv$UV_VERSION-${WASM_HASH:0:8}'|" "$DST/sw.js"
fi

echo "Deployed uvpacket-web to docs/uvpacket/ (v$UV_VERSION, wasm md5=${WASM_HASH:0:12}…)"
