// The free form mode in Node, and its sameness with the OCR module: a page's
// candidates from the scanner's module (formGeometry, with `empty` and the
// fields' `label`), a
// template made of them as the site makes it (a field per candidate), then
// the fill module's layout and PDFs, over the PDF and over the scan, each
// compared byte for byte with the OCR module's.
//   node a4norm-fill/web/node-check.mjs SCAN_DIST/st OCR_DIST/st FILL_DIST
import { readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
const load = async (dir, name) => {
  const m = await import(new URL(`${dir}/${name}.js`, `file://${process.cwd()}/`).href);
  await m.default({ module_or_path: readFileSync(`${dir}/${name}_bg.wasm`) });
  return m;
};
const [scanDir, ocrDir, fillDir] = process.argv.slice(2);
const [scan, ocr, fill] = [await load(scanDir, "a4norm"), await load(ocrDir, "a4norm_ocr"), await load(fillDir, "a4norm_fill")];
const forms = new URL("../../examples/forms/", import.meta.url).pathname;
const png = (pdf, page) => {
  const out = `/tmp/a4norm-fill-check-${page}`;
  execFileSync("pdftoppm", ["-r", "200", "-png", "-f", `${page}`, "-l", `${page}`, "-singlefile", pdf, out]);
  return new Uint8Array(readFileSync(out + ".png"));
};
// a signature: a synthetic stroke on nothing (sig.png)
const sig = new Uint8Array(readFileSync(new URL("sig.png", import.meta.url)));
const scanJpg = (() => {
  const d = readFileSync(forms + "demo-blank-a4norm-scan.pdf");
  const i = d.indexOf(Buffer.from([0xff, 0xd8])), j = d.lastIndexOf(Buffer.from([0xff, 0xd9]));
  return new Uint8Array(d.subarray(i, j + 2));
})();
let fail = 0;
const same = (a, b, what) => {
  const ok = a.length === b.length && a.every((v, i) => v === b[i]);
  console.log(`  ${ok ? "same" : "DIFFERENT"}  ${what}${a.length !== undefined ? ` (${a.length} bytes)` : ""}`);
  if (!ok) fail++;
};
const cases = [
  ["demo", forms + "demo-blank.pdf", 1],
  ["KG 1 p. 2", forms + "official/ba-kg1-kindergeld.pdf", 2],
];
for (const [name, pdf, page] of cases) {
  console.log(name);
  const img = png(pdf, page);
  let t = performance.now();
  const g = scan.formGeometry({ jpg: img, dpi: 200 });
  const ms = Math.round(performance.now() - t);
  const o = ocr.formGeometryImage(img, 595.28, 841.89);
  const all = [...g.lines, ...g.rects, ...g.combs, ...g.boxes];
  console.log(`  formGeometry ${ms} ms: ${g.lines.length}/${g.rects.length}/${g.combs.length}/${g.boxes.length}, empty ${all.filter((c) => c.empty).length} of ${all.length}`);
  const bare = JSON.parse(JSON.stringify(g, (k, v) => (k === "empty" || k === "label" ? undefined : v)));
  same(new TextEncoder().encode(JSON.stringify(bare)), new TextEncoder().encode(JSON.stringify(o)), "formGeometry: the scanner's (without empty, label) and the OCR module's");
  console.log(`  labels found in ${g.rects.filter((r) => r.label !== undefined).length} of ${g.rects.length} fields`);
  // the site's template: a field per candidate, keyed by page and id
  const fields = [], answers = {};
  for (const c of g.lines.concat(g.rects)) { fields.push({ key: `p${page}-${c.id}`, type: "text", place: { page, candidate: c.id } }); answers[`p${page}-${c.id}`] = `Feld ${c.id}`; }
  for (const c of g.combs) { fields.push({ key: `p${page}-${c.id}`, type: "comb", place: { page, candidate: c.id } }); answers[`p${page}-${c.id}`] = "1234567890".slice(0, c.cells); }
  for (const c of g.boxes) { fields.push({ key: `p${page}-${c.id}`, type: "choice", place: { page, candidate: c.id } }); answers[`p${page}-${c.id}`] = true; }
  const req = JSON.stringify({
    inspections: [{ ...g, page }], template: { fields }, answers,
    images: [{ page, box: [300, 700, 500, 760], rotate: 90 }],
    texts: [{ page, x: 60, y: 810, size: 11, text: "Heute: 30.09.2026 — Größe ş ł", rotate: 15 }],
  });
  same(new TextEncoder().encode(JSON.stringify(fill.fillLayout(req))), new TextEncoder().encode(JSON.stringify(ocr.fillLayout(req))), `fillLayout, ${fields.length} fields`);
  t = performance.now();
  const a = fill.fillPdf(req, new Uint8Array(readFileSync(pdf)), [], [sig]);
  const pms = Math.round(performance.now() - t);
  const b = ocr.fillPdf(req, new Uint8Array(readFileSync(pdf)), [], [sig]);
  same(a.pdf, b.pdf, `fillPdf (${pms} ms)`);
  writeFileSync(`/tmp/a4norm-fill-check-${page}.pdf`, a.pdf);
  if (page === 1) {
    same(fill.fillScan(req, [scanJpg], [sig]).pdf, ocr.fillScan(req, [scanJpg], [sig]).pdf, "fillScan over the phone scan");
  }
}
console.log(fail ? `${fail} DIFFERENT` : "all the same");
process.exit(fail ? 1 : 0);
