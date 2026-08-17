# lurien first-lock prove

Host: Linux x86_64. Engine: Camoufox Firefox 150.0.2-beta.25
(`~/.local/share/lurien/lurien`). Display: private Xvfb `:10` (not the
logged-in desktop session). `MOZ_DISABLE_CONTENT_SANDBOX=1`.

| Check | Result |
|---|---|
| `cargo test -p lurien-driver --tests` | 32 passed (blank-DISPLAY unit + prior 31) |
| `lurien version` | `lurien-browser 0.1.0 / engine Camoufox Firefox 150.0.2-beta.25` |
| Headful with `DISPLAY` unset or `DISPLAY=` | exit 1: `headful lurien needs DISPLAY…` (blank no longer hangs 30s) |
| Missing engine (`HOME=/tmp/lurien-no-home`) | exit 1: `lurien engine not installed…` |
| `LURIEN_BIN=/usr/bin/true` | exit 1: `not a Firefox engine` (`--version` is not Firefox) |
| Stock path `/tmp/stock-ff-probe/firefox` | exit 1: never falls back to `/usr/bin/firefox` |
| Unreachable proxy `http://127.0.0.1:9` | exit 1: `proxy unreachable… Connection refused` before spawn |
| Reachable proxy `socks5://127.0.0.1:9050` | `lurien launch` → `launched about:blank` exit 0. No direct fallback |
| `lurien goto https://example.com` | `kind=None url=https://example.com/` |
| Managed Turnstile (`demo.turnstile.workers.dev`) | `kind=Score` after 2s settle + 8s token wait |
| MCP `initialize` / `goto` / `url` / `challenge` | stdout is JSON-RPC only (4 lines). `goto` → `kind=None`. `challenge` is `isError:true`. stderr empty |
| Playwright `firefox.launch({executable_path})` | Python 1.60 async. Title `Example Domain`, url `https://example.com/` |
| `lurien as --profile <tmp>` | `imported cookies=true logins=true storage=true` |
| Profile import (unit) | cookies+logins round-trip; missing `key4.db` warns; corrupt sqlite errors |
| `lurien_gate` vs stock FF-150 | PASS. 185/194 agree. High leftovers are PersonaIntended or lurien-cleaner (`webdriver=false`, trust 80>35). Oracle: stock Firefox 150 at `$LURIEN_STAGING/firefox-150/firefox/firefox` |
| `live_detector_suite` vs stock FF-150 | 3/3 after applying `lurien_gate`'s leftover High table to G254. 5 High leftovers: timezone PersonaIntended, `webdriver=false`, trust 80>35, worker-realm self-coherent, no automation globals. Same leftovers `lurien_gate` already accepted |
| Idle RSS | 351916 KiB (351 MiB) vs no stock FF-150 RSS compare this turn |
| `cargo tree -p lurien-driver` | no scanclient / wafrift / ahura / captchaforge |
| crates.io captchaforge | live=`0.2.41` shim; 36 prior yanked. `guise` 0.1.6 not yanked |
| Consumer path deps | loginflow, scanclient, wafrift-captchaforge-bridge `cargo check --offline` OK against `software/browser/{guise,foxdriver}`. No leftover `libs/runtime/guise` path. jsdet workspace blocked by foreign `faultkit ^0.1` vs `0.2.2`, not a guise path |
| Ahura | no guise/foxdriver/lurien Cargo dep. `AHURA_GUISE_BRIDGE_URL` HTTP face kept one release |
| Engine CI leftover | `engine/.github/workflows/build.yml` still uploads `CamoufoxBuilds-*` for linux/windows/macos. v1 does not host an engine tarball. `software/browser/.github/workflows/engine.yml` is not shipped |

`lurien_gate` and `live_detector_suite` are green on this host. Do not treat a skipped run as green.
