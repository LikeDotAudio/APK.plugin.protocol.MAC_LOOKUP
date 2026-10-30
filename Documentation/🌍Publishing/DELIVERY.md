# Delivery — LIB.MAC_LOOKUP

← [Wiki home](../README.md)

A library, so it ships inside the binaries that link it — thirteen plugin crates (APPLETV, CHROMECAST, DANTE, DNSSD, MILAN, NETBOX, NETGEAR, NMOS, NMOS_CONTROL, PING, PLAYSTATION, PRINTERS, PTP), all in `apk-plugins:local`, built by `DISCOVERY.Engine/Docker/Dockerfile` (which copies `LIBRARY/` whole, so the seed is there for `include_str!`). `NODE.BareMetal` and `DATABUS.Broker.MQTT` Dockerfiles also copy `SRC/` as a workspace member.

At run time the plugin image sets `APKAUDIO_MAC_CACHE=/var/lib/apkaudio/mac/oui_cache.txt` (`DISCOVERY.Engine/Docker/Dockerfile` line 168) and `Docker/plugin-base.yml` mounts the named volume `apk-audio-mac-lookup-cache` there, so every plugin shares one cache.

Rebuild after a change: `PODS/POD.manager/MANAGER.Docktor/K8/up-stack.sh DISCOVERY.Engine`.

Test: `cd PODS/POD.THICK/DISCOVERY.Engine/SRC && cargo test -p apkaudio-maclookup` (26 pass). Smoke: `python3 .apk.scripts/smoke.py PODS/POD.THICK/LIBRARY/LIB.MAC_LOOKUP` (root; repo pushed) — [SMOKE.md](../%F0%9F%A7%A9Manual%20and%20Artifacts/SMOKE.md).

**Not automated:** no CI and no gate runs the tests; the plugin image's `--keep-going` would hide a compile failure behind stub binaries. The seed is grown by hand. No live URL.
