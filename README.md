# APK:lib:MAC_LOOKUP

**Wiki:** [Documentation/README.md](Documentation/README.md) — principles, architecture, delivery.

MAC vendor lookup — who made this device, from the first 24 bits of its hardware address — as a Rust library, cache first, with the public API https://api.macvendors.com behind it under a hard budget.

| | |
|---|---|
| **Recipe** | none — a library, not a container: it is linked into the binaries that use it |
| **Source** | `SRC/` (this repository — https://github.com/LikeDotAudio/APK.plugin.protocol.MAC_LOOKUP.git) — crate `apkaudio-maclookup`; bin `mac-lookup` is the hand tool |
| **Used by** | thirteen plugin crates: APPLETV, CHROMECAST, DANTE, DNSSD, MILAN, NETBOX, NETGEAR, NMOS, NMOS_CONTROL, PING, PLAYSTATION, PRINTERS, PTP. Not the orchestrator — `ORCHESTRATOR:hunter:VISA/Cargo.toml` says so |
| **Workspace** | none of its own — builds standalone (`cargo test` in `SRC/`, SDK by tag `sdk-v0.5`); the DISCOVERY.Engine workspace reaches it through the link `SRC/members/LIB.MAC_LOOKUP` |
| **Family / Category** | LIBRARY — address/vendor library; no bus surface, no `config.ini`, no port. The only crate in the workspace that talks to the public internet |

Was `APK:BareMetal/PLUGINS/PLUGIN:protocol:MAC_LOOKUP`.

## What it does

| File | What is in it |
|---|---|
| `SRC/src/address/` | `Oui` and `Mac`: `Mac::parse_any`/`from_octets`/`octets`/`Display`/`lower` are THE MAC parse/format for plugins; parse every spelling the bench produces (`00-11-…`, `00:11:…`, `0011.2233.4455`, bare hex, EUI-64 with `FF:FE`), `from_ipv6` for SLAAC addresses; refuse multicast, locally administered and all-zero addresses locally |
| `SRC/src/vendors/` | `MacVendors` — `start()`, `vendor(mac)` (never blocks: cache hit or `None` and the OUI is queued), `label(mac)`, `vendor_of_any` / `mac_of_any`, `blocking_vendor(mac)` (waits for its turn), `known()` |
| `SRC/src/vendors/budget.rs` | 1000 requests a day, one every 1.1 s, a 60 s back-off after a 429 |
| `SRC/src/vendors/cache.rs` | the TSV cache: `OUI<TAB>vendor`, an empty vendor for "IEEE assigned this to nobody", and a `#budget<TAB>day<TAB>spent` line |
| `SRC/src/bin/mac-lookup.rs` | `mac-lookup [--cached] [MAC ...]`, or addresses on stdin |
| `SRC/src/self_test.rs` | the self test, module name `maclookup` |

**The seed and the cache.** `SRC/mac_vendors.seed.tsv` is tracked and read-only: it is compiled into the binary (`include_str!`, `SRC/src/vendors/cache.rs` `SEED`) and loaded first by `MacVendors::start()`. The writable cache overlays it: `APKAUDIO_MAC_CACHE` names it; without it, the per-user cache dir (`$XDG_CACHE_HOME` or `~/.cache`)`/apkaudio/mac_vendors.tsv` — never the source checkout. In a plugin container `DISCOVERY.Engine/Docker/plugin-base.yml` mounts the named volume `apk-audio-mac-lookup-cache` at `/var/lib/apkaudio/mac` (with `APKAUDIO_MAC_CACHE=/var/lib/apkaudio/mac/oui_cache.txt` set by the image), so every plugin shares one cache and asks the API once. **That file is safe for many writers** (since the 2026-10-30 audit): every write is a read-merge-write under an exclusive `flock` on `oui_cache.txt.lock` with an atomic rename, and the daily budget and request spacing are charged in the file, so the whole volume — not each container — keeps the API's 1000/day and 1/s. The self test checks the baked seed, not the disk, so an empty cache volume does not fail it.

## Build, run, test

```sh
cd PODS/POD.THICK/LIBRARY/LIB.MAC_LOOKUP/SRC && cargo test   # standalone — 35 unit tests + 1 doc test
cd PODS/POD.THICK/DISCOVERY.Engine/SRC
cargo test -p apkaudio-maclookup              # same, as a workspace member
cargo run -p apkaudio-maclookup --bin mac-lookup -- 00:0A:92:FF:FE:01:56:A3
cargo run -p apkaudio-maclookup --bin mac-lookup -- --cached
```

Checked 2026-10-30: `cargo test` in `SRC/` — 35 passed + 1 doc test; `mac-lookup --help` prints its usage.

## Philosophy

- **The budget is the design.** Cache by OUI, not by MAC; cache misses too (a 404 is a fact, not a failure); persist across restarts; never ask about an address that cannot have a vendor (`SRC/src/lib.rs` header, `SRC/src/vendors/budget.rs`, `SRC/src/address/oui.rs`).
- **Never block the caller.** Discovery calls `vendor()` from packet paths; it answers from cache or returns `None` and queues, and one worker drains the queue at the permitted rate. The budget sleep happens with the lock released (`SRC/src/vendors/lookup.rs`, `SRC/src/vendors/worker.rs`, `take_slot` in `budget.rs`).
- **Refusal over fabrication.** No vendor is guessed: an unknown OUI is `None` until the API has answered, and the CLI says which of three reasons applies — not a vendor-bearing address, assigned to nobody, or not reached (`SRC/src/bin/mac-lookup.rs`, `report`).
- **One gate for every caller.** The worker and `blocking_vendor` both pay through `take_slot`, because the limit is per source address, not per thread (`SRC/src/vendors/budget.rs`).
- **Losing the cache costs budget, not correctness.** The file is a hint store; deleting it re-learns (`SRC/src/vendors/mod.rs`, `start`).
- **The self test is a plain function** under the `apkaudio_contracts::self_test` contract, runnable by `cargo test` with no broker (`SRC/src/self_test.rs`).

---

*© 2026 — Written & Maintained by Anthony P. Kuzub. MIT Licence.*

<!-- wiki:smoke -->
**Smoke test** — ✅ PASS 1/1 since 2026-10-30 12:51 EDT — [results](Documentation/%F0%9F%A7%A9Manual%20and%20Artifacts/SMOKE.md)
<!-- /wiki:smoke -->
