// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! `apkaudio-maclookup` — who made this device, from the first 24 bits of its MAC.
//!
//! Every discovery agent here ends up holding a hardware address and nothing
//! else to show for it. A PTP tab listing `00:07:F5:FF:FE:00:54:72` four times
//! is technically complete and practically useless; the same table saying
//! *Bosch Security Systems* against each one is the same data made readable.
//!
//! The first three bytes of a MAC are the OUI — the block IEEE assigned to a
//! manufacturer — so one lookup per OUI names every device that shares it.
//!
//! # The budget is the design
//!
//! <https://api.macvendors.com> needs no key and allows **1000 requests a day at
//! 1 per second**. Both limits shape this crate:
//!
//! * **Cache by OUI, not by MAC.** A bench of 40 devices is perhaps 8 vendors.
//! * **Cache misses too.** A 404 means IEEE never assigned that block. Asking
//!   again tomorrow will not change that, and each retry costs a request.
//! * **Persist across restarts**, or a restart loop spends the day's budget by
//!   lunchtime re-learning what it already knew.
//! * **Never ask about an address that cannot have a vendor** — multicast,
//!   locally-administered and all-zero addresses are free to reject locally.
//! * **Never block the caller.** [`MacVendors::vendor`] answers from cache or
//!   returns `None` and queues the OUI; one worker thread drains that queue at
//!   the permitted rate. Discovery agents call this from packet-handling paths
//!   where a one-second sleep drops frames.
//!
//! So a name appears a flush or two after a device does, and from then on it is
//! free and offline.
//!
//! # Who calls this
//!
//! **Eight crates in this workspace depend on it**, which makes it the one
//! member here whose failure is not local: `apkaudio-AppleTV`,
//! `apkaudio-AVB-Milan`, `apkaudio-ChromeCast`, `apkaudio-Dante`,
//! `apkaudio-dnssd`, `apkaudio-nmos`, `apkaudio-printers` and `apkaudio-ptp`.
//! A library whose callers are invisible gets changed as though it had none —
//! a widened signature or a stricter parse here costs eight compiles, and
//! `./.apk.scripts/check.sh comprotocols` is what will say so.
//!
//! ```no_run
//! let vendors = apkaudio_maclookup::MacVendors::start();
//! // First call for an unseen OUI: None, and the OUI is queued.
//! let _ = vendors.vendor("00:0A:92:FF:FE:01:56:A3");
//! // A moment later the worker has answered, and it stays answered.
//! ```


use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

mod address;

pub use address::{Mac, Oui};

/// What is known about one OUI.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Known {
    /// The API named it.
    Vendor(String),
    /// The API returned 404 — IEEE assigned this block to nobody. Remembered so
    /// it is never asked again.
    Unassigned,
}

struct Shared {
    known: HashMap<Oui, Known>,
    /// OUIs seen but not yet asked about, oldest first.
    queue: VecDeque<Oui>,
    /// In the queue or in flight — so a device seen on every packet does not
    /// enqueue itself a thousand times before the first answer lands.
    pending: HashSet<Oui>,
    /// Requests spent today, and the day they were spent on.
    spent: u32,
    day: u64,
    /// When the last request left, so the rate gate is shared. The worker and a
    /// CLI call go through the same door — a tool run while an agent is
    /// resolving must not double the rate and earn both of them a 429.
    last_request: Option<std::time::Instant>,
    dirty: bool,
    stop: bool,
}

/// A lookup table with a worker thread behind it.
///
/// Cloneable and shareable: every clone talks to the same cache and the same
/// worker. Dropping the last one stops the thread.
#[derive(Clone)]
pub struct MacVendors {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    path: Option<PathBuf>,
}

/// The published ceiling for a keyless caller.
const DAILY_BUDGET: u32 = 1000;

/// One request per second, plus a little — the limit is a rate, and being
/// exactly at it invites the 429 that costs more than the wait.
const REQUEST_SPACING: Duration = Duration::from_millis(1100);

/// How long to sit out after a 429 before trying again.
///
/// Being told to slow down is not the same as being at the daily ceiling, so it
/// pauses rather than gives up — but a tight retry would just earn another 429.
const THROTTLE_BACKOFF: Duration = Duration::from_secs(60);

/// `mac_vendors.tsv` beside this crate's source.
///
/// Anchored to the crate rather than to the working directory, because the
/// directory a process starts in is not a property of the cache. Eight agents
/// share this file (see the README) and they are started from wherever their
/// launcher happens to be, so a relative default scattered copies around the
/// tree — each one starting empty, each one spending its own daily budget to
/// relearn what another had already answered.
///
/// The crate directory does not exist in a deployed image, which copies the
/// binary and nothing else. That case falls back to the working directory and
/// behaves exactly as before: the cache is an optimisation, and losing it costs
/// a day's budget rather than correctness.
fn default_cache_path() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    if crate_dir.is_dir() {
        crate_dir.join("mac_vendors.tsv")
    } else {
        PathBuf::from("mac_vendors.tsv")
    }
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

    /// The manufacturer for a hardware address, if it is already known.
    ///
    /// Never blocks and never fails: an unknown OUI is queued and this returns
    /// `None`, so a caller on a capture thread pays nothing. Ask again next
    /// flush.
    pub fn vendor(&self, mac: &str) -> Option<String> {
        self.vendor_of(Oui::parse(mac)?)
    }

    /// `Vendor / 00:0A:92:FF:FE:01:56:A3`, or the address alone until the name
    /// arrives.
    ///
    /// The label a table cell wants: it is never empty, never shorter than the
    /// address it replaces, and it gains the name in place when the worker
    /// answers.
    pub fn label(&self, mac: &str) -> String {
        match self.vendor(mac) {
            Some(name) => format!("{name} / {mac}"),
            None => mac.to_string(),
        }
    }

    /// The manufacturer behind whatever identifiers a discovery record holds.
    ///
    /// For agents with no MAC column. Give it every address and identifier on
    /// the record — a comma- or space-separated address list is fine — and it
    /// takes the first thing a vendor can be read out of, trying hardware
    /// addresses before IPv6 because a MAC states the OUI where an address only
    /// implies it.
    ///
    /// DNS-SD is the caller this exists for: it publishes no hardware address,
    /// but a SLAAC link-local address carries one, so
    /// `fe80::46fa:66ff:fee4:2fbf` is enough to say *Brother*.
    pub fn vendor_of_any<'a>(&self, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
        let mut ipv6: Vec<Oui> = Vec::new();
        let mut queued: Option<String> = None;

        for candidate in candidates {
            for token in candidate.split([',', ' ', '\t', ';']) {
                let token = token.trim();
                if token.is_empty() {
                    continue;
                }
                if let Some(oui) = Oui::parse(token) {
                    if let Some(name) = self.vendor_of(oui) {
                        return Some(name);
                    }
                    queued = queued.or(Some(String::new()));
                } else if let Some(oui) = Oui::from_ipv6(token) {
                    ipv6.push(oui);
                }
            }
        }
        // Only now fall back to what an address implies.
        for oui in ipv6 {
            if let Some(name) = self.vendor_of(oui) {
                return Some(name);
            }
        }
        let _ = queued;
        None
    }

    /// The hardware address hiding in a discovery record, if one is.
    ///
    /// The companion to [`MacVendors::vendor_of_any`], and it takes the same
    /// candidates. Publish both: the vendor is the readable fact, the MAC is the
    /// evidence for it, and a row showing *Google, Inc.* with no address to
    /// check it against is a claim rather than a finding.
    ///
    /// A stated MAC wins over one inferred from an IPv6 address.
    pub fn mac_of_any<'a>(&self, candidates: impl IntoIterator<Item = &'a str>) -> Option<Mac> {
        let mut inferred: Option<Mac> = None;
        for candidate in candidates {
            for token in candidate.split([',', ' ', '\t', ';']) {
                let token = token.trim();
                if token.is_empty() {
                    continue;
                }
                if let Some(mac) = Mac::parse(token) {
                    return Some(mac);
                }
                if inferred.is_none() {
                    inferred = Mac::from_ipv6(token);
                }
            }
        }
        inferred
    }

    /// Cache hit, or queue it and answer `None`. The single place that decides.
    fn vendor_of(&self, oui: Oui) -> Option<String> {
        let (lock, cv) = &*self.shared;
        let mut shared = lock.lock().ok()?;
        match shared.known.get(&oui) {
            Some(Known::Vendor(name)) => return Some(name.clone()),
            Some(Known::Unassigned) => return None,
            None => {}
        }
        if shared.pending.insert(oui) {
            shared.queue.push_back(oui);
            cv.notify_one();
        }
        None
    }

    /// Ask now and wait for the answer — for one-shot tools, never for agents.
    pub fn blocking_vendor(&self, mac: &str) -> Option<String> {
        let oui = Oui::parse(mac)?;
        {
            let (lock, _) = &*self.shared;
            let shared = lock.lock().ok()?;
            match shared.known.get(&oui) {
                Some(Known::Vendor(name)) => return Some(name.clone()),
                Some(Known::Unassigned) => return None,
                None => {}
            }
        }
        if !self.take_slot() {
            return None; // out of allowance for today
        }
        match fetch(oui) {
            Fetched::Vendor(name) => {
                self.remember(oui, Known::Vendor(name.clone()));
                Some(name)
            }
            Fetched::Unassigned => {
                self.remember(oui, Known::Unassigned);
                None
            }
            Fetched::Throttled | Fetched::Failed => None,
        }
    }

    /// Everything learned so far, for a caller that wants to render the table.
    pub fn known(&self) -> Vec<(Oui, String)> {
        let (lock, _) = &*self.shared;
        let shared = match lock.lock() {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let mut out: Vec<(Oui, String)> = shared
            .known
            .iter()
            .filter_map(|(oui, k)| match k {
                Known::Vendor(name) => Some((*oui, name.clone())),
                Known::Unassigned => None,
            })
            .collect();
        out.sort_by_key(|(oui, _)| oui.0);
        out
    }

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
    fn take_slot(&self) -> bool {
        loop {
            let wait = {
                let (lock, _) = &*self.shared;
                let Ok(mut shared) = lock.lock() else { return false };

                let now_day = today();
                if now_day != shared.day {
                    shared.day = now_day;
                    shared.spent = 0;
                    shared.dirty = true;
                }
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

    fn remember(&self, oui: Oui, what: Known) {
        let (lock, _) = &*self.shared;
        if let Ok(mut shared) = lock.lock() {
            shared.known.insert(oui, what);
            shared.pending.remove(&oui);
            shared.dirty = true;
        }
        self.persist();
    }

    /// Write the cache out. Cheap enough to do per answer — the file is one
    /// short line per vendor and answers arrive at most once a second.
    fn persist(&self) {
        let Some(path) = self.path.as_deref() else { return };
        let (lock, _) = &*self.shared;
        let Ok(mut shared) = lock.lock() else { return };
        if !shared.dirty {
            return;
        }
        let mut out = String::new();
        out.push_str("# apkaudio-maclookup vendor cache — one OUI per line\n");
        out.push_str(&format!("#budget\t{}\t{}\n", shared.day, shared.spent));
        let mut entries: Vec<(&Oui, &Known)> = shared.known.iter().collect();
        entries.sort_by_key(|(oui, _)| oui.0);
        for (oui, known) in entries {
            let name = match known {
                Known::Vendor(n) => n.as_str(),
                // A tab-delimited empty field, so the negative survives a
                // round trip instead of being re-asked on every restart.
                Known::Unassigned => "",
            };
            out.push_str(&format!("{oui}\t{name}\n"));
        }
        shared.dirty = false;
        drop(shared);

        // Write-then-rename, or a restart during the write leaves a truncated
        // cache that reads as "nothing is known".
        let tmp = path.with_extension("tsv.tmp");
        if let Some(parent) = tmp.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::File::create(&tmp) {
            if f.write_all(out.as_bytes()).is_ok() && f.sync_all().is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    /// The worker: one OUI at a time, no faster than allowed.
    fn run(self) {
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
                    let now = today();
                    if now != shared.day {
                        shared.day = now;
                        shared.spent = 0;
                        shared.dirty = true;
                    }
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
            if !self.take_slot() {
                self.requeue(oui);
                std::thread::sleep(THROTTLE_BACKOFF);
                continue;
            }
            match fetch(oui) {
                Fetched::Vendor(name) => self.remember(oui, Known::Vendor(name)),
                Fetched::Unassigned => self.remember(oui, Known::Unassigned),
                // Neither of these says anything about the OUI, so neither is
                // cached — requeue and wait rather than remembering a wrong
                // "unknown" that would never be retried.
                Fetched::Throttled | Fetched::Failed => {
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

enum Fetched {
    Vendor(String),
    Unassigned,
    Throttled,
    Failed,
}

/// One GET. The API answers with a bare string, or 404, or 429.
fn fetch(oui: Oui) -> Fetched {
    let url = format!("https://api.macvendors.com/{}", oui.as_query());
    match ureq::get(&url)
        .timeout(Duration::from_secs(10))
        .call()
    {
        Ok(response) => match response.into_string() {
            Ok(body) => {
                let name = body.trim().to_string();
                if name.is_empty() {
                    Fetched::Unassigned
                } else {
                    Fetched::Vendor(name)
                }
            }
            Err(_) => Fetched::Failed,
        },
        // 404 is an answer: no such assignment. Anything else in 4xx/5xx is not.
        Err(ureq::Error::Status(404, _)) => Fetched::Unassigned,
        Err(ureq::Error::Status(429, _)) => Fetched::Throttled,
        Err(_) => Fetched::Failed,
    }
}

/// Days since the epoch — the coarsest clock that can tell "is it tomorrow yet".
fn today() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0)
}

fn load_cache(path: &Path) -> (HashMap<Oui, Known>, u32, u64) {
    let mut known = HashMap::new();
    let mut spent = 0;
    let mut day = today();

    let Ok(text) = std::fs::read_to_string(path) else {
        return (known, spent, day);
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("#budget\t") {
            let mut parts = rest.split('\t');
            let stored_day: u64 = parts.next().and_then(|d| d.parse().ok()).unwrap_or(0);
            let stored_spent: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            // Yesterday's tally is not today's problem.
            if stored_day == day {
                spent = stored_spent;
            } else {
                day = today();
            }
            continue;
        }
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, '\t');
        let Some(oui) = parts.next().and_then(Oui::parse_exact) else { continue };
        let name = parts.next().unwrap_or("").trim();
        known.insert(
            oui,
            if name.is_empty() { Known::Unassigned } else { Known::Vendor(name.to_string()) },
        );
    }
    (known, spent, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The DNS-SD case: an address list, no MAC anywhere, vendor still found.
    #[test]
    fn vendor_of_any_reads_the_address_list() {
        let v = MacVendors::start_with_cache(None);
        v.remember(Oui([0x44, 0xFA, 0x66]), Known::Vendor("Brother".into()));
        let addresses = "44.44.44.165, fe80::46fa:66ff:fee4:2fbf%enp5s0f0";
        assert_eq!(v.vendor_of_any([addresses]).as_deref(), Some("Brother"));
    }

    /// A real MAC on the record outranks an address that merely implies one.
    #[test]
    fn a_hardware_address_wins_over_an_inferred_one() {
        let v = MacVendors::start_with_cache(None);
        v.remember(Oui([0x00, 0x0A, 0x92]), Known::Vendor("Presonus".into()));
        v.remember(Oui([0x44, 0xFA, 0x66]), Known::Vendor("Brother".into()));
        assert_eq!(
            v.vendor_of_any(["fe80::46fa:66ff:fee4:2fbf", "00:0A:92:01:56:A3"])
                .as_deref(),
            Some("Presonus")
        );
    }

    #[test]
    fn mac_of_any_prefers_a_stated_address_over_an_inferred_one() {
        let v = MacVendors::start_with_cache(None);
        assert_eq!(
            v.mac_of_any(["fe80::46fa:66ff:fee4:2fbf", "00:0A:92:01:56:A3"])
                .unwrap()
                .to_string(),
            "00:0A:92:01:56:A3"
        );
        assert_eq!(
            v.mac_of_any(["44.44.44.165, fe80::46fa:66ff:fee4:2fbf%enp5s0f0"])
                .unwrap()
                .to_string(),
            "44:FA:66:E4:2F:BF"
        );
        assert_eq!(v.mac_of_any(["44.44.44.160"]), None);
    }

    #[test]
    fn label_falls_back_to_the_address_until_the_name_arrives() {
        let v = MacVendors::start_with_cache(None);
        assert_eq!(v.label("00:0A:92:01:56:A3"), "00:0A:92:01:56:A3");
    }

    /// A row whose address is a dash must not enqueue anything — that is the
    /// placeholder the PTP agent writes for an unknown grandmaster.
    #[test]
    fn placeholder_rows_queue_nothing() {
        let v = MacVendors::start_with_cache(None);
        assert_eq!(v.vendor("-"), None);
        let (lock, _) = &*v.shared;
        assert!(lock.lock().unwrap().queue.is_empty());
    }

    #[test]
    fn a_repeated_address_is_queued_once() {
        let v = MacVendors::start_with_cache(None);
        for _ in 0..50 {
            let _ = v.vendor("00:0A:92:FF:FE:01:56:A3");
        }
        let (lock, _) = &*v.shared;
        let shared = lock.lock().unwrap();
        // The worker may already have taken it; either way it was never twice.
        assert!(shared.queue.len() <= 1, "queued {} times", shared.queue.len());
    }

    #[test]
    fn cache_round_trips_names_and_negatives() {
        let dir = std::env::temp_dir().join(format!("oam-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mac_vendors.tsv");

        let v = MacVendors::start_with_cache(Some(path.clone()));
        v.remember(Oui([0x00, 0x0A, 0x92]), Known::Vendor("PreSonus".into()));
        v.remember(Oui([0x00, 0x07, 0xF5]), Known::Unassigned);

        let (known, _, _) = load_cache(&path);
        assert_eq!(
            known.get(&Oui([0x00, 0x0A, 0x92])),
            Some(&Known::Vendor("PreSonus".into()))
        );
        // The negative has to survive, or every restart re-asks and the day's
        // budget goes on addresses IEEE never assigned.
        assert_eq!(known.get(&Oui([0x00, 0x07, 0xF5])), Some(&Known::Unassigned));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cached_name_answers_without_the_network() {
        let v = MacVendors::start_with_cache(None);
        v.remember(Oui([0x00, 0x0A, 0x92]), Known::Vendor("PreSonus".into()));
        assert_eq!(v.vendor("00:0A:92:FF:FE:01:56:A3").as_deref(), Some("PreSonus"));
        assert_eq!(
            v.label("00:0A:92:FF:FE:01:56:A3"),
            "PreSonus / 00:0A:92:FF:FE:01:56:A3"
        );
    }

    #[test]
    fn a_cached_negative_is_not_re_queued() {
        let v = MacVendors::start_with_cache(None);
        v.remember(Oui([0x00, 0x07, 0xF5]), Known::Unassigned);
        assert_eq!(v.vendor("00:07:F5:FF:FE:00:54:72"), None);
        let (lock, _) = &*v.shared;
        assert!(lock.lock().unwrap().queue.is_empty(), "spent a request on a known 404");
    }
}

/// This crate's own self test — what it can prove about itself with nothing
/// plugged in. Reached from the bus through `outgoing/selftest_run`; see
/// `apkaudio_contracts::self_test` for the two inputs and the withdrawal the
/// first of them performs. PLAN-459.01.
pub mod self_test;
