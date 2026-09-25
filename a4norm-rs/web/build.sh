#!/bin/sh
# The browser's two modules, from the one crate:
#   st/  one thread, runs anywhere (a page that is not cross-origin isolated,
#        the Telegram web client's iframe, an old browser)
#   mt/  rayon over Web Workers; needs SharedArrayBuffer, so a page served
#        with COOP same-origin + COEP require-corp
# Same output from both, byte for byte: the work is split by rows and
# channels, and nothing depends on which thread did what.
#
#   a4norm-rs/web/build.sh [OUT]          (default a4norm-rs/web/dist)
#
# Needs wasm-bindgen-cli at the version in Cargo.lock, and a nightly
# toolchain with rust-src: atomics need a std built with them.
set -e
cd "$(dirname "$0")/.."
OUT=${1:-web/dist}
BG=wasm32-unknown-unknown/release/a4norm_rs.wasm

RUSTFLAGS="-C target-feature=+simd128" \
  cargo build --release --lib --target wasm32-unknown-unknown --target-dir target/wasm-st
wasm-bindgen --target web --out-dir "$OUT/st" --out-name a4norm "target/wasm-st/$BG"

# The shared memory needs a maximum; 1 GiB is four times the peak of a
# 50 MP photo and reserves nothing up front.
RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals,+simd128 -C link-arg=--max-memory=1073741824" \
  rustup run nightly cargo build --release --lib --target wasm32-unknown-unknown \
    --features wasm-threads --target-dir target/wasm-mt -Z build-std=panic_abort,std
wasm-bindgen --target web --out-dir "$OUT/mt" --out-name a4norm "target/wasm-mt/$BG"

rm -f "$OUT"/*/*.d.ts
# Node reads a bare .js as CommonJS; the modules are ES, which a browser
# knows from the import. This is only for checking them in Node.
for d in "$OUT"/st "$OUT"/mt; do echo '{"type":"module"}' > "$d/package.json"; done
find "$OUT" -type f | sort | while read -r f; do
  printf '%8s  %s\n' "$(wc -c < "$f" | tr -d ' ')" "$f"
done
