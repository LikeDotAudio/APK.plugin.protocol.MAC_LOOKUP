# Architecture — LIB.MAC_LOOKUP

← [Wiki home](../README.md)

Crate `apkaudio-maclookup` (`SRC/Cargo.toml`, member of `DISCOVERY.Engine/SRC`), dependency `ureq` for one blocking GET, and the hand tool `mac-lookup`. No bus surface, no `config.ini`, no port; the only workspace crate that reaches the public internet (`https://api.macvendors.com/<OUI>`, `vendors/fetch.rs`).

## API

```rust
let vendors = apkaudio_maclookup::MacVendors::start();   // seed + cache, worker thread started
vendors.vendor("00:0A:92:01:56:A3");      // Some(name) from cache, or None and the OUI is queued
vendors.label(mac);                        // "Vendor / MAC", or the MAC until a name arrives
vendors.vendor_of_any(["44.44.44.165, fe80::46fa:66ff:fee4:2fbf"]);  // first MAC stated or implied
vendors.blocking_vendor(mac);              // waits for its budget slot (CLI use)
vendors.known();                           // every cached (OUI, vendor)
```

`Mac` and `Oui` (`address/`) parse `00-11-…`, `00:11:…`, `0011.2233.4455`, bare hex and EUI-64; `Oui::from_ipv6` reads a SLAAC address.

## Inside

| File | Job |
|---|---|
| `address/mac.rs`, `oui.rs`, `ipv6.rs` | parsing; refusing multicast / locally administered / all-zero |
| `vendors/mod.rs` | `MacVendors` handle: `start()` (seeded), `start_with_cache()` (tests, unseeded), `seed_len()`, worker spawn, stop on last drop |
| `vendors/lookup.rs` | `vendor`, `label`, `vendor_of_any`, `mac_of_any`, `blocking_vendor`, `known` |
| `vendors/worker.rs` | the single `mac-vendor-lookup` thread draining the queue |
| `vendors/budget.rs` | `take_slot`: daily budget, spacing, 429 back-off, day roll-over |
| `vendors/fetch.rs` | the HTTP GET; 404 → `Unassigned` |
| `vendors/cache.rs` | baked `SEED`, `load_cache`, `persist` (TSV, write-then-rename), `default_cache_path` |
| `vendors/state.rs` | `Shared` (known, queue, pending, spent, day) and `Known` |
| `bin/mac-lookup.rs` | `mac-lookup [--cached] [MAC ...]`, or addresses on stdin |
| `self_test.rs` | `self_test(Depth)` / `self_test_json`, module `maclookup` |

## Seed and cache

Load order in `start()`: the compiled-in `SRC/mac_vendors.seed.tsv`, then the cache file overlays it. The cache path is `APKAUDIO_MAC_CACHE`; else `mac_vendors.tsv` beside `Cargo.toml` (gitignored); else `./mac_vendors.tsv`. Format: `OUI<TAB>vendor`, an empty vendor for "assigned to nobody", and `#budget<TAB>day<TAB>spent`.

```sh
cd PODS/POD.THICK/DISCOVERY.Engine/SRC
cargo run -p apkaudio-maclookup --bin mac-lookup -- --cached
```
