// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! Hardware addresses: the OUI block, the full MAC, and the IPv6 they hide in.

mod ipv6;
mod mac;
mod oui;

pub use mac::Mac;
pub use oui::Oui;
