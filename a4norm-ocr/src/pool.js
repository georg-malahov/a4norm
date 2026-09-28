// The threaded build's workers (src/pool.rs), as in a4norm-rs/src/pool.js
// with this module's name. This file is both the pool's
// keeper, imported by the module where initThreadPool is called, and each
// worker's script: a worker loads it by its own URL.
//
// A worker instantiates the module on the shared memory, runs one rayon
// thread until the pool is dropped, frees that thread's stack and closes.
// Between init and release nothing waits inside the module but rayon's own
// threads; after release nothing waits at all.

function waitFor(target, ...types) {
  return new Promise((resolve) => {
    target.addEventListener('message', function on({ data }) {
      if (!types.includes(data?.type)) return;
      target.removeEventListener('message', on);
      resolve(data);
    });
  });
}

// in a worker: this file is at snippets/<crate>/src/, the module three up
if (typeof self !== 'undefined' && self.name === 'a4norm-ocr-pool') waitFor(self, 'a4norm_ocr_pool_init').then(async ({ init }) => {
  const pkg = await import('../../../a4norm_ocr.js');
  const wasm = await pkg.default(init);
  postMessage({ type: 'a4norm_ocr_pool_ready' });
  const { type } = await waitFor(self, 'a4norm_ocr_pool_run', 'a4norm_ocr_pool_quit');
  if (type === 'a4norm_ocr_pool_run') pkg.wbg_pool_thread();
  // the thread's stack and thread-locals, taken from the shared memory
  wasm.__wbindgen_thread_destroy?.();
  postMessage({ type: 'a4norm_ocr_pool_exit' });
  close();
});

// The pool now: { n, ready, workers, pkg, running, stopped }. The Worker
// objects are kept: Firefox collects a worker on a shared memory otherwise.
let pool = null;

export function startPool(module, memory, n) {
  if (!(n > 0)) return Promise.reject(new Error('initThreadPool: threads must be > 0'));
  if (pool && pool.n === n) return pool.ready;
  const before = pool ? stopPool() : Promise.resolve();
  const p = { n, workers: [], running: false, stopped: false };
  p.ready = before.then(() => spawn(p, module, memory));
  pool = p;
  return p.ready;
}

async function spawn(p, module, memory) {
  p.pkg = await import('../../../a4norm_ocr.js');
  const init = { type: 'a4norm_ocr_pool_init', init: { module_or_path: module, memory } };
  p.workers = await Promise.all(Array.from({ length: p.n }, async () => {
    const w = new Worker(new URL('./pool.js', import.meta.url), { type: 'module', name: 'a4norm-ocr-pool' });
    const ready = waitFor(w, 'a4norm_ocr_pool_ready');
    const failed = new Promise((_, no) => w.addEventListener('error', (e) => no(new Error(`a4norm-ocr pool worker: ${e.message || 'failed to load'}`))));
    w.postMessage(init);
    await Promise.race([ready, failed]);
    return w;
  }));
  p.exits = p.workers.map((w) => waitFor(w, 'a4norm_ocr_pool_exit'));
  if (p.stopped) {
    // released while the workers were loading: they close without a thread
    for (const w of p.workers) w.postMessage({ type: 'a4norm_ocr_pool_quit' });
    return;
  }
  p.pkg.wbg_pool_build(p.n);
  for (const w of p.workers) w.postMessage({ type: 'a4norm_ocr_pool_run' });
  p.running = true;
}

export function stopPool() {
  const p = pool;
  pool = null;
  if (!p) return Promise.resolve();
  p.stopped = true;
  // the threads are told now, not when the promise is awaited
  if (p.running) p.pkg.wbg_pool_drop();
  return p.ready.catch(() => {}).then(async () => {
    const late = new Promise((r) => setTimeout(r, 2000));
    await Promise.race([Promise.all(p.exits || []), late]);
    for (const w of p.workers) w.terminate();
  });
}
