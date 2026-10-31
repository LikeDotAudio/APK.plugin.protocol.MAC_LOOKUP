// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The non-blocking and blocking questions callers ask.

use super::budget::Slot;
use super::fetch::{fetch, Fetched};
use super::state::Known;
use super::MacVendors;
use crate::{Mac, Oui};

impl MacVendors {
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
        match self.take_slot(oui) {
            Slot::Granted => {}
            Slot::Spent => return None, // out of allowance for today
            Slot::Known => return self.vendor_of(oui),
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
            Fetched::Throttled => {
                self.note_throttled();
                None
            }
            Fetched::Failed => None,
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
