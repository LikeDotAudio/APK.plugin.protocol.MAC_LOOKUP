// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The locked table: what is known, what is queued, what is spent.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::Oui;

/// What is known about one OUI.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) enum Known {
    /// The API named it.
    Vendor(String),
    /// The API returned 404 — IEEE assigned this block to nobody. Remembered so
    /// it is never asked again.
    Unassigned,
}

pub(super) struct Shared {
    pub(super) known: HashMap<Oui, Known>,
    /// OUIs seen but not yet asked about, oldest first.
    pub(super) queue: VecDeque<Oui>,
    /// In the queue or in flight — so a device seen on every packet does not
    /// enqueue itself a thousand times before the first answer lands.
    pub(super) pending: HashSet<Oui>,
    /// Requests spent today, and the day they were spent on. With a shared
    /// file this mirrors the file's tally (updated whenever the gate reads it),
    /// so the worker idles when the VOLUME's allowance is gone.
    pub(super) spent: u32,
    pub(super) day: u64,
    /// When the last request left, so the rate gate is shared. The worker and a
    /// CLI call go through the same door — a tool run while an agent is
    /// resolving must not double the rate and earn both of them a 429.
    /// Used only when there is no shared file (tests, or the file's lock
    /// could not be taken); with a file, the spacing lives in it (`#next`).
    pub(super) last_request: Option<std::time::Instant>,
    /// Answers this process got from the API and has not yet merged into the
    /// shared file. Only these are written — never the whole table — so one
    /// process cannot erase another's answers.
    pub(super) learned: HashMap<Oui, Known>,
    pub(super) stop: bool,
}

impl Shared {
    /// A new day restores the allowance: `now` is today's day number.
    pub(super) fn roll_day(&mut self, now: u64) {
        if now != self.day {
            self.day = now;
            self.spent = 0;
        }
    }
}
