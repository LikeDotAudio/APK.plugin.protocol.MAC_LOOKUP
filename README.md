# APK:lib:MAC_LOOKUP

MAC vendor lookup — the IEEE OUI registry, offline first, as a Rust library.

| | |
|---|---|
| **Recipe** | none — a library, not a container: it is linked into the binaries that use it |
| **Source** | `SRC/` (this repository — https://github.com/LikeDotAudio/APK.plugin.protocol.MAC_LOOKUP.git) — `apkaudio-maclookup`; `mac_vendors.tsv` is the bundled table; bin `mac-lookup` is the hand tool |
| **Used by** | nearly every discovery plugin (DANTE, MILAN, PTP, DNSSD, PRINTERS, NMOS, NMOS_CONTROL, APPLETV, CHROMECAST, PLAYSTATION, NETBOX, NETGEAR) and the orchestrator |
| **Workspace** | `APK:discovery/SRC/Cargo.toml` |

No bus surface and no `config.ini`. In a plugin container the cache is `APKAUDIO_MAC_CACHE` on the shared named volume `apk-audio-mac-lookup-cache` (see `APK:discovery/Docker/plugin-base.yml`), so twelve containers ask the API once. Was `APK:BareMetal/PLUGINS/PLUGIN:protocol:MAC_LOOKUP`.

---

*© 2026 — Written & Maintained by Anthony P. Kuzub. MIT Licence.*
