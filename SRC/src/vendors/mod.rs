// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! `MacVendors`: the handle, starting its worker, and stopping it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use budget::today;
use cache::{default_cache_path, load_cache};
use state::Shared;

mod budget;
mod cache;
mod fetch;
mod lookup;
mod state;
mod worker;

/// A lookup table with a worker thread behind it.
///
/// Cloneable and shareable: every clone talks to the same cache and the same
/// worker. Dropping the last one stops the thread.
#[derive(Clone)]
pub struct MacVendors {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    path: Option<PathBuf>,
}

impl MacVendors {
    /// Start with the default cache location.
    ///
    /// `APKAUDIO_MAC_CACHE` overrides it; otherwise `mac_vendors.tsv` beside this
    /// crate. Losing the file costs a day's budget, not correctness.
    pub fn start() -> Self {
        let path = std::env::var_os("APKAUDIO_MAC_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(default_cache_path);
        Self::start_with_cache(Some(path))
    }

    /// Start against a specific cache file, or none at all (tests).
    pub fn start_with_cache(path: Option<PathBuf>) -> Self {
        let (known, spent, day) = path
            .as_deref()
            .map(load_cache)
            .unwrap_or_else(|| (HashMap::new(), 0, today()));

        let shared = Arc::new((
            Mutex::new(Shared {
                known,
                queue: VecDeque::new(),
                pending: HashSet::new(),
                spent,
                day,
                last_request: None,
                dirty: false,
                stop: false,
            }),
            Condvar::new(),
        ));

        let me = MacVendors { shared: shared.clone(), path: path.clone() };
        let worker = me.clone();
        std::thread::Builder::new()
            .name("mac-vendor-lookup".into())
            .spawn(move || worker.run())
            .ok();
        me
    }
}

impl Drop for MacVendors {
    fn drop(&mut self) {
        // 2 = this handle and the worker's. Past that, someone still holds one.
        if Arc::strong_count(&self.shared) <= 2 {
            let (lock, cv) = &*self.shared;
            if let Ok(mut shared) = lock.lock() {
                shared.stop = true;
            }
            cv.notify_all();
        }
    }
}
