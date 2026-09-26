// A cross-origin-isolated page for the browser check: COOP same-origin +
// COEP require-corp on everything, as malahov.io serves the scanner.
//   node server.mjs PORT  -> /dist/ (the build), /examples/, /photos/ (PHOTOS)
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join } from "node:path";
const here = new URL(".", import.meta.url).pathname;
const roots = {
  "/dist/": join(here, "../dist/"),
  "/examples/": join(here, "../../../examples/"),
  "/photos/": process.env.PHOTOS || "/nonexistent/",
  "/": here,
};
const types = { ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".html": "text/html", ".json": "application/json" };
createServer(async (req, res) => {
  const url = decodeURIComponent(req.url.split("?")[0]);
  const pre = Object.keys(roots).find((p) => url.startsWith(p));
  try {
    const file = join(roots[pre], url.slice(pre.length) || "index.html");
    const body = await readFile(file);
    res.writeHead(200, {
      "Content-Type": types[extname(file)] || "application/octet-stream",
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
      "Cross-Origin-Resource-Policy": "same-origin",
    });
    res.end(body);
  } catch {
    res.writeHead(404).end();
  }
}).listen(+process.argv[2] || 8765);
