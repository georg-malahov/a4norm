// The Rust module in Node, stage by stage, on PREFIX-in.png from bench/dump.py.
// Writes PREFIX-rs-wasm.png. Best of N.
//   node bench/rsbench.mjs target/wasm32-unknown-unknown/release/a4norm_rs.wasm PREFIX [N]
import { readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
const [wasmPath, pre, repsS] = process.argv.slice(2);
const reps = +(repsS || 3);
const [w, h] = execFileSync("magick", ["identify", "-format", "%w %h", pre + "-in.png"]).toString().split(" ").map(Number);
const raw = execFileSync("magick", [pre + "-in.png", "-depth", "8", "RGB:-"], { maxBuffer: 1 << 30 });
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const X = instance.exports;
const n = w * h * 3;
const ptr = X.alloc(n);
const best = { load: Infinity, flat: Infinity, neutral: Infinity, tone: Infinity, store: Infinity };
let share = 0;
for (let r = 0; r < reps; r++) {
  new Uint8Array(X.memory.buffer, ptr, n).set(raw);
  let t = performance.now(), t2;
  X.load(ptr, w, h); t2 = performance.now(); best.load = Math.min(best.load, t2 - t); t = t2;
  X.flat(); t2 = performance.now(); best.flat = Math.min(best.flat, t2 - t); t = t2;
  share = X.neutral(); t2 = performance.now(); best.neutral = Math.min(best.neutral, t2 - t); t = t2;
  X.tone(); t2 = performance.now(); best.tone = Math.min(best.tone, t2 - t); t = t2;
  X.store(ptr); t2 = performance.now(); best.store = Math.min(best.store, t2 - t);
}
const out = Buffer.from(new Uint8Array(X.memory.buffer, ptr, n));
execFileSync("magick", ["-size", `${w}x${h}`, "-depth", "8", "RGB:-", `PNG24:${pre}-rs-wasm.png`], { input: out });
const s = (k) => `${k} ${(best[k] / 1000).toFixed(3)}s`;
console.log(`${pre} ${w}x${h}  ${["flat", "neutral", "tone"].map(s).join("  ")}  sum ${((best.flat + best.neutral + best.tone) / 1000).toFixed(3)}s  (${s("load")} ${s("store")})  share ${share.toFixed(3)}%  mem ${(X.memory.buffer.byteLength / 2 ** 20).toFixed(0)} MB`);
