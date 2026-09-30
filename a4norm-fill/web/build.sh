#!/bin/sh
# The fill module for the browser: the free form mode fills a form in and
# writes its PDF with it and the scanner's module (formGeometry), without
# the OCR module. One build: filling runs on one thread anywhere.
#   a4norm-fill/web/build.sh [OUT]     (default a4norm-rs/web/dist/fill)
# Needs wasm-bindgen-cli at the version in Cargo.lock and wasm-opt (binaryen).
set -e
cd "$(dirname "$0")/.."
OUT=${1:-../a4norm-rs/web/dist/fill}
cargo build --release --lib --target wasm32-unknown-unknown --features web --target-dir target/wasm
wasm-bindgen --target web --out-dir "$OUT" --out-name a4norm_fill target/wasm/wasm32-unknown-unknown/release/a4norm_fill.wasm
wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals \
  --strip-debug -o "$OUT/a4norm_fill_bg.wasm" "$OUT/a4norm_fill_bg.wasm"
rm -f "$OUT"/*.d.ts
# Node reads a bare .js as CommonJS; the module is ES
echo '{"type":"module"}' > "$OUT/package.json"
find "$OUT" -type f | sort | while read -r f; do
  printf '%9s  %s\n' "$(wc -c < "$f" | tr -d ' ')" "$f"
done
