// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The 6-byte hardware address: parse, from_ipv6, oui, Display.

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
}
