// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The 6-byte hardware address: parse, from_ipv6, oui, Display.
//!
//! Since the 2026-10-30 audit this is also THE MAC helper for the plugins:
//! [`Mac::parse_any`] / [`Mac::from_octets`] / [`Mac::octets`] / `Display`
//! (`AA:BB:…`) / [`Mac::lower`] (`aa:bb:…`) replace the private
//! `parse_mac` / `format_mac` copies in PING, DANTE and MILAN.

use super::ipv6::expand_ipv6;
use super::{assignable, hex_nibbles, Oui};

/// A full 48-bit hardware address.
///
/// The OUI is what gets looked up, but the address itself is worth publishing
/// on its own: on the mDNS tabs there is no MAC column at all, and "this row is
/// `44:FA:66:E4:2F:BF`" is a fact you cannot otherwise get out of DNS-SD without
/// running `ip neigh` by hand.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Mac([u8; 6]);

impl std::fmt::Display for Mac {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hex: Vec<String> = self.0.iter().map(|b| format!("{b:02X}")).collect();
        write!(f, "{}", hex.join(":"))
    }
}

impl Mac {
    /// Any 48-bit address from its six bytes — no vendor filter. For a frame
    /// field or an interface address the caller already holds as bytes.
    pub const fn from_octets(octets: [u8; 6]) -> Self {
        Mac(octets)
    }

    /// The first six bytes of `bytes`, if there are six. The `mac_at(frame, i)`
    /// shape every packet decoder writes.
    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        Some(Mac(bytes.get(..6)?.try_into().ok()?))
    }

    /// The six bytes, in wire order.
    pub const fn octets(&self) -> [u8; 6] {
        self.0
    }

    /// Any written 48-bit address — multicast, locally administered and
    /// all-zero INCLUDED — for a caller that needs the address itself (an
    /// interface's own MAC from sysfs, a frame's destination), not a vendor.
    ///
    /// Strict about shape, unlike [`Mac::parse`]'s "keep the hex digits": the
    /// only characters allowed besides hex are the separators `:` `-` `.` and
    /// surrounding whitespace, and there must be exactly twelve hex digits.
    /// Accepts `aa:bb:cc:dd:ee:ff`, `AA-BB-CC-DD-EE-FF`, `aabb.ccdd.eeff`,
    /// `aabbccddeeff`. Colon/dash groups may be one digit (`0:1b:…`, as some
    /// tools print them), which is padded, not misread.
    pub fn parse_any(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '-' | '.')) {
            return None;
        }
        // Six colon- or dash-separated groups of 1–2 digits.
        let groups: Vec<&str> = raw.split([':', '-']).collect();
        if groups.len() == 6 && !raw.contains('.') {
            let mut mac = [0u8; 6];
            for (b, g) in mac.iter_mut().zip(&groups) {
                if g.is_empty() || g.len() > 2 {
                    return None;
                }
                *b = u8::from_str_radix(g, 16).ok()?;
            }
            return Some(Mac(mac));
        }
        // Otherwise bare (`aabbccddeeff`) or dotted (`aabb.ccdd.eeff`): twelve digits.
        if groups.len() != 1 {
            return None;
        }
        let dotted: Vec<&str> = raw.split('.').collect();
        if !(dotted.len() == 1 || (dotted.len() == 3 && dotted.iter().all(|g| g.len() == 4))) {
            return None;
        }
        let hex = hex_nibbles(raw);
        if hex.len() != 12 {
            return None;
        }
        let mut mac = [0u8; 6];
        for (i, b) in mac.iter_mut().enumerate() {
            *b = (hex[i * 2] << 4) | hex[i * 2 + 1];
        }
        Some(Mac(mac))
    }

    /// `aa:bb:cc:dd:ee:ff` — the lowercase form Linux (`ip link`, sysfs) and
    /// Dante's own tools print. `Display` is the uppercase form.
    pub fn lower(&self) -> String {
        self.to_string().to_ascii_lowercase()
    }

    /// Whether this address can name a manufacturer: not multicast, not
    /// locally administered, not all-zero. [`Mac::parse`] admits only these.
    pub fn is_vendor_assignable(&self) -> bool {
        assignable(&self.0)
    }

    /// The all-zero "no address" placeholder.
    pub fn is_zero(&self) -> bool {
        self.0 == [0; 6]
    }

    /// The MAC a SLAAC IPv6 address was built from. See [`Oui::from_ipv6`] for
    /// why the universal/local bit has to be flipped back.
    pub fn from_ipv6(addr: &str) -> Option<Self> {
        let addr = addr.split('%').next()?.trim();
        let bytes = expand_ipv6(addr)?;
        let iid = &bytes[8..];
        if iid[3] != 0xFF || iid[4] != 0xFE {
            return None;
        }
        let mac = [iid[0] ^ 0x02, iid[1], iid[2], iid[5], iid[6], iid[7]];
        if !assignable(&mac) {
            return None;
        }
        Some(Mac(mac))
    }

    /// Any of the written forms, MAC-48 only — an EUI-64 is not an address.
    pub fn parse(raw: &str) -> Option<Self> {
        let hex = hex_nibbles(raw);
        if hex.len() != 12 {
            return None;
        }
        let mut mac = [0u8; 6];
        for (i, b) in mac.iter_mut().enumerate() {
            *b = (hex[i * 2] << 4) | hex[i * 2 + 1];
        }
        if !assignable(&mac) {
            return None;
        }
        Some(Mac(mac))
    }

    /// The block this address belongs to.
    pub fn oui(&self) -> Oui {
        Oui([self.0[0], self.0[1], self.0[2]])
    }
}

impl From<[u8; 6]> for Mac {
    fn from(octets: [u8; 6]) -> Self {
        Mac(octets)
    }
}

impl From<Mac> for [u8; 6] {
    fn from(mac: Mac) -> Self {
        mac.0
    }
}

/// `str::parse::<Mac>()` is [`Mac::parse_any`]: the shape, no vendor filter.
impl std::str::FromStr for Mac {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Mac::parse_any(s).ok_or("not a 48-bit MAC address")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The evidence half: the address a DNS-SD row never states, recovered.
    #[test]
    fn full_mac_comes_back_out_of_a_slaac_address() {
        for (addr, expected) in [
            ("fe80::46fa:66ff:fee4:2fbf%enp5s0f0", "44:FA:66:E4:2F:BF"),
            ("fe80::4ad6:d5ff:fe8d:d6b7", "48:D6:D5:8D:D6:B7"),
            ("fe80::f2ef:86ff:fe51:1afb%wlp13s0", "F0:EF:86:51:1A:FB"),
            ("fe80::22df:b9ff:fe9e:cd4b", "20:DF:B9:9E:CD:4B"),
        ] {
            assert_eq!(Mac::from_ipv6(addr).unwrap().to_string(), expected, "{addr}");
        }
        assert_eq!(Mac::from_ipv6("fe80::1029:72a0:3793:8b5b"), None, "privacy");
    }

    #[test]
    fn a_mac_and_its_oui_agree() {
        let mac = Mac::from_ipv6("fe80::46fa:66ff:fee4:2fbf").unwrap();
        assert_eq!(mac.oui(), Oui::from_ipv6("fe80::46fa:66ff:fee4:2fbf").unwrap());
        assert_eq!(mac.oui().to_string(), "44:FA:66");
    }

    /// The plugins' own helpers, replaced: sysfs text in, both spellings out.
    #[test]
    fn parse_any_and_the_two_formats_replace_the_plugin_copies() {
        let m = Mac::parse_any("02:42:ac:11:00:02\n").unwrap(); // a docker veth: locally administered
        assert_eq!(m.octets(), [0x02, 0x42, 0xAC, 0x11, 0x00, 0x02]);
        assert_eq!(m.to_string(), "02:42:AC:11:00:02");
        assert_eq!(m.lower(), "02:42:ac:11:00:02");
        assert!(!m.is_vendor_assignable());
        assert_eq!(Mac::parse("02:42:ac:11:00:02"), None, "parse keeps its vendor filter");

        for ok in ["00-0A-92-01-56-A3", "000a.9201.56a3", "000A920156A3", "0:a:92:1:56:a3"] {
            assert_eq!(Mac::parse_any(ok), Some(Mac([0, 0x0A, 0x92, 1, 0x56, 0xA3])), "{ok}");
        }
        for bad in ["", "00:0A:92:01:56", "00:0A:92:01:56:A3:FF", "00:0A:92:01:56:ZZ", "000:A:92:01:56:A3",
                    "00:0A-92 01:56:A3x", "00:0A:92:FF:FE:01:56:A3", "0a92.0156.a3", "00:0A:92.01:56:A3"] {
            assert_eq!(Mac::parse_any(bad), None, "{bad:?}");
        }
        assert!(Mac::parse_any("00:00:00:00:00:00").unwrap().is_zero());
        assert_eq!("01:00:5e:00:00:fb".parse::<Mac>().unwrap().to_string(), "01:00:5E:00:00:FB");
        assert_eq!(Mac::from_slice(&[1, 2, 3, 4, 5, 6, 7]), Some(Mac::from_octets([1, 2, 3, 4, 5, 6])));
        assert_eq!(Mac::from_slice(&[1, 2, 3]), None);
        assert_eq!(<[u8; 6]>::from(Mac::from([9; 6])), [9; 6]);
    }
}
