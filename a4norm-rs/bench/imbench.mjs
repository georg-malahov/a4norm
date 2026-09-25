// The three stages exactly as a4norm runs them, in the browser's magick.wasm
// (malahov.io public/a4norm-web), from what bench/dump.py kept. Best of N.
//   MAGICK_WASM_DIR=.../public/a4norm-web node bench/imbench.mjs PREFIX [N]
import { readFileSync, writeFileSync } from "node:fs";
const dir = process.env.MAGICK_WASM_DIR;
if (!dir) throw new Error("set MAGICK_WASM_DIR to the folder with magick.mjs and magick.wasm");
const { default: createMagick } = await import(dir.replace(/\/?$/, "/") + "magick.mjs");
const pre = process.argv[2];
const reps = +(process.argv[3] || 3);
const M = await createMagick({ locateFile: (f) => dir.replace(/\/?$/, "/") + f,
  preRun: [(m) => m.FS.init(null, (c) => {}, (c) => c && process.stderr.write(String.fromCharCode(c)))] });
M.FS.mkdir("/w");
M.FS.writeFile("/w/in.png", readFileSync(pre + "-in.png"));
M.callMain(["/w/in.png", "/w/s0.miff"]);
const cmds = JSON.parse(readFileSync(pre + "-cmds.json", "utf8"))
  .filter((c) => c.some((a) => a.endsWith(".miff")) && !c.includes("info:"));
// stage n reads s{n}.miff and writes s{n+1}.miff
const stages = cmds.map((c, i) => {
  const miffs = c.map((a, j) => [a, j]).filter(([a]) => a.endsWith(".miff"));
  const args = c.slice(1);
  const inJ = miffs[0][1] - 1, outJ = miffs[miffs.length - 1][1] - 1;
  args[inJ] = `/w/s${i}.miff`; args[outJ] = `/w/s${i + 1}.miff`;
  return args.map((a) => a.replace(/^PNG24:.*/, `/w/s${i + 1}.miff`));
});
const names = ["flat", "neutral", "tone"];
const best = stages.map(() => Infinity);
for (let r = 0; r < reps; r++) {
  stages.forEach((a, i) => {
    const t = performance.now(); const rc = M.callMain([...a]); M._fflush(0); if (rc) console.log("FAIL", rc, a.join(" ").slice(0, 200));
    best[i] = Math.min(best[i], performance.now() - t);
  });
}
M.callMain([`/w/s${stages.length}.miff`, "PNG24:/w/out.png"]);
writeFileSync(pre + "-wasm-tone.png", M.FS.readFile("/w/out.png"));
M.callMain([`/w/s1.miff`, "PNG24:/w/o1.png"]); writeFileSync(pre + "-wasm-flat.png", M.FS.readFile("/w/o1.png"));
M.callMain([`/w/s2.miff`, "PNG24:/w/o2.png"]); writeFileSync(pre + "-wasm-neutral.png", M.FS.readFile("/w/o2.png"));
console.log(pre, names.map((n, i) => `${n} ${(best[i] / 1000).toFixed(3)}s`).join("  "),
  ` sum ${(best.reduce((a, b) => a + b) / 1000).toFixed(3)}s`);
