// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The background thread that drains the queue at the permitted rate.

use std::sync::Arc;
use std::time::Duration;

use super::budget::{today, Slot, DAILY_BUDGET, THROTTLE_BACKOFF};
use super::fetch::{fetch, Fetched};
use super::state::Known;
use super::MacVendors;
use crate::Oui;

impl MacVendors {
    /// The worker: one OUI at a time, no faster than allowed.
    pub(super) fn run(self) {
        loop {
            let oui = {
                let (lock, cv) = &*self.shared;
                let mut shared = match lock.lock() {
                    Ok(s) => s,
                    Err(_) => return,
                };
                loop {
                    if shared.stop || Arc::strong_count(&self.shared) <= 1 {
                        return;
                    }
                    // A new day restores the allowance.
                    shared.roll_day(today());
                    if shared.spent < DAILY_BUDGET {
                        if let Some(oui) = shared.queue.pop_front() {
                            break oui;
                        }
                    }
                    // Over budget, or nothing waiting: idle. Tomorrow's worker
                    // finds the queue already populated.
                    let (guard, _) = match cv.wait_timeout(shared, Duration::from_secs(30)) {
                        Ok(pair) => pair,
                        Err(_) => return,
                    };
                    shared = guard;
                }
            };

            // The gate both charges the budget and enforces the spacing, so
            // there is no sleep at the bottom of this loop.
            match self.take_slot(oui) {
                Slot::Granted => {}
                // Another container answered it meanwhile: free.
                Slot::Known => continue,
                Slot::Spent => {
                    self.requeue(oui);
                    std::thread::sleep(THROTTLE_BACKOFF);
                    continue;
                }
            }
            let fetched = fetch(oui);
            match &fetched {
                Fetched::Vendor(name) => self.remember(oui, Known::Vendor(name.clone())),
                Fetched::Unassigned => self.remember(oui, Known::Unassigned),
                // Neither of these says anything about the OUI, so neither is
                // cached — requeue and wait rather than remembering a wrong
                // "unknown" that would never be retried.
                Fetched::Throttled | Fetched::Failed => {
                    if matches!(fetched, Fetched::Throttled) {
                        self.note_throttled();
                    }
                    self.requeue(oui);
                    std::thread::sleep(THROTTLE_BACKOFF);
                }
            }
        }
    }

    fn requeue(&self, oui: Oui) {
        let (lock, _) = &*self.shared;
        if let Ok(mut shared) = lock.lock() {
            shared.queue.push_back(oui);
        }
    }
}
