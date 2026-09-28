#!/bin/sh
# The OCR module for the browser, apart from a4norm.wasm so that scanning
# never downloads it: the same two builds as a4norm-rs/web/build.sh.
#   st/  one thread
#   mt/  lines read on rayon over Web Workers (SharedArrayBuffer: COOP +
#        COEP); initThreadPool / releaseThreadPool as in a4norm
# Then wasm-opt -Oz. The models are not in it: models.sh fetches them.
#
#   a4norm-ocr/web/build.sh [OUT]     (default a4norm-rs/web/dist/ocr)
#
# Needs wasm-bindgen-cli at the version in Cargo.lock, wasm-opt (binaryen),
# and a nightly toolchain with rust-src for the threaded build.
set -e
cd "$(dirname "$0")/.."
OUT=${1:-../a4norm-rs/web/dist/ocr}
BG=wasm32-unknown-unknown/release/a4norm_ocr.wasm
# "s" for size; tract-linalg's kernels stay at 3 (Cargo.toml)
export CARGO_PROFILE_RELEASE_OPT_LEVEL=${OPT:-s}
OPT_FLAGS="-Oz --enable-simd --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals"

RUSTFLAGS="-C target-feature=+simd128" \
  cargo build --release --lib --target wasm32-unknown-unknown --target-dir target/wasm-st
wasm-bindgen --target web --out-dir "$OUT/st" --out-name a4norm_ocr "target/wasm-st/$BG"
wasm-opt $OPT_FLAGS --strip-debug -o "$OUT/st/a4norm_ocr_bg.wasm" "$OUT/st/a4norm_ocr_bg.wasm"

# a shared memory with a maximum, imported, TLS exported: see a4norm-rs/web/build.sh
LINK="-C link-arg=--shared-memory -C link-arg=--import-memory -C link-arg=--max-memory=1073741824"
LINK="$LINK -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size"
LINK="$LINK -C link-arg=--export=__tls_align -C link-arg=--export=__tls_base"
RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals,+simd128 $LINK" \
  rustup run nightly cargo build --release --lib --target wasm32-unknown-unknown \
    --features wasm-threads --target-dir target/wasm-mt -Z build-std=panic_abort,std
wasm-bindgen --target web --out-dir "$OUT/mt" --out-name a4norm_ocr "target/wasm-mt/$BG"
wasm-opt $OPT_FLAGS --enable-threads --strip-debug -o "$OUT/mt/a4norm_ocr_bg.wasm" "$OUT/mt/a4norm_ocr_bg.wasm"

# the pool's workers (src/pool.js) load the module three levels up
grep -q "import('../../../a4norm_ocr.js')" "$OUT"/mt/snippets/*/src/pool.js

rm -f "$OUT"/*/*.d.ts
for d in "$OUT"/st "$OUT"/mt; do echo '{"type":"module"}' > "$d/package.json"; done
find "$OUT" -type f | sort | while read -r f; do
  printf '%9s  %s\n' "$(wc -c < "$f" | tr -d ' ')" "$f"
done
