# 🔍 Architecture audit — LIB.MAC_LOOKUP (2026-10-30)

Audited against the APK.audio multi-repo architecture standard: coupling, boundaries, duplication, contracts.
← [Audits](INDEX.md) · Improvements: [🛣Roadmap/IMPROVEMENTS.md](../%F0%9F%9B%A3Roadmap/IMPROVEMENTS.md)

## Bounded context
MAC/OUI parsing and "who made this device" lookup: address parsing (`address/`), a rate-budgeted client for api.macvendors.com with a TSV cache (`vendors/`), and the `mac-lookup` CLI. Rust crate `apkaudio-maclookup`, ~1,450 lines. Sampled: all of `SRC/`.

## Context map
| Talks to | How (topic / endpoint / file / import) | Relationship |
|---|---|---|
| 12 plugin crates (PING, DANTE, DNSSD, PTP, NMOS, …) | Rust git dependency pinned `rev = "7ba39dd…"`; `[patch]`ed to `members/LIB.MAC_LOOKUP` in the DISCOVERY.Engine workspace | Shared Kernel (supplier) |
| APK.plugin.SDK `apkaudio-contracts` (tag `sdk-v0.5`) | `self_test::{Check, Depth, Report}`, `topics::is_segment` | Conformist |
| api.macvendors.com | one HTTPS GET via `ureq` in `SRC/src/vendors/fetch.rs` | Anti-Corruption Layer (this crate is it) |
| Volume `apk-audio-mac-lookup-cache` | TSV file at `$APKAUDIO_MAC_CACHE`, mounted into every plugin container by `DISCOVERY.Engine/Docker/plugin-base.yml:60-67`; read-merge-write under `flock` (delivered 2026-10-30) | Shared mutable state, coordinated |

## Scorecard
| Category | Score (0–3) | Finding |
|---|---|---|
| Coupling & Cohesion | 2 | One cache file on one shared volume is still shared state, but every write is now a locked read-merge-write and the daily budget/spacing live in the file (delivered — see DELIVERY). |
| Boundary Integrity | 3 | The vendor API is fully behind `fetch.rs` (39 lines) and the `Fetched` enum; callers see `vendor(mac)` / `label(mac)` only. |
| Duplication Control | 2 | `Mac::parse_any`/`from_octets`/`octets`/`Display`/`lower` now cover every plugin copy; PING, DANTE and MILAN still have to delete theirs (handoffs filed). |
| Testing & Contracts | 3 | 35 unit tests (incl. two-writer, 12-writer, shared-budget and shared-spacing tests) + 1 doc test, run standalone by `cargo test` in `SRC/`; SDK self-test at `Depth::Own`. |
| Change management | 1 | Version 0.1.0, no tags; consumers pin a raw commit (`7ba39dd`) that is not HEAD (`9e1b3ef`), so the workspace build (path patch) and a standalone plugin build compile different code. |

Score key: 3 meets the standard · 2 minor gaps · 1 significant gaps · 0 absent / violated.

## Findings
1. **Good — vendor API is behind a real adapter with a hard budget.** `SRC/src/vendors/fetch.rs:17-38` is the only HTTP call; `budget.rs:10-20` caps it at 1000/day, 1.1 s spacing, 60 s back-off on 429. 404 is cached as `Unassigned` so it is never asked again (`state.rs:9-16`).
2. **Good — address parsing refuses non-identities locally.** `SRC/src/address/` rejects multicast, locally-administered and zero MACs before any lookup, which supports the house rule "identity = serial/MAC".
3. **Remaining: duplicate MAC helpers in consumers.** `PLUGIN.PING/SRC/src/arp.rs:40-47`, `PLUGIN.DANTE` (`transport/local_mac.rs`, `controller/registry.rs`, `protocol/conmon/identity.rs`) and `PLUGIN.MILAN/SRC/src/adp.rs` keep their own `parse_mac`/`format_mac`; this crate's side (`Mac::parse_any` and friends) is delivered and handoffs are filed.
4. **Unversioned releases (needs owner).** `SRC/Cargo.toml` is `0.1.0`; the repo has no tags; 12 consumers pin `rev = "7ba39dd…"` while HEAD is `9e1b3ef`. The workspace `[patch]` hides the difference in the image build.
