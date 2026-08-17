# rebrowser-bot-detector vs reynard, applicability taxonomy (R010/R011)

`rebrowser-bot-detector` (bot-detector.rebrowser.net) is the canonical *automation-protocol*
leak suite. Its premise, stated by its own maintainers, is **Chromium driven by Puppeteer/
Playwright over CDP**. reynard is **Firefox driven over WebDriver BiDi**, so most of the suite
is *structurally* inapplicable, not merely passed. This document classifies each test. It is an
architecture analysis (no compute); rows marked **[live]** are confirmed/queued against a real
run on santhserver (R010), the rest are sound by construction.

Legend: **N/A-arch** = cannot occur given Firefox/BiDi · **APPLIES** = a real surface reynard must
handle · **✅** confirmed elsewhere in this scorecard.

| rebrowser test | What it detects | reynard verdict | Basis |
|---|---|---|---|
| `dummyFn` / Runtime.enable | CDP `Runtime.enable` exposes an isolated execution context; any page can probe it | **N/A-arch** | No CDP. BiDi `script.evaluate` uses sandbox realms, not CDP `Runtime.enable`. The leak primitive does not exist. |
| `exposeFunctionLeak` | Playwright `exposeFunction` leaves a binding on `window` | **N/A-arch** | We don't use Playwright/`exposeFunction`. Oracle confirms **0 automation globals** (`AUTOMATION_GLOBALS_JS`). ✅ |
| `sourceUrlLeak` | Puppeteer/PW inject `//# sourceURL=__puppeteer_evaluation_script__` | **APPLIES [live]** | BiDi injection has its own source labeling. Must confirm reynard's `script.evaluate` leaves no telltale sourceURL. **Queued: santhserver.** |
| `mainWorldExecution` | CDP isolated-world scripts observable from main world | **N/A-arch** | BiDi's realm isolation differs from CDP isolated worlds; no main-world bleed primitive. Confirm sandbox realm on live run. |
| `navigatorWebdriver` | `navigator.webdriver === true` under automation | **APPLIES ✅✅** | Hardcoded at the C++ level: `playwright/1-leak-fixes.patch` rewrites `Navigator::Webdriver()` to `return false` (strips the Marionette + RemoteAgent `GetRunning` checks too), so it's enforced in Gecko below any page reach. Confirmed: oracle + sannysoft + areyouheadless. |
| `pwInitScripts` / `__playwright` | Playwright init-script globals | **N/A-arch** | foxdriver/BiDi, not Playwright. 0 automation globals (oracle). ✅ |
| `chromeForTesting` / `cdc_` props | Chrome-for-Testing binary + ChromeDriver `cdc_` vars | **N/A-arch** | reynard is Firefox; `window.chrome`/`cdc_` are Chromium constructs. Oracle FP-API-coherence asserts no Chromium-only surface. ✅ |
| `useragent` consistency | UA vs platform/oscpu mismatch | **APPLIES ✅** | Persona coherence; oracle diffs UA-derived surfaces against stock FF (0 High). Languages tell fixed (R064). |
| `viewport` / window geometry | headless-default or inconsistent window dims | **APPLIES [live]** | Headful (real Xorg). Confirm window/screen coherence on the live rebrowser run; oracle already checks `screen.*`. |
| `documentElementDetails` / `pluginsLength` | headless Chromium plugin/PDF anomalies | **N/A-arch / APPLIES** | Chromium-plugin-shaped; for FF the analog is the PDF viewer + `plugins` shape, oracle covers it. |
| `webGL` vendor/renderer | headless SwiftShader renderer tell | **APPLIES ✅** | reynard spoofs WebGL via engine patch; oracle diffs `webgl.*` against stock (0 High). |

## Conclusion (calibrated)

The **entire CDP-leak class** that drives rebrowser's signal. `Runtime.enable`, isolated-world
bleed, Chrome-for-Testing, `cdc_`, Playwright init scripts: **cannot occur for a Firefox/BiDi
browser**. This is the architectural advantage the incolumitas 2026 study identified ("automation-
protocol fingerprinting dominates"): nodriver wins on the Chromium side by avoiding the Playwright
shim; reynard avoids the *entire protocol class* by not being Chromium.

What genuinely **APPLIES** and must be verified live (R010, queued for santhserver):
1. `sourceUrlLeak`: does BiDi `script.evaluate` leave an identifiable source name?
2. `mainWorldExecution`: is reynard's BiDi sandbox realm observable from the page's main world?
3. `viewport`: headful window/screen coherence under the rebrowser harness.

The rest are either N/A-arch (sound by construction) or already ✅ in this scorecard. **No "clean"
claim**: this is "not-applicable-by-architecture or confirmed-on-the-surfaces-run", dated 2026-06-11.

## Queued for santhserver (R010 live run)
```bash
# once SSH is up: drive reynard to bot-detector.rebrowser.net, capture each test's pass/leak,
# confirm the 3 APPLIES-[live] rows. Harness to be modeled on tests/creepjs_gate.rs.
```
