// The one-thread module in Node, on the public examples: the report and the
// pages it returns, the time each run took, and a PDF out of pack().
//   node a4norm-rs/web/node-check.mjs a4norm-rs/web/dist/st  [--dpi 200]
import { readFileSync, writeFileSync } from "node:fs";
const [dir, ...extra] = process.argv.slice(2);
const m = await import(new URL(`${dir}/a4norm.js`, `file://${process.cwd()}/`).href);
await m.default({ module_or_path: readFileSync(`${dir}/a4norm_bg.wasm`) });
const ex = new URL("../../examples/", import.meta.url);
const runs = [["notebook-photo.jpg"], ["landing-invoice.webp"], ["specimen-card-front.jpg", "specimen-card-back.jpg"]];
for (const names of runs) {
  const files = names.map((n) => ({ name: n, bytes: readFileSync(new URL(n, ex)) }));
  const steps = [];
  const t = performance.now();
  const r = m.process(files, ["--format", "jpg", "--dpi", "200", ...extra], (s, d) => steps.push(d));
  const secs = ((performance.now() - t) / 1000).toFixed(2);
  const rising = steps.every((d, i) => i === 0 || d >= steps[i - 1]);
  console.log(`${names.join(" + ")}: ${r.pages.length} page(s), ${secs}s, ${steps.length} steps, rising ${rising}`);
  console.log(r.report.trimEnd().split("\n").map((l) => "   " + l).join("\n"));
  const pdf = m.pack(r.pages.map((p) => p.jpg), new Uint32Array(r.pages.map((p) => p.dpi)), false);
  writeFileSync(`/tmp/node-${names[0]}.pdf`, pdf);
  console.log(`   pdf ${pdf.length} bytes, starts ${new TextDecoder().decode(pdf.slice(0, 5))}`);
}

// What was found and the geometry: corners sent back give the same key, the
// looks that move nothing keep it, two quads make one spread page.
const eq = (a, b, what) => console.log(`   ${a === b ? "ok  " : "FAIL"} ${what}`);
const invoice = { name: "landing-invoice.webp", bytes: readFileSync(new URL("landing-invoice.webp", ex)) };
const args = ["--format", "jpg", "--dpi", "200", ...extra];
const auto = m.process([invoice], args);
const [ph] = auto.photos, [pg] = auto.pages;
console.log(`api: ${ph.kind}, ${ph.quads.length / 8} quad(s) in ${ph.width}x${ph.height}, page ${ph.page}; ` +
  `geom ${pg.geom.look}/${pg.geom.flat}, lines ${pg.geom.lines}\n   key ${pg.geom.key}`);
const hand = m.process([{ ...invoice, quad: ph.quads, kind: ph.kind }], args);
eq(hand.pages[0].geom.key, pg.geom.key, "the corners sent back: the same key");
eq(hand.photos[0].hand, true, "the photo says its corners came by hand");
for (const look of ["color", "original"]) {
  const r = m.process([{ ...invoice, look }], args);
  eq(r.pages[0].geom.key, pg.geom.key, `look ${look}: the same key`);
}
const q = ph.quads, mid = (i, j) => [(q[i] + q[j]) / 2, (q[i + 1] + q[j + 1]) / 2];
const [mt, mb] = [mid(0, 2), mid(6, 4)];
const spread = Float64Array.from([q[0], q[1], ...mt, ...mb, q[6], q[7], ...mt, q[2], q[3], q[4], q[5], ...mb]);
const two = m.process([{ ...invoice, quad: spread }], args);
eq(two.pages.length + two.photos[0].kind, "1spread", "16 numbers: one spread page");

// A page that looks like a blank form is offered for filling in (D24): the
// demo form after the scanner is, the invoice is not.
const offer = (f) => f.empty >= 6 && f.empty >= 0.6 * f.total;
const form = m.process([{ name: "demo-blank-scan.jpg", bytes: readFileSync(new URL("forms/demo-blank-scan.jpg", ex)) }], args);
for (const [what, page, want] of [["the demo form", form.pages[0], true], ["the invoice", pg, false]]) {
  const t = performance.now();
  const f = m.looksLikeForm(page);
  const ms = (performance.now() - t).toFixed(0);
  eq(offer(f), want, `looksLikeForm, ${what}: ${JSON.stringify(f)}, ${ms} ms`);
}

// A document's size and where it lies: each page says it, and `place` puts
// it where the person wants it.
const sized = m.process([{ name: "landing-invoice.webp", bytes: readFileSync(new URL("landing-invoice.webp", ex)), place: { w: 100, x: 20, y: 30 } }], args);
const sp = sized.pages[0];
eq(Math.round(sp.placed.w) + "," + Math.round(sp.placed.x) + "," + Math.round(sp.placed.y), "100,20,30", `place {w: 100, x: 20, y: 30}: placed ${JSON.stringify(sp.placed)}`);
eq(["id1", "id3", "a5", "a6"].every((k) => pg.size.candidates.some((c) => c.kind === k)) && pg.size.by === "aspect" && !pg.size.applied, true, `size of the invoice: ${pg.size.by}, ${pg.size.candidates.length} candidates`);
eq(sized.pages[0].geom.key !== pg.geom.key, true, "place moves the key");

// The brush: under a mask the shadow goes white and the print stays;
// nothing outside the mask changes.
{
  const page = pg; // the invoice
  const sizeOf = (jpg) => { let i = 2; while (i < jpg.length) { const m = jpg[i + 1], len = (jpg[i + 2] << 8) | jpg[i + 3]; if (m >= 0xc0 && m <= 0xc3) return [(jpg[i + 7] << 8) | jpg[i + 8], (jpg[i + 5] << 8) | jpg[i + 6]]; i += 2 + len; } };
  const [w, h] = sizeOf(page.jpg);
  const mask = new Uint8Array(w * h);
  for (let y = Math.floor(h * 0.3); y < Math.floor(h * 0.5); y++) mask.fill(1, y * w, (y + 1) * w);
  const t = performance.now();
  const out = m.cleanArea(page, mask);
  const ms = Math.round(performance.now() - t);
  eq(sizeOf(out).join("x"), `${w}x${h}`, `cleanArea over a fifth of the invoice: a JPEG of the page, ${out.length} bytes, ${ms} ms`);
  let bad = null;
  try { m.cleanArea(page, new Uint8Array(10)); } catch (e) { bad = String(e); }
  eq(/mask is 10 bytes/.test(bad), true, "a mask of the wrong size is refused");
}
