# Principles — LIB.MAC_LOOKUP

The rules this library keeps, each with the code that keeps it (paths under `SRC/src/`). Ecosystem rules are cited, not copied: [Refusal over fabrication](../../../../../../Documentation/%F0%9F%8F%9BPhilosophy/Doctrine/readme.Refusal%20over%20fabrication.md), [Bare Metal](../../../../../../Documentation/%F0%9F%8F%9BPhilosophy/Bare%20Metal/goldenrule.md).

← [Wiki home](../README.md)

| Principle | Kept by | Where it breaks |
|---|---|---|
| **The budget is the design.** 1000 requests a day, one per 1.1 s, 60 s back-off after a 429; cache by OUI, not MAC; cache negatives too. | `vendors/budget.rs` `DAILY_BUDGET` (line 10), `REQUEST_SPACING` (14), `THROTTLE_BACKOFF` (20); `vendors/state.rs` `Known::Unassigned` | — |
| **Never ask about an address that cannot have a vendor.** Multicast, locally administered and all-zero OUIs are refused locally. | `address/oui.rs` `Oui::parse` (line 42, refusal at line 56) | — |
| **Never block the caller.** `vendor()` answers from cache or returns `None` and queues; one worker drains the queue; the budget sleep happens with the lock released. | `vendors/lookup.rs` `vendor()` (line 16), `vendors/worker.rs` `run()` (line 16), `budget.rs` `take_slot()` (line 34) | — |
| **One gate for every caller.** The worker and `blocking_vendor` both pay through `take_slot`; the limit is per source address, not per thread. | `vendors/lookup.rs` `blocking_vendor()` (line 118), `budget.rs` | Each process has its own budget counter; the shared cache file carries `#budget`, but two processes starting together can each spend from the same day |
| **Refusal over fabrication.** No vendor is guessed; the CLI says which of three reasons applies — not a vendor-bearing address, assigned to nobody, or not reached. | `bin/mac-lookup.rs` `report()` (line 50) | — |
| **Bake in, don't look up — and never write what git tracks.** The seed is compiled in and only read; the writable cache is separate and untracked. | `vendors/cache.rs` `SEED` (line 92), `seed()`; `vendors/mod.rs` `start()` (line 37) overlays the cache on the seed; `.gitignore` | — |
| **A self test reads only what is baked in.** At `Depth::Own` the vendor-table check counts the compiled seed, so an empty cache volume is not a failure. | `self_test.rs` `the_vendor_table_loaded` (line 76) → `MacVendors::seed_len()` | — |
| **Losing the cache costs budget, not correctness.** Written with write-then-rename so a crash cannot leave a truncated cache. | `vendors/cache.rs` `persist()` (line 51, rename at 83) | — |
