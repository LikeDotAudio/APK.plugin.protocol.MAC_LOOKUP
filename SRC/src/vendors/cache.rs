// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! `mac_vendors.tsv`: where it is, reading it, writing it.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::budget::today;
use super::state::Known;
use super::MacVendors;
use crate::Oui;

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
pub(super) fn default_cache_path() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    if crate_dir.is_dir() {
        crate_dir.join("mac_vendors.tsv")
    } else {
        PathBuf::from("mac_vendors.tsv")
    }
}

impl MacVendors {
    pub(super) fn remember(&self, oui: Oui, what: Known) {
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
    pub(super) fn persist(&self) {
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
}

pub(super) fn load_cache(path: &Path) -> (HashMap<Oui, Known>, u32, u64) {
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
}
