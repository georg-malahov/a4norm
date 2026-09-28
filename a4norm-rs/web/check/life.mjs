// node life.mjs [webkit|chromium] MODE LOADS FILES: the thread pool's life
// (life.html) over LOADS page loads, each a scan of FILES (comma-separated
// URLs under the server). Prints the times of init, the scan and release,
// and, in WebKit, the CPU its web process spends in the 10 s after each load
// settles: threads a page left behind show up there, and add up by reload.
import { chromium, webkit } from "playwright";
import { execFileSync } from "node:child_process";
const [name = "webkit", mode = "release", loads = "4", files = "/examples/notebook-photo.jpg"] = process.argv.slice(2);
const web = name === "webkit" ? /ms-playwright\/webkit-.*WebContent/ : /ms-playwright\/chromium.*Renderer|Google Chrome for Testing Helper \(Renderer\)/;
const cpu = () => Object.fromEntries(execFileSync("ps", ["-A", "-o", "pid=,time=,rss=,comm="], { encoding: "utf8" }).trim().split("\n")
  .map((l) => l.trim().match(/^(\d+)\s+(\S+)\s+(\d+)\s+(.*)$/)).filter((m) => m && web.test(m[4]))
  .map((m) => [m[1], { t: m[2].split(":").map(Number).reduce((a, b) => a * 60 + b, 0), rss: Math.round(+m[3] / 1024) }]));
setTimeout(() => { console.log("TIMEOUT"); process.exit(3); }, +(process.env.LIMIT || 600) * 1000);
const before = cpu();
const browser = await ({ chromium, webkit })[name].launch();
const page = await browser.newPage();
page.on("console", (m) => console.log("  console:", m.text()));
const url = `http://localhost:${process.env.PORT || 8765}/life.html?mode=${mode}&files=${files}` + (process.env.THREADS ? `&threads=${process.env.THREADS}` : "");
// the web processes this browser started: others' runs may start their own
let mine = null;
for (let i = 0; i < +loads; i++) {
  if (i) await page.reload();
  else await page.goto(url);
  mine ??= Object.keys(cpu()).filter((k) => !before[k]);
  await page.waitForFunction(() => window.done, null, { timeout: 0 });
  const r = await page.evaluate(() => window.done);
  if (r.error) { console.log("ERROR", r.error); process.exit(1); }
  await page.waitForTimeout(3000);
  const c0 = cpu();
  await page.waitForTimeout(10000);
  const c1 = cpu();
  const busy = Object.keys(c1).filter((k) => mine.includes(k)).map((k) => `${Math.round((c1[k].t - (c0[k]?.t ?? 0)) * 100)} ms/s ${c1[k].rss} MB`).join(" | ");
  console.log(`${name} ${mode} load ${i + 1}: ${JSON.stringify(r.ms)} ${JSON.stringify(r.hashes)}  cpu ${busy}`);
}
await browser.close();
process.exit(0);
