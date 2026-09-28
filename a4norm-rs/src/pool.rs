//! The threaded build's pool, which can be let go: rayon over Web Workers
//! that leave rayon's loop and close when the pool is released.
//!
//! A global pool (wasm-bindgen-rayon's) is never dropped: its threads wait in
//! `Atomics.wait` for as long as the page lives, and WebKit keeps such
//! threads running after the page is reloaded or left, until the browser is
//! killed. This pool is an ordinary `rayon::ThreadPool`: dropping it lets
//! each thread out of its loop; its worker then frees the thread's stack and
//! closes (`src/pool.js`).
//!
//! The work runs on the pool's threads, never on the caller's. The caller
//! only relays the progress to JS, which it alone may call, and waits. So the
//! caller never joins a pool, and a pool can be built again after a release.

use rayon::{ThreadBuilder, ThreadPool, ThreadPoolBuilder};
use std::sync::mpsc;
use std::sync::Mutex;
use wasm_bindgen::prelude::*;

static POOL: Mutex<Option<ThreadPool>> = Mutex::new(None);
/// The threads of the pool being built, each taken up by one worker.
static THREADS: Mutex<Vec<ThreadBuilder>> = Mutex::new(Vec::new());

#[wasm_bindgen(module = "/src/pool.js")]
extern "C" {
    #[wasm_bindgen(js_name = startPool)]
    fn start_pool(module: JsValue, memory: JsValue, threads: usize) -> js_sys::Promise;
    #[wasm_bindgen(js_name = stopPool)]
    fn stop_pool() -> js_sys::Promise;
}

/// Starts `threads` workers and a pool over them; resolves once they run.
/// Called again with the same count it gives the same promise, so it can be
/// called whenever a run may come. After `releaseThreadPool` it starts anew.
#[wasm_bindgen(js_name = initThreadPool)]
pub fn init_thread_pool(threads: usize) -> js_sys::Promise {
    start_pool(wasm_bindgen::module(), wasm_bindgen::memory(), threads)
}

/// Lets the pool go: its threads are told to leave at once, within this
/// call. Resolves when every worker has left rayon, freed its thread's stack
/// and closed. Without a pool it does nothing; the calls then run on one
/// thread until `initThreadPool` is called again.
#[wasm_bindgen(js_name = releaseThreadPool)]
pub fn release_thread_pool() -> js_sys::Promise {
    stop_pool()
}

#[wasm_bindgen]
#[doc(hidden)]
pub fn wbg_pool_build(threads: usize) {
    let pool = ThreadPoolBuilder::new()
        .num_threads(threads)
        .spawn_handler(|t| {
            THREADS.lock().unwrap().push(t);
            Ok(())
        })
        .build()
        .unwrap_throw();
    *POOL.lock().unwrap() = Some(pool);
}

#[wasm_bindgen]
#[doc(hidden)]
pub fn wbg_pool_drop() {
    // rayon's terminate: each thread is woken and leaves once out of work
    drop(POOL.lock().unwrap().take());
}

/// A worker's thread: rayon's loop, until the pool is dropped.
#[wasm_bindgen]
#[doc(hidden)]
pub fn wbg_pool_thread() {
    let t = THREADS.lock().unwrap().pop();
    if let Some(t) = t {
        t.run();
    }
}

/// `f` on the pool, while this thread hands what `f` reports to `on`: the
/// stage and the share done, as `run`'s progress. Without a pool, `f` runs
/// here, on one thread.
pub fn run<R: Send>(f: impl FnOnce(&dyn Fn(&str, f64)) -> R + Send, on: impl Fn(&str, f64)) -> R {
    let guard = POOL.lock().unwrap();
    let Some(pool) = guard.as_ref() else {
        drop(guard);
        return f(&on);
    };
    enum Msg<R> {
        Tick(String, f64),
        Done(R),
    }
    let (tx, rx) = mpsc::channel::<Msg<R>>();
    pool.in_place_scope(|s| {
        s.spawn(move |_| {
            let tick = |st: &str, d: f64| {
                let _ = tx.send(Msg::Tick(st.to_string(), d));
            };
            let r = f(&tick);
            let _ = tx.send(Msg::Done(r));
        });
        loop {
            match rx.recv().expect("the pool's job ended without a result") {
                Msg::Tick(st, d) => on(&st, d),
                Msg::Done(r) => return r,
            }
        }
    })
}
