# 🛣 Improvements — LIB.MAC_LOOKUP (from the 2026-10-30 architecture audit)

What this repo should change to meet the standard, most valuable first. Source: [🔍Audits/ARCHITECTURE-AUDIT.md](../%F0%9F%94%8DAudits/ARCHITECTURE-AUDIT.md).
← [Roadmap](INDEX.md)

Delivered items are recorded in [🌍Publishing/DELIVERY.md](../%F0%9F%8C%8DPublishing/DELIVERY.md).

| # | Improvement | Category | Effort (S/M/L) | Evidence |
|---|---|---|---|---|
| 1 | Tag releases and move all 12 consumers from `rev = 7ba39dd…` to the tag; bump `version` on change. **Needs owner:** bump `SRC/Cargo.toml` to `0.2.0` (the multi-writer cache and the `Mac` helper API landed 2026-10-30), refresh the engine and Core lockfiles in the same change (`cargo build --locked` in the plugin image fails on a path-package version skew), push, publish tag `maclookup-v0.2.0`; then the consumer repins (handoffs filed) | Change management | S | `SRC/Cargo.toml`; consumer Cargo.toml files |
