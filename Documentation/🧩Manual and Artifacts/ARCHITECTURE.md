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

`Mac` is also the one MAC helper for plugins (since the 2026-10-30 audit), replacing private `parse_mac`/`format_mac` copies:

```rust
use apkaudio_maclookup::Mac;
let m = Mac::parse_any("02:42:ac:11:00:02")?;   // any 48-bit address, strict shape, no vendor filter
let m: Mac = "aabb.ccdd.eeff".parse()?;          // FromStr = parse_any
let m = Mac::from_octets([0, 0x0A, 0x92, 1, 0x56, 0xA3]);  // or Mac::from(bytes), Mac::from_slice(&frame[6..])
m.octets();                // [u8; 6]
m.to_string();             // "00:0A:92:01:56:A3"  (Display, uppercase)
m.lower();                 // "00:0a:92:01:56:a3"  (Linux / Dante spelling)
m.is_vendor_assignable();  // false for multicast, locally administered, all-zero
Mac::parse("…");           // vendor-bearing addresses ONLY (what a lookup wants)
```

## Inside

| File | Job |
|---|---|
| `address/mac.rs`, `oui.rs`, `ipv6.rs` | parsing; refusing multicast / locally administered / all-zero |
| `vendors/mod.rs` | `MacVendors` handle: `start()` (seeded), `start_with_cache()` (tests, unseeded), `seed_len()`, worker spawn, stop on last drop |
| `vendors/lookup.rs` | `vendor`, `label`, `vendor_of_any`, `mac_of_any`, `blocking_vendor`, `known` |
| `vendors/worker.rs` | the single `mac-vendor-lookup` thread draining the queue |
| `vendors/budget.rs` | `take_slot(oui) -> Slot`: daily budget, spacing, 429 back-off, day roll-over — charged in the shared file under its lock (`shared_gate`), or in memory when there is no file (`local_gate`) |
| `vendors/fetch.rs` | the HTTP GET; 404 → `Unassigned` |
| `vendors/cache.rs` | baked `SEED`, `load_cache`, `with_locked` (flock + re-read + atomic write), `persist` (merge, never overwrite), `default_cache_path` |
| `vendors/state.rs` | `Shared` (known, learned-but-unwritten, queue, pending, spent, day) and `Known` |
| `bin/mac-lookup.rs` | `mac-lookup [--cached] [MAC ...]`, or addresses on stdin |
| `self_test.rs` | `self_test(Depth)` / `self_test_json`, module `maclookup` |

## Seed and cache

Load order in `start()`: the compiled-in `SRC/mac_vendors.seed.tsv`, then the cache file overlays it. The cache path is `APKAUDIO_MAC_CACHE` (every image sets it); else `$XDG_CACHE_HOME/apkaudio/mac_vendors.tsv`, else `~/.cache/apkaudio/mac_vendors.tsv`, else the temp dir — never the source checkout. Format: `OUI<TAB>vendor`, an empty vendor for "assigned to nobody", `#budget<TAB>day<TAB>spent` and `#next<TAB>unix-ms` (older readers skip `#next` as a comment).

**Many writers.** In the plugin image ~12 containers share one file on the volume `apk-audio-mac-lookup-cache`. Every access is read-merge-write under an exclusive `flock` on `<cache>.lock` (`cache.rs` `with_locked`): the file is re-read, this process's newly learned answers are merged in (a name is never replaced by a later "unassigned"), the merged table comes back into memory, and the result is written to a per-process temp file, fsynced and renamed. The daily budget (`#budget`) and the spacing / 429 back-off (`#next`) are charged in the file under the same lock, so the API's per-source-address limits are kept by the whole volume, and a worker that finds its queued OUI already answered by another container takes it without spending (`Slot::Known`). If the lock cannot be taken the process falls back to its in-memory gate. Tests: `cache.rs` `two_writers_merge_instead_of_overwriting`, `concurrent_writers_lose_nothing` (12 handles × 10 answers), `budget.rs` `the_budget_is_shared_through_the_file`, `the_spacing_is_shared_through_the_file`, `an_oui_answered_elsewhere_is_not_asked_again`.

```sh
cd PODS/POD.THICK/LIBRARY/LIB.MAC_LOOKUP/SRC && cargo test     # standalone: the one check (35 + 1 doc test)
cd PODS/POD.THICK/DISCOVERY.Engine/SRC
cargo run -p apkaudio-maclookup --bin mac-lookup -- --cached
```
