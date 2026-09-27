// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The 3-byte IEEE block: parse, from_ipv6, parse_exact, Display.

use super::ipv6::expand_ipv6;
use super::{assignable, hex_nibbles};

/// One IEEE-assigned block: the first three bytes of a hardware address.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Oui(pub(crate) [u8; 3]);

impl std::fmt::Display for Oui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02X}:{:02X}:{:02X}", self.0[0], self.0[1], self.0[2])
    }
}

impl Oui {
    /// The OUI of any hardware address the bench actually produces.
    ///
    /// Accepts everything the API accepts — `00-11-22-33-44-55`, `00:11:22:…`,
    /// `0011.2233.4455`, `001122334455` — by the simple expedient of keeping the
    /// hex digits and discarding everything else.
    ///
    /// It also accepts the **EUI-64** form, which matters more here than the
    /// tidy MAC-48 case: a PTP clock identity and an AVDECC entity ID are both a
    /// MAC with `FF:FE` inserted in the middle, so `00:0A:92:FF:FE:01:56:A3` is
    /// the same PreSonus-or-whoever block as `00:0A:92:01:56:A3`. Since the OUI
    /// is the leading three bytes either way, the insertion needs no special
    /// case — only the length check has to allow 16 hex digits as well as 12.
    ///
    /// Returns `None` for anything that cannot have a manufacturer:
    ///
    /// * **multicast** (low bit of the first byte) — `01:00:5E:…`, `91:E0:F0:…`
    ///   and every other group address. AVDECC's own discovery address is one of
    ///   these, so an agent that forwarded raw frame destinations here would
    ///   otherwise burn the daily budget on `91:E0:F0`.
    /// * **locally administered** (second bit) — a MAC someone assigned by hand
    ///   or a hypervisor invented. IEEE assigned it to nobody by definition.
    /// * **all zero** — the "no address" placeholder several of these protocols
    ///   emit before they know their own identity.
    pub fn parse(raw: &str) -> Option<Self> {
        let hex = hex_nibbles(raw);

        // 12 nibbles is a MAC-48, 16 an EUI-64. Anything else is not an address
        // that got mangled — it is a different kind of string, and guessing at
        // it would put junk in the cache and spend requests on it.
        if hex.len() != 12 && hex.len() != 16 {
            return None;
        }

        let byte = |i: usize| (hex[i * 2] << 4) | hex[i * 2 + 1];
        let oui = [byte(0), byte(1), byte(2)];

        if !assignable(&oui) {
            return None; // multicast, locally administered, or all zero
        }
        Some(Oui(oui))
    }

    /// `00:0A:92` — the form the API takes and the cache file stores.
    pub fn as_query(&self) -> String {
        self.to_string()
    }

    /// The OUI hiding inside an IPv6 address, if the host built it from its MAC.
    ///
    /// This is how a vendor name reaches the tables that have no MAC column at
    /// all. DNS-SD publishes no hardware address — but it publishes link-local
    /// addresses, and a **SLAAC** address derives its low 64 bits from the MAC
    /// by the *modified* EUI-64 rule: insert `FF:FE` in the middle, and **flip
    /// the universal/local bit**.
    ///
    /// ```text
    /// fe80::46fa:66ff:fee4:2fbf   ->  44:FA:66:E4:2F:BF
    /// ```
    ///
    /// That flip is why this cannot go through [`Oui::parse`]. Inverting bit 1
    /// makes a globally-assigned MAC look locally-administered, so the address
    /// above presents as `46:FA:66` — which `parse` rejects on sight, correctly,
    /// because in a plain MAC that bit means "IEEE assigned this to nobody".
    /// Undoing the flip first turns it back into Brother's real block.
    ///
    /// Returns `None` for a **privacy address** (RFC 4941), where the interface
    /// ID is random and has no `FF:FE` marker. Apple devices use these, so their
    /// rows legitimately have no vendor to show — and inventing one from random
    /// bytes would be worse than a blank cell.
    pub fn from_ipv6(addr: &str) -> Option<Self> {
        // Trim the zone index a link-local address carries: `%enp5s0f0`.
        let addr = addr.split('%').next()?.trim();
        let bytes = expand_ipv6(addr)?;

        // Only the interface ID matters, and only if it was built from a MAC.
        let iid = &bytes[8..];
        if iid[3] != 0xFF || iid[4] != 0xFE {
            return None; // privacy address, or a hand-assigned one
        }

        let first = iid[0] ^ 0x02; // undo the modified-EUI-64 flip
        let oui = [first, iid[1], iid[2]];
        if !assignable(&oui) {
            return None;
        }
        Some(Oui(oui))
    }

    /// Parse the 3-byte form the cache file stores.
    ///
    /// Separate from [`Oui::parse`], which insists on a whole address: a cache
    /// line is 6 hex digits and would fail that length check.
    pub(crate) fn parse_exact(raw: &str) -> Option<Self> {
        let hex = hex_nibbles(raw);
        if hex.len() != 6 {
            return None;
        }
        let byte = |i: usize| (hex[i * 2] << 4) | hex[i * 2 + 1];
        Some(Oui([byte(0), byte(1), byte(2)]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_separator_the_api_accepts() {
        let expected = Oui([0x00, 0x11, 0x22]);
        for form in [
            "00-11-22-33-44-55",
            "00:11:22:33:44:55",
            "00.11.22.33.44.55",
            "001122334455",
            "0011.2233.4455",
            "00/11/22/33/44/55",
            "  00:11:22:33:44:55  ",
        ] {
            assert_eq!(Oui::parse(form), Some(expected), "failed on {form}");
        }
    }

    /// The case this crate exists for: a PTP clock identity and an AVDECC
    /// entity ID are the device's MAC with FF:FE wedged into the middle, and
    /// both must resolve to the same vendor as the plain MAC.
    #[test]
    fn eui64_resolves_to_the_same_block_as_the_mac() {
        let clock_id = Oui::parse("00:0A:92:FF:FE:01:56:A3").unwrap();
        let mac = Oui::parse("00:0A:92:01:56:A3").unwrap();
        assert_eq!(clock_id, mac);
        assert_eq!(clock_id.to_string(), "00:0A:92");
    }

    #[test]
    fn lowercase_is_fine_and_output_is_not() {
        assert_eq!(Oui::parse("fc:a1:3e:2a:1c:33").unwrap().to_string(), "FC:A1:3E");
    }

    /// Every rejection here is a request not spent.
    #[test]
    fn addresses_that_cannot_have_a_vendor_are_never_queried() {
        // AVDECC's own discovery destination — the one an L2 agent is most
        // likely to hand over by accident.
        assert_eq!(Oui::parse("91:E0:F0:01:00:00"), None, "multicast");
        assert_eq!(Oui::parse("01:00:5E:00:00:01"), None, "IPv4 multicast");
        assert_eq!(Oui::parse("FF:FF:FF:FF:FF:FF"), None, "broadcast");
        assert_eq!(Oui::parse("02:42:AC:11:00:02"), None, "locally administered");
        assert_eq!(Oui::parse("00:00:00:00:00:00"), None, "the null address");
    }

    #[test]
    fn strings_that_are_not_addresses_are_rejected() {
        for junk in ["", "-", "unknown", "192.168.1.1", "00:11:22", "00:11:22:33:44:55:66"] {
            assert_eq!(Oui::parse(junk), None, "accepted {junk:?}");
        }
    }

    /// The Brother printer is the case that proves the bit flip, because the
    /// same device states its MAC twice by two unrelated routes: the link-local
    /// address and the hostname `BRW44FA66E42FBF.local`. They have to agree.
    #[test]
    fn ipv6_link_local_yields_the_mac_the_hostname_also_states() {
        let from_addr = Oui::from_ipv6("fe80::46fa:66ff:fee4:2fbf%enp5s0f0").unwrap();
        assert_eq!(from_addr.to_string(), "44:FA:66");
        // BRW44FA66E42FBF — the hostname's own copy of the same address.
        assert_eq!(from_addr, Oui::parse("44:FA:66:E4:2F:BF").unwrap());
    }

    #[test]
    fn ipv6_without_the_flip_would_have_been_rejected() {
        // The raw interface ID reads as locally administered, and `parse` is
        // right to refuse it. Only undoing the flip recovers the real block.
        assert_eq!(Oui::parse("46:FA:66:E4:2F:BF"), None);
        assert!(Oui::from_ipv6("fe80::46fa:66ff:fee4:2fbf").is_some());
    }

    #[test]
    fn slaac_addresses_across_the_bench() {
        for (addr, expected) in [
            ("fe80::4ad6:d5ff:fe8d:d6b7%enp5s0f0", "48:D6:D5"),
            ("fe80::f2ef:86ff:fe51:1afb%wlp13s0", "F0:EF:86"),
            ("fe80::22df:b9ff:fe9e:cd4b", "20:DF:B9"),
        ] {
            assert_eq!(Oui::from_ipv6(addr).unwrap().to_string(), expected, "{addr}");
        }
    }

    /// A privacy address is random bytes. Reading a vendor out of it would be
    /// inventing one, which is worse than the blank cell it replaces.
    #[test]
    fn privacy_addresses_yield_nothing() {
        assert_eq!(Oui::from_ipv6("fe80::1029:72a0:3793:8b5b%enp5s0f0"), None);
        assert_eq!(Oui::from_ipv6("44.44.44.155"), None);
        assert_eq!(Oui::from_ipv6("not an address"), None);
    }
}
