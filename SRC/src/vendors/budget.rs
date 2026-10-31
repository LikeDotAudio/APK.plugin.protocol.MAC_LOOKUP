// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The API's published limits, and the one gate that charges them.

use std::time::Duration;

use super::cache::with_locked;
use super::MacVendors;
use crate::Oui;

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

/// What the gate says to a caller that wants to ask the API about one OUI.
#[derive(Debug, PartialEq)]
pub(super) enum Slot {
    /// Go: one request charged to today's (shared) allowance.
    Granted,
    /// Today's allowance is gone — for every process sharing the cache.
    Spent,
    /// Another process already learned this OUI; it is now in this process's
    /// table too, and nothing was charged.
    Known,
}

impl MacVendors {
    /// Wait for this caller's turn to make a request about `oui`, and charge it.
    ///
    /// Both the worker and [`MacVendors::blocking_vendor`] come through here,
    /// because the limit is per source address and not per thread — before
    /// this existed, running the CLI five times in a row was five requests in
    /// one second and the API said so.
    ///
    /// With a cache file the gate is the FILE (`#budget`, `#next`, under its
    /// lock — see `cache.rs`), so every container on the shared volume pays
    /// into one allowance and keeps one spacing. Without one, or if the lock
    /// cannot be taken, it is this process's memory, as before.
    ///
    /// Sleeps with every lock RELEASED. Holding a mutex across a one-second
    /// sleep would stall every `vendor()` call on the capture thread, which is
    /// the one thing this crate promises not to do.
    pub(super) fn take_slot(&self, oui: Oui) -> Slot {
        loop {
            let wait = match self.path.as_deref().and_then(|p| self.shared_gate(p, oui)) {
                Some(Ok(slot)) => return slot,
                Some(Err(wait)) => wait,
                None => match self.local_gate() {
                    Ok(slot) => return slot,
                    Err(wait) => wait,
                },
            };
            std::thread::sleep(wait);
        }
    }

    /// The gate on the shared file. `None` = the file could not be locked.
    fn shared_gate(&self, path: &std::path::Path, oui: Oui) -> Option<Result<Slot, Duration>> {
        let (gate, theirs, spent) = with_locked(path, |disk| {
            disk.roll_day(today());
            let theirs = disk.known.get(&oui).cloned();
            if theirs.is_some() {
                return ((Ok(Slot::Known), theirs, disk.spent), false);
            }
            if disk.spent >= DAILY_BUDGET {
                return ((Ok(Slot::Spent), None, disk.spent), false);
            }
            let now = unix_ms();
            // A `#next` absurdly far ahead means the clock went backwards;
            // do not wait out someone else's wrong clock.
            let ahead = disk.next_ms.saturating_sub(now);
            if ahead > 0 && ahead <= (THROTTLE_BACKOFF + REQUEST_SPACING).as_millis() as u64 {
                return ((Err(Duration::from_millis(ahead)), None, disk.spent), false);
            }
            disk.spent += 1;
            disk.next_ms = now + REQUEST_SPACING.as_millis() as u64;
            ((Ok(Slot::Granted), None, disk.spent), true)
        })?;

        let (lock, _) = &*self.shared;
        if let Ok(mut shared) = lock.lock() {
            shared.roll_day(today());
            shared.spent = spent;
            if let Some(what) = theirs {
                shared.known.insert(oui, what);
                shared.pending.remove(&oui);
            }
        }
        Some(gate)
    }

    /// Tell every process sharing the file to sit out a 429 — the limit is
    /// per source address, so one container being throttled means all are.
    pub(super) fn note_throttled(&self) {
        if let Some(path) = self.path.as_deref() {
            with_locked(path, |disk| {
                disk.next_ms = disk.next_ms.max(unix_ms() + THROTTLE_BACKOFF.as_millis() as u64);
                ((), true)
            });
        }
    }

    /// The process-local gate: no file, or the file's lock is unavailable.
    fn local_gate(&self) -> Result<Slot, Duration> {
        let (lock, _) = &*self.shared;
        let Ok(mut shared) = lock.lock() else { return Ok(Slot::Spent) };

        shared.roll_day(today());
        if shared.spent >= DAILY_BUDGET {
            return Ok(Slot::Spent);
        }
        let wait = shared
            .last_request
            .map(|t| REQUEST_SPACING.saturating_sub(t.elapsed()))
            .unwrap_or_default();
        if wait.is_zero() {
            shared.last_request = Some(std::time::Instant::now());
            shared.spent += 1;
            return Ok(Slot::Granted);
        }
        Err(wait)
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Days since the epoch — the coarsest clock that can tell "is it tomorrow yet".
pub(super) fn today() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vendors::cache::load_cache;
    use crate::vendors::state::Known;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("oam-budget-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oui_cache.txt")
    }

    /// Two processes, one allowance: what A spends, B sees spent.
    #[test]
    fn the_budget_is_shared_through_the_file() {
        let path = scratch("shared");
        std::fs::write(&path, format!("#budget\t{}\t{}\n", today(), DAILY_BUDGET - 1)).unwrap();
        let a = MacVendors::start_with_cache(Some(path.clone()));
        let b = MacVendors::start_with_cache(Some(path.clone()));
        assert_eq!(a.take_slot(Oui([0, 0x0A, 0x92])), Slot::Granted);
        assert_eq!(b.take_slot(Oui([0, 0x0A, 0x93])), Slot::Spent, "B spent past the shared budget");
        assert_eq!(load_cache(&path).spent, DAILY_BUDGET);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Another process already answered: no charge, and the answer is ours.
    #[test]
    fn an_oui_answered_elsewhere_is_not_asked_again() {
        let path = scratch("known");
        let v = MacVendors::start_with_cache(Some(path.clone()));
        // Written by "another process" after this one started.
        std::fs::write(&path, format!("#budget\t{}\t3\n00:0A:92\tPreSonus\n", today())).unwrap();
        assert_eq!(v.vendor("00:0A:92:01:56:A3"), None);
        assert_eq!(v.take_slot(Oui([0, 0x0A, 0x92])), Slot::Known);
        assert_eq!(v.vendor("00:0A:92:01:56:A3").as_deref(), Some("PreSonus"));
        assert_eq!(load_cache(&path).spent, 3, "charged for a known OUI");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// The spacing is shared too: a second grant waits out `#next`.
    #[test]
    fn the_spacing_is_shared_through_the_file() {
        let path = scratch("spacing");
        let a = MacVendors::start_with_cache(Some(path.clone()));
        let b = MacVendors::start_with_cache(Some(path.clone()));
        let t = std::time::Instant::now();
        assert_eq!(a.take_slot(Oui([0, 1, 2])), Slot::Granted);
        assert_eq!(b.take_slot(Oui([0, 1, 4])), Slot::Granted);
        assert!(t.elapsed() >= REQUEST_SPACING - Duration::from_millis(50), "{:?}", t.elapsed());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn no_file_falls_back_to_the_process_gate() {
        let v = MacVendors::start_with_cache(None);
        assert_eq!(v.take_slot(Oui([0, 1, 2])), Slot::Granted);
        let _ = Known::Unassigned;
    }
}
