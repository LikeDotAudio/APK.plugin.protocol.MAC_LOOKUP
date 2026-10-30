# APK:lib:MAC_LOOKUP

MAC vendor lookup — who made this device, from the first 24 bits of its hardware address — as a Rust library, cache first, with the public API https://api.macvendors.com behind it under a hard budget.

| | |
|---|---|
| **Recipe** | none — a library, not a container: it is linked into the binaries that use it |
| **Source** | `SRC/` (this repository — https://github.com/LikeDotAudio/APK.plugin.protocol.MAC_LOOKUP.git) — crate `apkaudio-maclookup`; bin `mac-lookup` is the hand tool |
| **Used by** | thirteen plugin crates: APPLETV, CHROMECAST, DANTE, DNSSD, MILAN, NETBOX, NETGEAR, NMOS, NMOS_CONTROL, PING, PLAYSTATION, PRINTERS, PTP. Not the orchestrator — `ORCHESTRATOR:hunter:VISA/Cargo.toml` says so |
| **Workspace** | `PODS/POD.THICK/DISCOVERY.Engine/SRC/Cargo.toml` (shared `target/` at the repository root) |
| **Family / Category** | LIBRARY — address/vendor library; no bus surface, no `config.ini`, no port. The only crate in the workspace that talks to the public internet |

Was `APK:BareMetal/PLUGINS/PLUGIN:protocol:MAC_LOOKUP`.

## What it does

| File | What is in it |
|---|---|
| `SRC/src/address/` | `Oui` and `Mac`: parse every spelling the bench produces (`00-11-…`, `00:11:…`, `0011.2233.4455`, bare hex, EUI-64 with `FF:FE`), `from_ipv6` for SLAAC addresses; refuse multicast, locally administered and all-zero addresses locally |
| `SRC/src/vendors/` | `MacVendors` — `start()`, `vendor(mac)` (never blocks: cache hit or `None` and the OUI is queued), `label(mac)`, `vendor_of_any` / `mac_of_any`, `blocking_vendor(mac)` (waits for its turn), `known()` |
| `SRC/src/vendors/budget.rs` | 1000 requests a day, one every 1.1 s, a 60 s back-off after a 429 |
| `SRC/src/vendors/cache.rs` | the TSV cache: `OUI<TAB>vendor`, an empty vendor for "IEEE assigned this to nobody", and a `#budget<TAB>day<TAB>spent` line |
| `SRC/src/bin/mac-lookup.rs` | `mac-lookup [--cached] [MAC ...]`, or addresses on stdin |
| `SRC/src/self_test.rs` | the self test, module name `maclookup` |

**The cache file.** `APKAUDIO_MAC_CACHE` names it; without it, `mac_vendors.tsv` beside `Cargo.toml` (`CARGO_MANIFEST_DIR`, baked in at compile time), or `./mac_vendors.tsv` when that directory is gone. The `SRC/mac_vendors.tsv` in this repository is that default file — a seed of seven OUIs that grows whenever the library runs from a checkout. In a plugin container `DISCOVERY.Engine/Docker/plugin-base.yml` mounts the named volume `apk-audio-mac-lookup-cache` at `/var/lib/apkaudio/mac`, so every plugin shares one cache and asks the API once.

## Build, run, test

```sh
cd PODS/POD.THICK/DISCOVERY.Engine/SRC
cargo test -p apkaudio-maclookup              # 25 unit tests
cargo run -p apkaudio-maclookup --bin mac-lookup -- 00:0A:92:FF:FE:01:56:A3
cargo run -p apkaudio-maclookup --bin mac-lookup -- --cached
```

Checked 2026-10-30: `cargo test -p apkaudio-maclookup` — 25 passed; `mac-lookup --help` prints its usage.

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
