# lurien engine rebuild checklist: M2 (perf/productize) + M8 (bloat) items

These lurien items are **rebuild-gated** (each needs a Firefox engine build to land + verify).
Audited and specified no-compute on 2026-06-11; **queued for santhserver** (build is multi-hour,
must not run on the lagging desktop). Run order is grouped so ONE rebuild validates the whole batch.

Build host: santhserver (`/var/santh-cargo-target` for cargo; Firefox build uses its own obj dir).
Source: `software/browser/engine/` (the project's own repo under the Santh tree; its `camoufox-150*/obj-*`
build tree is symlinked from the off-tree build volume named by `$LURIEN_STAGING`).

## Group 1: build performance (R201–R206) · edit `assets/base.mozconfig`

Currently all commented out → a plain `--enable-release` with no optimization. Flip on and measure:

| Flag | Action | Expected | Verify |
|---|---|---|---|
| `--enable-optimize` | uncomment | runtime speed ↑ | oracle still 0-High; perf bench |
| `--enable-lto=cross` | uncomment | binary smaller, faster; build slower | size delta; launch time |
| `--enable-rust-simd` | uncomment | WebRender/encode paths ↑ | no surface change in oracle |
| `--enable-jxl` | decide vs real FF codec set | codec coherence | `codec` probe vs stock FF |
| `--enable-hardening` | evaluate | security; perf cost | oracle unchanged |

PGO (R204) is a second pass: instrument build → run a browsing profile → optimize build.
**Gate after:** re-run `lurien_gate` + `tls_fingerprint` + `creepjs_gate`: all must stay green
(perf flags must not change any fingerprint surface).

## Group 2: identity rename camoufox → lurien/firefox (R259–R261, R288) · branding

NOT a web tell (audited. UA/navigator clean), but WM_CLASS/profile/about-dialog hygiene + coherence
(real FF has WM_CLASS `firefox`). Edit:
- `additions/browser/branding/camoufox/configure.sh`: `MOZ_APP_NAME`, `MOZ_APP_BASENAME`,
  `MOZ_APP_DISPLAYNAME`, `MOZ_APP_REMOTINGNAME`: set to a neutral/real-FF-coherent value
  (decide: literal `firefox` for maximal WM_CLASS coherence, vs a distinct internal name kept
  off every user/web surface). **Recommendation:** WM_CLASS + remoting → `firefox` (coherence);
  keep an internal build id elsewhere.
- `additions/browser/branding/camoufox/locales/en-US/brand.{dtd,ftl,properties}`: brandShortName/
  brandFullName/vendorShortName/syncBrandShortName.
- `additions/browser/base/content/aboutDialog.xhtml`: remove "independent fork of Firefox for
  webscraping" wordmark + the `github.com/daijro/camoufox` link (self-incriminating if inspected).
- `assets/base.mozconfig`: `--with-app-name` / `--with-branding` if the branding dir is renamed.
- **Verify:** `lurien --version` no longer prints "Camoufox"; `xprop WM_CLASS` on the window =
  `firefox`; profile dir path; about-dialog. Then re-run the full gate suite.

## Group 3: bloat cut (R251–R257) 

- **Cut juggler** (R251/R252): drop `additions/juggler` from the build (it's flag-gated/inert in
  our BiDi usage (audited (so removing it is safe; it's Playwright's protocol we never invoke))).
  Remove its `moz.build`/`jar.mn` wiring; confirm `dist/bin/chrome/juggler/` is gone post-build.
- **R253 verify:** after cut, probe a page for any juggler global/`chrome://juggler` reachability
  (should remain absent (it already is, but prove it post-removal)).
- **R257:** delete `additions/juggler/TargetRegistry.js.bak`.
- **R254–R256:** ✅ audited: `ghostery/` is only `Disable-Onboarding-Messages.patch` (NOT an
  ad-blocker → no request/behavioral tell; keep, drop the `.bak`). `librewolf/` = build/packaging
  patches (musl, dbus_name, mozilla_dirs, `disable-data-reporting` which is *pro*-stealth, devtools-
  bypass, urlbar-interventions), build/UI only, keep; verify `devtools-bypass` + `urlbar-interventions`
  have no web-observable delta (likely UI-only). `playwright/0-playwright.patch` (135KB) IS the juggler
  source → trim with the juggler cut; `playwright/1-leak-fixes.patch` is relevant to R010 (read for the
  sourceUrlLeak/mainWorld answers). Several `.bak`/`.opt` files to clean alongside R257.
- **Verify:** binary size delta recorded to scorecard; oracle + tls + creepjs still green.

## Group 4: version coherence (R311–R314)

- Decide lurien's own version scheme (not "150.0.2-beta.25").
- Close the 133-UA-on-150-engine gap (R313): bump the shipped persona UA to the engine major OR
  document the intentional choice; add the CI coherence assertion (`guise::browser::firefox_engine_major()`
  == persona UA major).

## Single validating run (santhserver, post-rebuild)
```bash
# after the rebuild produces a new dist/bin binary, point the harnesses at it and re-baseline:
LURIEN_BIN=<new>/dist/bin/<appname> STEALTH_FIREFOX=/tmp/firefox-150/firefox DISPLAY=:10 \
  cargo test -p guise --features browser --test lurien_gate --test tls_fingerprint --test creepjs_gate -- --nocapture
# expect: oracle 0-High, JA3/JA4 still byte-identical, CreepJS still grade A, no "Camoufox" strings.
```
