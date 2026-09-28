// The OCR module in a browser, as malahov.io would run it: in a worker on a
// cross-origin-isolated page, the threaded build on a pool. Prints each
// page's time (the first run compiles the plans), what it read, and the
// time of init and release.
//   node a4norm-ocr/web/check.mjs [chromium|webkit] [st|mt] [THREADS] [DIST] [MODELS]
// Playwright comes from a4norm-rs/web/check (npm install there), or from the
// folder in PLAYWRIGHT.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, join } from "node:path";
const here = new URL(".", import.meta.url).pathname;
const [name = "chromium", build = "mt", threads = "0", dist = join(here, "../../a4norm-rs/web/dist/ocr"), models = join(here, "../models")] =
  process.argv.slice(2);
const { chromium, webkit } = createRequire(process.env.PLAYWRIGHT || join(here, "../../a4norm-rs/web/check/"))("playwright");

const page = `<!doctype html><meta charset="utf-8"><script type="module">
const src = \`self.onmessage = async ({ data: { build, threads, files } }) => { try {
  const o = self.location.origin, ms = {}, out = [];
  const time = async (k, f) => { const t = performance.now(); const r = await f(); ms[k] = Math.round(performance.now() - t); return r; };
  const m = await import(o + '/dist/' + build + '/a4norm_ocr.js');
  await m.default({ module_or_path: o + '/dist/' + build + '/a4norm_ocr_bg.wasm' });
  if (build === 'mt') await time('init', () => m.initThreadPool(threads));
  const get = async (p) => new Uint8Array(await (await fetch(o + p)).arrayBuffer());
  const [det, rec, yml] = await Promise.all([get('/models/det.onnx'), get('/models/rec.onnx'), fetch(o + '/models/rec.yml').then((r) => r.text())]);
  const ocr = await time('models', () => new m.Ocr(det, rec, yml));
  for (const f of files) {
    const bytes = await get('/examples/forms/' + f);
    const r1 = await time(f + ' first', () => ocr.inspectImage(bytes, 595.28, 841.89));
    const r = await time(f, () => ocr.inspectImage(bytes, 595.28, 841.89));
    out.push({ f, words: r.words.length, same: JSON.stringify(r) === JSON.stringify(r1), langs: r.langs, skew: r.skewDeg, print: r.printedSize, text: r.words.slice(0, 10).map((w) => w.text).join(' ') });
  }
  if (build === 'mt') await time('release', () => m.releaseThreadPool());
  self.postMessage({ ms, out });
} catch (e) { self.postMessage({ error: String(e && e.stack || e) }); } };\`;
const w = new Worker(URL.createObjectURL(new Blob([src], { type: 'text/javascript' })), { type: 'module' });
w.onmessage = (e) => { window.done = e.data; };
w.onerror = (e) => { window.done = { error: e.message || String(e) }; };
const q = new URLSearchParams(location.search);
w.postMessage({ build: q.get('build'), threads: +q.get('threads') || navigator.hardwareConcurrency, files: ['demo-filled-scan.jpg', 'demo-blank-a4norm-scan.pdf'] });
</script>`;

const roots = { "/dist/": dist, "/models/": models, "/examples/": join(here, "../../examples/") };
const types = { ".js": "text/javascript", ".wasm": "application/wasm", ".html": "text/html", ".json": "application/json" };
const server = createServer(async (req, res) => {
  const url = decodeURIComponent(req.url.split("?")[0]);
  const head = { "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp", "Cross-Origin-Resource-Policy": "same-origin" };
  if (url === "/") return res.writeHead(200, { ...head, "Content-Type": "text/html" }).end(page);
  const pre = Object.keys(roots).find((p) => url.startsWith(p));
  try {
    const file = join(roots[pre], url.slice(pre.length));
    const body = await readFile(file);
    res.writeHead(200, { ...head, "Content-Type": types[extname(file)] || "application/octet-stream" }).end(body);
  } catch {
    res.writeHead(404).end();
  }
}).listen(0);
const port = server.address().port;

const browser = await ({ chromium, webkit })[name].launch();
const p = await browser.newPage();
p.on("console", (m) => console.log("  console:", m.text()));
await p.goto(`http://localhost:${port}/?build=${build}&threads=${threads}`);
await p.waitForFunction(() => window.done, null, { timeout: 600000 });
const r = await p.evaluate(() => window.done);
await browser.close();
server.close();
if (r.error) {
  console.log("ERROR", r.error);
  process.exit(1);
}
console.log(`${name} ${build}: ${JSON.stringify(r.ms)}`);
for (const o of r.out) console.log(`  ${o.f}: ${o.words} words, same twice ${o.same}, langs ${o.langs}, skew ${o.skew.toFixed(2)}°, print ${o.print.toFixed(1)} pt\n    ${o.text}`);
