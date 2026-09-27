// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! Hardware addresses: the OUI block, the full MAC, and the IPv6 they hide in.

mod ipv6;
mod mac;
mod oui;

pub use mac::Mac;
pub use oui::Oui;

/// The hex digits of `raw`, one nibble each, with everything else discarded.
///
/// Every separator the API accepts — `-`, `:`, `.`, none — is handled by not
/// looking at it, which is why `Oui::parse`, `Oui::parse_exact` and
/// `Mac::parse` all start here and differ only in how many nibbles they want.
fn hex_nibbles(raw: &str) -> Vec<u8> {
    raw.chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_digit(16).unwrap_or(0) as u8)
        .collect()
}

/// Whether the leading three bytes of an address can name a manufacturer.
///
/// False for a multicast / group address (low bit of the first byte), a
/// locally administered one (second bit — IEEE assigned it to nobody), and the
/// all-zero placeholder. Every rejection here is a request not spent.
fn assignable(address: &[u8]) -> bool {
    address[0] & 0x01 == 0 && address[0] & 0x02 == 0 && address[..3] != [0, 0, 0]
}
