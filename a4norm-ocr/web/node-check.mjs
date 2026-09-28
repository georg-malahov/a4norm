// The one-thread OCR module in Node on the demo form: the time of a page,
// the first time (plans compiled) and again, and what it read.
//   node a4norm-ocr/web/node-check.mjs [DIST/st] [MODELS]
import { readFileSync } from "node:fs";
const here = new URL(".", import.meta.url);
const [dir = new URL("../../a4norm-rs/web/dist/ocr/st", here).pathname, models = new URL("../models", here).pathname] =
  process.argv.slice(2);
const m = await import(new URL(`${dir}/a4norm_ocr.js`, `file://${process.cwd()}/`).href);
await m.default({ module_or_path: readFileSync(`${dir}/a4norm_ocr_bg.wasm`) });
let t = performance.now();
const ocr = new m.Ocr(readFileSync(`${models}/det.onnx`), readFileSync(`${models}/rec.onnx`), readFileSync(`${models}/rec.yml`, "utf8"));
console.log(`models ${Math.round(performance.now() - t)} ms`);
const forms = new URL("../../examples/forms/", here);
for (const name of ["demo-filled-scan.jpg", "demo-blank-a4norm-scan.pdf"]) {
  const bytes = readFileSync(new URL(name, forms));
  const times = [];
  let r;
  for (let i = 0; i < 2; i++) {
    t = performance.now();
    r = ocr.inspectImage(bytes, 595.28, 841.89);
    times.push(((performance.now() - t) / 1000).toFixed(2));
  }
  const mb = Math.round(process.memoryUsage().rss / 1048576);
  console.log(`${name}: ${times.join(" s, then ")} s; ${r.words.length} words, skew ${r.skewDeg.toFixed(2)}°, ` +
    `print ${r.printedSize.toFixed(1)} pt, langs ${r.langs}; rss ${mb} MB`);
  console.log("   " + r.words.slice(0, 12).map((w) => w.text).join(" "));
}
