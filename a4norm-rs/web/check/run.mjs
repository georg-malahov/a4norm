// node run.mjs [chromium|webkit] [files...]: both builds on the given runs
// (each run a comma-separated list of URLs under the server), with times,
// the result's hash (the same for 1 and N threads) and the module memory.
import { chromium, webkit } from "playwright";
const [browserName = "chromium", ...runs] = process.argv.slice(2);
const files = runs.map((r) => r.split(","));
const browser = await ({ chromium, webkit })[browserName].launch();
// a hung check is a failure, not a wait
setTimeout(() => { console.log("TIMEOUT"); process.exit(3); }, +(process.env.LIMIT || 120) * 1000);
const page = await browser.newPage();
page.on("console", (m) => console.log("  console:", m.text()));
await page.goto("http://localhost:8765/");
await page.waitForFunction(() => window.check);
console.log(`${browserName}: crossOriginIsolated = ${await page.evaluate(() => window.isolated)}`);
const threads = await page.evaluate(() => navigator.hardwareConcurrency);
const builds = (process.env.BUILDS || "st,mt").split(",");
for (const [build, n] of [["st", 1], ["mt", threads]].filter(([b]) => builds.includes(b))) {
  const res = await page.evaluate(([b, n, f]) => window.check(b, n, f, ["--format", "jpg", "--dpi", "200"]), [build, n, files]);
  for (const r of res)
    console.log(`  ${build} x${n}  ${r.run.padEnd(44)} ${r.secs.toFixed(2)}s  steps ${r.steps}  pages ${r.pages}  pdf ${r.hash}${r.card ? "  card-contract" : ""}  mem ${r.mem.toFixed(0)} MB`);
}
await browser.close();
process.exit(0);
