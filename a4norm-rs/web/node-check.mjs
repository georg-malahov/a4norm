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
