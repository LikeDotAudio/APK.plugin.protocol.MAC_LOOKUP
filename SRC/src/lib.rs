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
//! **Thirteen crates in this workspace depend on it**, which makes it the one
//! member here whose failure is not local: `apkaudio-appletv`,
//! `apkaudio-chromecast`, `apkaudio-dante`, `apkaudio-dnssd`,
//! `apkaudio-milan`, `apkaudio-netbox`, `apkaudio-netgear`, `apkaudio-nmos`,
//! `apkaudio-nmos-control`, `apkaudio-ping`, `apkaudio-playstation`,
//! `apkaudio-printers` and `apkaudio-ptp`. A library whose callers are
//! invisible gets changed as though it had none — a widened signature or a
//! stricter parse here costs thirteen compiles, and `cargo check --workspace`
//! in `PODS/POD.THICK/DISCOVERY.Engine/SRC` is what will say so.
//!
//! ```no_run
//! let vendors = apkaudio_maclookup::MacVendors::start();
//! // First call for an unseen OUI: None, and the OUI is queued.
//! let _ = vendors.vendor("00:0A:92:FF:FE:01:56:A3");
//! // A moment later the worker has answered, and it stays answered.
//! ```


mod address;
mod vendors;

pub use address::{Mac, Oui};
pub use vendors::MacVendors;

/// This crate's own self test — what it can prove about itself with nothing
/// plugged in. Reached from the bus through `outgoing/selftest_run`; see
/// `apkaudio_contracts::self_test` for the two inputs and the withdrawal the
/// first of them performs. PLAN-459.01.
pub mod self_test;
