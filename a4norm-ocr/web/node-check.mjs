// The one-thread OCR module in Node on the demo form: the time of a page,
// the first time (plans compiled) and again, and what it read; then the
// form filled from its template, laid out and written as a PDF.
//   node a4norm-ocr/web/node-check.mjs [DIST/st] [MODELS]
import { readFileSync, writeFileSync } from "node:fs";
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

// Filling: the demo template on the phone scan's inspection, laid out, then
// written over the scan's JPEG as a PDF.
{
  const scan = readFileSync(new URL("demo-blank-a4norm-scan.pdf", forms));
  const ins = ocr.inspectImage(scan, 595.28, 841.89);
  const request = JSON.stringify({
    inspections: [ins],
    template: JSON.parse(readFileSync(new URL("demo-template.json", forms), "utf8")),
    answers: JSON.parse(readFileSync(new URL("demo-answers.json", forms), "utf8")),
  });
  const layout = m.fillLayout(request);
  console.log(`fillLayout: base ${layout.baseSize} pt, ${layout.placed.length} placed, ` +
    `${layout.placed.filter((p) => p.overflow).length} overflowing`);
  // the scan PDF holds one JPEG: its bytes from SOI to EOI
  const soi = scan.indexOf(Buffer.from([0xff, 0xd8, 0xff]));
  const jpeg = scan.subarray(soi, scan.lastIndexOf(Buffer.from([0xff, 0xd9])) + 2);
  const r = m.fillScan(request, [jpeg]);
  writeFileSync("/tmp/a4norm-ocr-filled-scan.pdf", r.pdf);
  console.log(`fillScan: ${r.pdf.length} bytes, starts ${new TextDecoder().decode(r.pdf.slice(0, 5))}, ` +
    `fallback ${r.fallback} -> /tmp/a4norm-ocr-filled-scan.pdf`);
}
