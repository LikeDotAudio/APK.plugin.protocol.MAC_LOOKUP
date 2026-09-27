// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The API's published limits, and the one gate that charges them.

use std::time::Duration;

use super::MacVendors;

/// The published ceiling for a keyless caller.
pub(super) const DAILY_BUDGET: u32 = 1000;

/// One request per second, plus a little — the limit is a rate, and being
/// exactly at it invites the 429 that costs more than the wait.
pub(super) const REQUEST_SPACING: Duration = Duration::from_millis(1100);

/// How long to sit out after a 429 before trying again.
///
/// Being told to slow down is not the same as being at the daily ceiling, so it
/// pauses rather than gives up — but a tight retry would just earn another 429.
pub(super) const THROTTLE_BACKOFF: Duration = Duration::from_secs(60);

impl MacVendors {
    /// Wait for this caller's turn to make a request, and charge it.
    ///
    /// Returns false when today's allowance is gone. Both the worker and
    /// [`MacVendors::blocking_vendor`] come through here, because the limit is
    /// per source address and not per thread — before this existed, running the
    /// CLI five times in a row was five requests in one second and the API said
    /// so.
    ///
    /// Sleeps with the lock RELEASED. Holding a mutex across a one-second sleep
    /// would stall every `vendor()` call on the capture thread, which is the one
    /// thing this crate promises not to do.
    pub(super) fn take_slot(&self) -> bool {
        loop {
            let wait = {
                let (lock, _) = &*self.shared;
                let Ok(mut shared) = lock.lock() else { return false };

                shared.roll_day(today());
                if shared.spent >= DAILY_BUDGET {
                    return false;
                }

                let wait = shared
                    .last_request
                    .map(|t| REQUEST_SPACING.saturating_sub(t.elapsed()))
                    .unwrap_or_default();
                if wait.is_zero() {
                    shared.last_request = Some(std::time::Instant::now());
                    shared.spent += 1;
                    shared.dirty = true;
                    return true;
                }
                wait
            };
            std::thread::sleep(wait);
        }
    }
}

/// Days since the epoch — the coarsest clock that can tell "is it tomorrow yet".
pub(super) fn today() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0)
}
