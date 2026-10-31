// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! `mac_vendors.tsv`: where it is, reading it, and writing it SAFELY when a
//! dozen processes share it.
//!
//! # Many writers, one file
//!
//! In the plugin image every plugin container mounts the same volume
//! (`apk-audio-mac-lookup-cache`) and points `APKAUDIO_MAC_CACHE` at the same
//! file, so a dozen independent processes read and write it. Until the
//! 2026-10-30 audit each one read the file once at start and later rewrote it
//! whole from its own memory: last writer won, other containers' answers were
//! erased, and the `#budget` line was per process — twelve containers could
//! spend twelve days' allowance in one.
//!
//! Now every touch of the file is a **read-merge-write under an advisory lock**
//! (`flock` on `<cache>.lock`, via `std::fs::File::lock`):
//!
//! * [`with_locked`] takes the exclusive lock, re-reads the file, lets the
//!   caller change the [`Disk`] image, and — only if it changed — writes a temp
//!   file, `fsync`s it and renames it over the cache (atomic: a reader never
//!   sees half a file, and a crash leaves the old one).
//! * Answers are **merged**, never replaced: `persist` adds what this process
//!   learned to what is on disk and takes back what the others learned.
//! * The **daily budget and the request spacing live in the file** (`#budget`
//!   and `#next` lines), charged under the lock — so the 1000/day and 1/s limits
//!   are shared by every process on the volume, which is what the API's
//!   per-source-address limit actually is.
//!
//! The lock is advisory and the file is a hint store: if the lock cannot be
//! taken (read-only volume, odd filesystem) a process falls back to its own
//! in-memory gate, exactly the old behaviour. Losing the cache costs budget,
//! never correctness.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::budget::today;
use super::state::Known;
use super::MacVendors;
use crate::Oui;

/// Where the writable cache lives when `APKAUDIO_MAC_CACHE` does not say.
///
/// The per-user cache directory — `$XDG_CACHE_HOME/apkaudio/mac_vendors.tsv`,
/// else `$HOME/.cache/apkaudio/mac_vendors.tsv`, else the system temp dir.
/// Every image sets `APKAUDIO_MAC_CACHE`, so this is the dev-box case only.
///
/// It used to be beside `CARGO_MANIFEST_DIR`, i.e. inside the git checkout at
/// run time (audit 2026-10-30, finding 4): surprising, and the reason the repo
/// needed `.gitignore` entries for its own runtime state. A user cache dir is
/// shared by every tool on the box just as well, because every process using
/// this crate resolves the same path regardless of its working directory.
pub(super) fn default_cache_path() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = env("XDG_CACHE_HOME")
        .or_else(|| env("HOME").map(|h| h.join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("apkaudio").join("mac_vendors.tsv")
}

/// Everything the shared file holds.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct Disk {
    pub(super) known: HashMap<Oui, Known>,
    /// Day number the `spent` tally belongs to.
    pub(super) day: u64,
    /// Requests charged today by EVERY process sharing the file.
    pub(super) spent: u32,
    /// Unix milliseconds before which no process may send the next request —
    /// `now + spacing` after a request, `now + back-off` after a 429.
    pub(super) next_ms: u64,
}

impl Disk {
    /// A new day restores the allowance.
    pub(super) fn roll_day(&mut self, now: u64) {
        if self.day != now {
            self.day = now;
            self.spent = 0;
        }
    }
}

/// The lock file beside the cache: `<cache>.lock`. A separate file, because
/// the cache itself is replaced by rename and a lock on a replaced inode
/// would lock nothing.
fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".lock");
    path.with_file_name(name)
}

/// Read-merge-write under the exclusive lock.
///
/// `f` gets the current on-disk image and returns its answer plus whether it
/// changed the image; only a changed image is written. Returns `None` when the
/// lock could not be taken — the caller then falls back to process-local
/// state. Never call this while holding the `MacVendors` mutex: the lock order
/// is file lock first, mutex second.
pub(super) fn with_locked<T>(path: &Path, f: impl FnOnce(&mut Disk) -> (T, bool)) -> Option<T> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(parent);
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path(path))
        .ok()?;
    lock.lock().ok()?;

    let mut disk = load_cache(path);
    let (answer, changed) = f(&mut disk);
    if changed {
        let _ = write_atomic(path, &render(&disk));
    }
    // Dropping `lock` closes the descriptor, which releases the flock.
    drop(lock);
    Some(answer)
}

/// Write-then-rename, so a reader (or a restart mid-write) never sees a
/// truncated cache. The temp name is unique per process and call, so even a
/// writer that does not take the lock (an older binary) cannot interleave
/// with ours inside one temp file.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".{}.{}.tmp", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    let tmp = path.with_file_name(name);
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Merge `learned` into `into`. A name beats "unassigned" (a later 404 for a
/// block somebody else already has a name for is the odd one out); otherwise
/// the newer answer wins.
pub(super) fn merge(into: &mut HashMap<Oui, Known>, learned: impl IntoIterator<Item = (Oui, Known)>) -> bool {
    let mut changed = false;
    for (oui, what) in learned {
        match (into.get(&oui), &what) {
            (Some(old), new) if old == new => {}
            (Some(Known::Vendor(_)), Known::Unassigned) => {}
            _ => {
                into.insert(oui, what);
                changed = true;
            }
        }
    }
    changed
}

impl MacVendors {
    pub(super) fn remember(&self, oui: Oui, what: Known) {
        let (lock, _) = &*self.shared;
        if let Ok(mut shared) = lock.lock() {
            shared.known.insert(oui, what.clone());
            shared.learned.insert(oui, what);
            shared.pending.remove(&oui);
        }
        self.persist();
    }

    /// Add what this process learned to the shared file, and take back what
    /// every other process learned. Cheap enough to do per answer — the file
    /// is one short line per vendor and answers arrive at most once a second.
    pub(super) fn persist(&self) {
        let Some(path) = self.path.as_deref() else { return };
        let (lock, _) = &*self.shared;
        let learned: Vec<(Oui, Known)> = match lock.lock() {
            Ok(mut shared) => shared.learned.drain().collect(),
            Err(_) => return,
        };
        if learned.is_empty() {
            return;
        }
        let merged = with_locked(path, |disk| {
            let changed = merge(&mut disk.known, learned.iter().cloned());
            (disk.known.clone(), changed)
        });
        if let Ok(mut shared) = lock.lock() {
            match merged {
                Some(theirs) => {
                    merge(&mut shared.known, theirs);
                }
                // Could not reach the file: keep the answers for next time.
                None => shared.learned.extend(learned),
            }
        }
    }
}

/// The tracked seed, compiled in. Read-only by construction: it is a string in
/// the binary, so nothing at run time can rewrite the file git tracks, and a
/// self test can prove the table loads without touching the disk.
pub(super) const SEED: &str = include_str!("../../mac_vendors.seed.tsv");

/// The baked-in seed as a table. Budget lines, if any, are ignored.
pub(super) fn seed() -> HashMap<Oui, Known> {
    parse_cache(SEED).known
}

pub(super) fn load_cache(path: &Path) -> Disk {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_cache(&text),
        Err(_) => Disk { day: today(), ..Disk::default() },
    }
}

/// The file format. Lines starting `#` are comments except the two state lines:
///
/// ```text
/// #budget<TAB><day number><TAB><requests spent that day>
/// #next<TAB><unix ms before which no request may leave>
/// 00:0A:92<TAB>PreSonus Audio Electronics
/// 00:07:F5<TAB>                      ← empty name: IEEE assigned it to nobody
/// ```
///
/// `#next` is new (2026-10-30); an older reader skips it as a comment, so the
/// format stays readable both ways.
fn parse_cache(text: &str) -> Disk {
    let now = today();
    let mut disk = Disk { day: now, ..Disk::default() };

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("#budget\t") {
            let mut parts = rest.split('\t');
            let stored_day: u64 = parts.next().and_then(|d| d.trim().parse().ok()).unwrap_or(0);
            let stored_spent: u32 = parts.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            // Yesterday's tally is not today's problem.
            if stored_day == now {
                disk.spent = stored_spent;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("#next\t") {
            disk.next_ms = rest.trim().parse().unwrap_or(0);
            continue;
        }
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, '\t');
        let Some(oui) = parts.next().and_then(Oui::parse_exact) else { continue };
        let name = parts.next().unwrap_or("").trim();
        disk.known.insert(
            oui,
            if name.is_empty() { Known::Unassigned } else { Known::Vendor(name.to_string()) },
        );
    }
    disk
}

fn render(disk: &Disk) -> String {
    let mut out = String::new();
    out.push_str("# apkaudio-maclookup vendor cache — one OUI per line; shared by every process on this volume\n");
    out.push_str(&format!("#budget\t{}\t{}\n", disk.day, disk.spent));
    out.push_str(&format!("#next\t{}\n", disk.next_ms));
    let mut entries: Vec<(&Oui, &Known)> = disk.known.iter().collect();
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
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "oam-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cache_round_trips_names_and_negatives() {
        let dir = scratch("rt");
        let path = dir.join("mac_vendors.tsv");

        let v = MacVendors::start_with_cache(Some(path.clone()));
        v.remember(Oui([0x00, 0x0A, 0x92]), Known::Vendor("PreSonus".into()));
        v.remember(Oui([0x00, 0x07, 0xF5]), Known::Unassigned);

        let known = load_cache(&path).known;
        assert_eq!(
            known.get(&Oui([0x00, 0x0A, 0x92])),
            Some(&Known::Vendor("PreSonus".into()))
        );
        // The negative has to survive, or every restart re-asks and the day's
        // budget goes on addresses IEEE never assigned.
        assert_eq!(known.get(&Oui([0x00, 0x07, 0xF5])), Some(&Known::Unassigned));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The running-system bug: two processes started on the same file, each
    /// learns something different — both answers must survive, in the file and
    /// in each process.
    #[test]
    fn two_writers_merge_instead_of_overwriting() {
        let dir = scratch("merge");
        let path = dir.join("oui_cache.txt");

        let a = MacVendors::start_with_cache(Some(path.clone()));
        let b = MacVendors::start_with_cache(Some(path.clone()));
        a.remember(Oui([0x00, 0x0A, 0x92]), Known::Vendor("PreSonus".into()));
        b.remember(Oui([0x44, 0xFA, 0x66]), Known::Vendor("Brother".into()));

        let known = load_cache(&path).known;
        assert_eq!(known.len(), 2, "last writer won: {known:?}");
        // b merged a's answer back when it wrote.
        assert_eq!(b.vendor("00:0A:92:01:56:A3").as_deref(), Some("PreSonus"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Many threads, each with its own handle (its own open file description,
    /// so the flock really is contended), all writing at once.
    #[test]
    fn concurrent_writers_lose_nothing() {
        let dir = scratch("conc");
        let path = dir.join("oui_cache.txt");
        let handles: Vec<_> = (0..12u8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let v = MacVendors::start_with_cache(Some(path));
                    for j in 0..10u8 {
                        v.remember(Oui([0x00, i * 4, j]), Known::Vendor(format!("V{i}-{j}")));
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(load_cache(&path).known.len(), 120);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn budget_and_next_round_trip_and_old_files_still_parse() {
        let disk = Disk {
            known: HashMap::from([(Oui([0, 0x0A, 0x92]), Known::Vendor("PreSonus".into()))]),
            day: today(),
            spent: 17,
            next_ms: 1234,
        };
        assert_eq!(parse_cache(&render(&disk)), disk);

        // A file written by the pre-2026-10-30 crate: no #next line.
        let old = format!("# old\n#budget\t{}\t5\n00:0A:92\tPreSonus\n", today());
        let d = parse_cache(&old);
        assert_eq!((d.spent, d.next_ms, d.known.len()), (5, 0, 1));

        // Yesterday's tally is dropped.
        let stale = format!("#budget\t{}\t999\n", today() - 1);
        assert_eq!(parse_cache(&stale).spent, 0);
    }

    #[test]
    fn a_name_is_not_overwritten_by_a_later_unassigned() {
        let mut m = HashMap::from([(Oui([0, 1, 2]), Known::Vendor("X".into()))]);
        assert!(!merge(&mut m, [(Oui([0, 1, 2]), Known::Unassigned)]));
        assert!(merge(&mut m, [(Oui([0, 1, 3]), Known::Unassigned)]));
        assert!(merge(&mut m, [(Oui([0, 1, 3]), Known::Vendor("Y".into()))]));
    }

    #[test]
    fn the_default_cache_is_not_in_the_checkout_nor_the_seed() {
        assert!(!seed().is_empty(), "the baked seed parsed to nothing");
        let p = default_cache_path();
        assert_ne!(p.file_name(), Some(std::ffi::OsStr::new("mac_vendors.seed.tsv")));
        assert!(
            !p.starts_with(env!("CARGO_MANIFEST_DIR")),
            "the runtime cache must not land in the source checkout: {}",
            p.display()
        );
    }
}
