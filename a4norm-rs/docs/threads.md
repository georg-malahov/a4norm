# The threaded build's pool, and letting it go

`web/dist/mt` runs rayon over Web Workers on a shared memory. Its pool can
now be released: the threads leave rayon's loop, and their workers free what
they took and close. After a release, the pool can be started again.

## Why

The pool used to be wasm-bindgen-rayon's, a global rayon pool. A global pool
is never dropped: between runs its threads wait in `Atomics.wait`.

WebKit, the engine of every browser on an iPhone, does not stop such threads
when their page is reloaded or left, or when the worker that owns them is
terminated. They keep burning CPU until the browser is killed, and each
reload after a scan adds more. The phone warms up after the work is done.

Ending the worker from JS does not help: `terminate()` and `self.close()`
leave the threads as they are.

## The API

```js
await m.initThreadPool(n);    // workers started, the pool built over them
const r = m.process(...);     // the runs, as before
await m.releaseThreadPool();  // the threads leave, the workers close
await m.initThreadPool(n);    // and again, when a run may come
```

- **`initThreadPool(n)`** starts `n` workers and resolves when they run.
  - Called again with the same `n`, it gives the same promise, so it can be
    called on every sign that a run may come.
  - Called with another `n`, it releases the old pool first.
- **`releaseThreadPool()`** tells the threads to leave within the call itself.
  The promise resolves when each worker has:
  - left rayon;
  - freed its thread's stack and thread-locals in the shared memory;
  - closed itself.

  Only a worker that has not answered in 2 s is terminated; one that has
  closed itself is left alone (terminating it right after it closed could
  leave it lingering in Chromium). The promise resolves with how many had
  to be terminated, normally 0. Without a pool
  the call does nothing, so calling it twice is safe. Called while
  `initThreadPool` is still loading the workers, it closes them before they
  enter rayon.
- **Without a pool**, before `initThreadPool` or after a release, every call
  still works, on one thread. It is slower but gives the same bytes.
- **A run in progress** cannot be released. `process` and the other calls are
  synchronous, so a `releaseThreadPool` that arrives as a message runs after
  the run.
- **Feature test:** `typeof m.releaseThreadPool === 'function'`. Older builds
  do not have it.

**When a page should call it: soon after its runs.** A release requested
while the page is already going away comes too late in WebKit. A message
posted to the worker from `pagehide` or from `visibilitychange` to hidden
still left the threads running after a reload (measured below).

Starting again is cheap: `initThreadPool` after a release takes 13 ms in
WebKit and 40 ms in Chromium. So a page can:
- `await initThreadPool(n)` before each run;
- release the pool when its queue is empty, or after a second or two idle.

`pagehide` may still post a release message, but it cannot be relied on.

## How it works

`src/pool.rs` and `src/pool.js` replace wasm-bindgen-rayon.

**The pool.** It is an ordinary `rayon::ThreadPool`:
- its `spawn_handler` queues rayon's threads, and each worker takes up one;
- dropping it is rayon's own termination: each thread is woken and leaves its
  loop once out of work.

**The work runs on the pool's threads, never on the caller's.** The caller
(the page's worker) keeps two jobs:
- relaying the progress to JS, since only it may call JS;
- waiting for the result.

Joining the caller to the pool (`use_current_thread`) would tie it to that
pool for good, so the pool could not be built again.

**Each pool worker** runs `src/pool.js`. It loads the module on the shared
memory, runs one rayon thread until the pool is dropped, calls
`__wbindgen_thread_destroy` and closes. Without that call, each start and
release would leave a stack in the 512 MiB memory for every thread.

## Measured

Playwright WebKit on an M-series Mac: a 12 MP photo, 8 threads (what WebKit
reports). Each load of `web/check/life.html` scans the photo. After a load
settles, the CPU of the web process is measured over the next 10 s:

| ms of CPU per s | load 1 | load 2 | load 3 | load 4 |
|---|---|---|---|---|
| before (wasm-bindgen-rayon) | 0 | 81 | 266 | 394 |
| this pool, never released | 1 | 146 | 296 | 397 |
| released after the scan | 1 | 2 | 1 | 2 |
| a release posted on `pagehide` | 2 | 177 | 316 | 418 |
| a release posted on hidden | 1 | 191 | 270 | |

The busy threads are those of the loads before, and they add up.

| | WebKit | Chromium (14 threads) |
|---|---|---|
| `initThreadPool`, first | 6–12 ms | 32 ms |
| `releaseThreadPool` | 0–2 ms | 3 ms |
| `initThreadPool` after a release | 13 ms | 38 ms |
| 12 MP scans, mt, before → now | 0.61, 0.46 → 0.63, 0.47 s | 0.57, 0.46 → 0.58, 0.47 s |
| the same with `--magic` | 0.83, 0.90 → 0.80, 0.87 s | 0.74, 0.76 → 0.75, 0.73 s |

The output is the same, byte for byte:
- the pool against no pool;
- a scan before a release against one after it;
- this build against the one before it, st and mt, on every example, in
  both browsers.

```sh
node web/check/server.mjs 8765 &
node web/check/life.mjs webkit release 4 /examples/notebook-photo.jpg
```

The modes are:
- `keep`;
- `release`;
- `hide`: a release from `pagehide` only;
- `hidden`: a release on `visibilitychange` only;
- `cycle`: release, init, scan again, release, then a scan with no pool.
