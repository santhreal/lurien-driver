# Lurien product spec

One installable browser. The word is **lurien** everywhere a human types it.
The browser itself is **lurien-browser**; the Rust crate that drives it is
**lurien-driver**. The browser owns the browser name.

There is no public or internal product named reynard after cutover.
guise, foxdriver, guise-bridge, and captchaforge stop being products.
guise crate *names* stay (`guise`, `guise-profiles`, `guise-pacing`, `guise-choice`, `guise-oracle`). Every guise crate *path* moves into `software/browser/`. `libs/runtime/` holds none of them after cutover.

This file is the plan.
Do not create `santhreal/reynard`, do not ship `REYNARD_*`, do not leave `software/reynard/` as a path.

## 0. Shipped, 2026-08-16

| Item | State |
|---|---|
| `santhreal/lurien-driver` | the control tree: driver crate, persona crates, catalog, docs |
| `santhreal/lurien-browser` | the Gecko fork, renamed off `reynard` |
| crates.io `guise` family | unified at `0.1.8` |
| crates.io `captchaforge` | 36 versions yanked; `0.2.41` stays as the retirement notice |
| challenge subsystem | `engine/additions/challenge/`, packaged, proven by `lurien/tests/e2e_challenge.sh` |
| claimed kinds | `none`, `score`, `checkbox`, `pow`, `slider`, each with a dated scorecard row |
| `pow` | solved in the browser: `Pow.sys.mjs` plus `PowWorker.js` lanes, no helper, proven by `lurien/tests/e2e_pow.sh` |
| `slider` | measured by `lurien-vision` from the widget's own snapshot, dragged along a sampled profile, proven by `lurien/tests/e2e_slider.sh` |
| the visit | `Prelude.sys.mjs` reads the page in the top document before any act, from a plan `guise` sampled, proven by `lurien/tests/e2e_prelude.sh` |
| dynamics per interaction | the driver ships a deck of sampled paths and a seed, `Dynamics.sys.mjs` deals one per interaction and records it, proven by `lurien/tests/e2e_dynamics.sh` |
| evidence a version apart | every row carries `v`, the driver refuses a row it does not read and names both versions, held equal across the two repositories by `lurien/tests/engine_package.rs` |
| a budget per kind | `kind_budget_ms` bounds a kind by its own work, counted from after the page was read, and the refusal names the budget it was given, proven by `lurien/tests/e2e_budget.sh` |
| a helper only this session can use | the helper protocol is versioned and every line carries a per-session token compared in constant time, documented in `docs/HELPERS.md`, proven by `lurien/tests/e2e_slider.sh` |
| two challenges on one page | classification and reduction are ordered by kind severity, the first sighting opens a settle window so a widget that attaches after paint is seen, and every widget the page held is reported, proven by `lurien/tests/e2e_classify.sh` |
| a token anywhere the vendor puts it | fields, cookies, a storage key in the widget's own origin, or a dotted path into a posted message; the verdict names the channel, and `goto` waits for as long as the budgets it granted, proven by `lurien/tests/e2e_token_channels.sh` |
| a typed answer with a rhythm | a gap per pair class and a hold per character class sampled from the persona's typing model, dealt one entry per keystroke and classified per digraph in the browser, proven by `lurien/tests/e2e_keys.sh` |
| a proof per claimed kind | a fixture and a runnable script for every kind the build claims, plus one adversarial page that refuses an untrusted click, a press with no approach, a forged token and a readable origin boundary, proven by `lurien/tests/e2e_score.sh` and `lurien/tests/e2e_adversarial.sh` |
| selectors and the wait | `role:`/`text:`/`label:`/`placeholder:`/`testid:` or CSS, one resolver, acts wait for their element, ambiguity is refused with candidates, proven by `lurien/tests/e2e_locator.sh` |
| the agent's page | `snapshot` answers with roles, names and handles; `ref:eN` acts, and a handle whose node changed is refused, proven by `lurien/tests/e2e_snapshot.sh` |
| one call, several verbs | `batch` runs a step list, validates it before running any of it, stops at the first failure and says how far it got, on all three faces, proven by `lurien/tests/e2e_batch.sh` |

`lurien-driver` is not published while `visual` and `audio` are still refused.

## 1. Name

| Face | Token |
|---|---|
| Spoken / CLI / MCP | `lurien`, `lurien-mcp` |
| The browser | `lurien-browser`, GitHub `santhreal/lurien-browser`, MPL, own repo |
| The driver | `lurien-driver`, GitHub `santhreal/lurien-driver`, crates.io `lurien-driver` |
| Installed browser binary | `lurien` |
| crates.io `lurien` | a dead 2021 config-file crate, not us, not available |
| crates.io `lurien-browser` | held for the crate that installs the browser, if that is ever built |
| npm (later) | `@santhreal/lurien-mcp` |
| Browser env | `LURIEN_BIN`, `LURIEN_CONFIG`, `LURIEN_CHALLENGE` |
| Persona crates | `guise`, `guise-profiles`, `guise-pacing`, `guise-choice`, `guise-oracle` |

A crate that cannot paint a page is not the browser. `lurien-driver` requires an
installed `lurien-browser` and says so when it is missing.

Do not publish `santhreal/reynard`, `reynard.dev`, `reynard-mcp`, or a Playwright Python package.
[minh-ton/reynard-browser](https://github.com/minh-ton/reynard-browser) is a 1.5k-star Gecko iOS browser.

Do not publish a crate named `guise` as the browser. [santhreal/guise](https://github.com/santhreal/guise) is the persona lib (32k downloads).

Do not print `lurien.dev` until the domain is owned. Install URL is GitHub raw or `santh.dev`.

## 2. Goal

A coding-agent user who already drives Playwright can install lurien, then either keep their Playwright script (launch uses the lurien-installed engine) or add:

```json
{ "mcpServers": { "playwright": { "command": "lurien-mcp" } } }
```

Same tool names as `@playwright/mcp`. The MCP description is the only skill.
Captchas that are really scores (managed Cloudflare) pass because the persona holds.
Hard captchas are solved inside the engine, not by a sidecar crate.

v1 does **not** claim “solves all captchas.” It claims: Playwright drop-in, no debug strip, import a real Firefox profile, managed CF holds, `lurien_gate` green on a real Linux host.

## 3. Organization

```
PUBLIC     lurien              CLI, MCP, installed engine binary
           lurien-browser      crates.io + GitHub
INTERNAL   guise + guise-*     persona crates, under software/browser/
           foxdriver           BiDi Page (Rust, ours)
           serve               HTTP face. Died into `lurien serve`; no separate daemon
DEAD       captchaforge        yank after shim
           reynard             every token, path, env, test, script
```

```
software/browser/
  README.md
  install.sh

  lurien/                         crate lurien-driver. The only public Rust face.
    Cargo.toml                    [[bin]] lurien, lurien-mcp
    src/
      lib.rs                      lurien::Browser
      launch.rs                   coherence → resolve → spawn → BiDi poll → Page
      resolve.rs                  LURIEN_BIN → Result. No Firefox fallback
      profile_import.rs           cookies.sqlite, logins.json+key4.db, storage
      goto.rs                     navigate + Challenge wait + named fail
      as_profile.rs               `as` — switch/import a real Firefox profile
      version.rs                  crate semver + engine --version
      error.rs                    every §6.2 failure is a typed variant
    bins/
      lurien.rs                   Playwright verbs + as
      lurien-mcp.rs               stdio, Playwright-MCP names, no `challenge` tool
    tests/
      resolve.rs                  missing bin → Err (mutation: Option fallback)
      error_registry.rs           every launcher hits resolve()

  engine/                         today's software/reynard/          MPL-2.0
    patches/
      banner-no-visual-cue.patch  skip gRemoteControl.updateVisualCue
      lurien-config.patch         MaskConfig reads LURIEN_CONFIG first
      challenge-register.patch    register the observer + load the catalog
    additions/challenge/          primitives. No vendor names in C++.
      Observer.{h,cpp}            attach nsIWebProgressListener to every BC
      Catalog.{h,cpp}             load kinds/*.toml; fail closed on unknown kind
      Classify.{h,cpp}            chrome signals → (kind, target, token spec)
      Input.{h,cpp}               trusted pointer / key / drag on a named BC
      Token.{h,cpp}               watch hidden-input / postMessage / cookie write
      Snapshot.{h,cpp}            compositor grab of a named BC
      HelperSock.{h,cpp}          local socket to the vision and audio helpers
    settings/                     camoufox.cfg → lurien.cfg
    scripts/                      make / package / install-local-build
    pythonlib/                    upstream Camoufox tests. Not a product
    tests/                        camoufox Playwright suite. Keep

  guise/                          persona compiler + launch
    src/
      browser/
        lurien.rs                 today's reynard.rs (resolve, config JSON, spawn)
        inject.rs                 JS stealth. Stock-Firefox path only
        userjs.rs                 prefs
        session_age.rs
        mod.rs                    launch orchestration + family gate
      fingerprint/                identity, TLS/JA3/JA4, WebGL, fonts, bundle
      human/                      mouse, keystroke, scroll, timing (single owner)
      http/                       headers, wire_emit, session_coherence
      probe/                      oracle + catalogue (gates, not product)
      persona_pool.rs rotation.rs config.rs
  guise-profiles/                 UA / profile ids
  guise-pacing/                   backoff / jitter
  guise-choice/                   sampling
  guise-oracle/                   surface taxonomy

  foxdriver/                      BiDi Page. Do not fold into guise.
    src/
      lib.rs
      browser.rs                  launch, Page, frames, performActions
      network.rs                  request / cookie / auth
      frame.rs frame_graph.rs     OOPIF tree
      cookies.rs dialog.rs sensors.rs

                                  (the HTTP daemon is now `lurien serve`)
  echo/                           guise-echo. Test reflector only

  captcha/                        catalog + helpers. Not a product.
    kinds/                        one TOML per vendor binding
      _schema.toml                closed kind enum + required fields
      turnstile.toml              signals → kind=score|checkbox, token field
      recaptcha.toml
      hcaptcha.toml
      arkose.toml                 kind=visual (or new kind + fixture, not C++)
      geetest.toml
      datadome.toml
      akamai.toml
      integrity.toml              always-block sitekeys
    vision/                       lurien-vision: slider measurement now, grid classification later. Not in libxul
    audio/                        helper process. STT

  docs/
    README.md                     what it is, install, honest leaks
    ENGINE.md                     build, rebase, add a patch
    KINDS.md                      add a vendor / add a kind
    TREE.md                       every folder's owner, allowed imports
    REBASE.md                     Camoufox/Firefox rebase runbook
    NOTICE                        MPL + Camoufox + Firefox

libs/runtime/                     no guise*, no foxdriver, no echo
```

Do not fold foxdriver into guise.
Do not put ONNX / VLM / Whisper in Gecko. Vision is a helper process.
Do not put a vendor name in `engine/additions/challenge/`.
Scanner path deps follow `software/browser/guise-*`. Crate names stay.

Public crate license: MIT OR Apache-2.0.
Engine: MPL-2.0, separate process. Never link libxul into the MIT crate.
Observer primitives under `engine/additions/challenge/` are MPL. Accepted.
Kind TOML is MIT. A new vendor is a TOML, not a patch.

## 4. Complete rename

Every `reynard` token becomes `lurien`. No leftover path, env, symbol, or test name after cutover.

### 4.1 Engine tree

| Today | After |
|---|---|
| `software/reynard/` | `software/browser/engine/` |
| `software/reynard/REYNARD.md` | fold into `software/browser/README.md` + `ENGINE.md` (fork notes only) |
| `software/reynard/scripts/publish-reynard.sh` | `software/browser/scripts/publish-engine.sh` |
| `software/reynard/pythonlib/` | stays under `engine/pythonlib/` as Camoufox test/tooling. Not a lurien product |
| aboutDialog / branding `camoufox` | lurien (v1 can keep Camoufox chrome strings if a patch is not ready; installed name is still `lurien`) |
| build product `camoufox` in `obj-*/dist/bin/` | `install.sh` copies/symlinks it to `lurien`. Native binary rename is a later branding patch |

Do not create `santhreal/reynard`.

### 4.2 Env and install paths

| Today | After |
|---|---|
| `REYNARD_BIN` | `LURIEN_BIN` |
| `GUISE_REYNARD_BIN` | `LURIEN_BIN` (one name) |
| `REYNARD_CONFIG` / `REYNARD_CONFIG_N` | `LURIEN_CONFIG` / `LURIEN_CONFIG_N` |
| `~/.local/share/reynard/reynard` | `~/.local/share/lurien/lurien` |
| `~/.cache/reynard/reynard` | `~/.cache/lurien/lurien` |
| `/opt/reynard/reynard` | `/opt/lurien/lurien` |
| `scripts/install-reynard.sh` | `software/browser/install.sh` |
| `$REYNARD_STAGING` | `$LURIEN_STAGING` (a host path the caller supplies; never hardcoded in the product) |

`MaskConfig.hpp` read order after the patch:

1. `LURIEN_CONFIG[_<n>]`
2. `REYNARD_CONFIG[_<n>]` (one release, then delete)
3. `CAMOU_CONFIG[_<n>]` (upstream Camoufox; keep)

Resolver (`resolve_lurien_bin`) after cutover:

1. `LURIEN_BIN`
2. `REYNARD_BIN` / `GUISE_REYNARD_BIN` (one release, then delete)
3. `~/.local/share/lurien/lurien`, `~/.cache/lurien/lurien`, `/opt/lurien/lurien`
4. old `~/.local/share/reynard/reynard` (one release, then delete)

Missing binary is `Err`. Never stock Firefox. The current `Option` + loud warning is the bug v1 closes.

### 4.3 Rust symbols

| Today | After |
|---|---|
| `guise/src/browser/reynard.rs` | `guise/src/browser/lurien.rs` (or `engine.rs`) |
| `REYNARD_CONFIG_ENV` | `LURIEN_CONFIG_ENV` |
| `reynard_config` / `reynard_config_env` | `lurien_config` / `lurien_config_env` |
| `resolve_reynard_bin` | `resolve_lurien_bin` → `Result<PathBuf, Error>` |
| `launch_reynard` / `launch_reynard_with_config` | public `lurien::Browser::launch`; internal `launch_with_config` |
| `firefox_engine_major` | keep (describes Gecko, not the brand) |
| `tests/reynard_gate.rs` | `tests/lurien_gate.rs` (`[[test]] name = "lurien_gate"`) |
| `tests/reynard_mouse_model_ownership.rs` | `tests/lurien_mouse_model_ownership.rs` |
| `tests/reynard_canvas_audio_farble_live.rs` | `tests/lurien_canvas_audio_farble_live.rs` |
| `tests/reynard_window_geometry*` | `tests/lurien_window_geometry*` |
| `e2e_reynard_bridge.sh` | `e2e_lurien_bridge.sh` |
| health `browser_engine == "reynard"` | `"lurien"` |
| rustenium timeout comment “Camoufox/reynard” | “lurien” |

`launch_profiled_firefox` and `foxdriver::drive_browser` stay test-only (stock-vs-lurien oracle). Not on the public path.

### 4.4 Docs and plans

| Today | After |
|---|---|
| `software/browser/README.md` | product face (this folder) |
| guise CHANGELOG historical “reynard” | leave (history). New text says lurien |
| MASTER_PLAN `02_stealth.md` / `02_guise.md` | leave. CLAIMS.md marks DONE |

After cutover, grep of `software/browser/` for `\breynard\b` and `REYNARD_` must be empty except the one-release alias table and `CAMOU_CONFIG` comments. Grep of `libs/runtime/` for `guise` / `foxdriver` / `guise-echo` must be empty.

## 5. Public API

```
lurien::Browser::launch(profile) -> Page
```

Engine binary required. Missing binary is an error.

MCP / CLI verbs = Playwright-MCP: `goto snapshot click type fill screenshot cookies url scroll wait frames`. Plus `as` (import/switch profile). Captcha is automatic. No `challenge` tool.

Node v1 is `firefox.launch({ executablePath })`. No Node package unless we decide later.
npm MCP is later. v1 is a binary on PATH.

Default is headful. Headless is a documented weaker mode, not the demo.

v1 OS: Linux x86_64 only. macOS / Windows / aarch64 are not v1.

## 6. Launch contract

This is the product. Every path a user or agent hits must fail loud with a next action.

### 6.1 Success

```
install.sh
  → ~/.local/share/lurien/lurien exists and is executable
  → lurien --version prints the Gecko/Camoufox version
lurien::Browser::launch(profile)
  → resolve LURIEN_BIN
  → enforce persona coherence (same primitive as today)
  → Firefox-family gate (refuse Chrome/Safari persona)
  → align UA major to engine --version
  → write LURIEN_CONFIG + wrapper script in a unique temp dir
  → spawn, poll BiDi port until accept (no fixed sleep)
  → apply session-age seed
  → return Page
goto url
  → NSS handshake (real Firefox)
  → Catalog.classify: none | score | checkbox | visual | slider | audio | pow | fail
  → document usable. No auto_solve. No challenge tool.
```

Concurrent launches get unique temp dirs (already true). Keep that.

### 6.2 Failures (v1 must name each)

| Failure | Behavior |
|---|---|
| No `LURIEN_BIN` and no install path | Error: “lurien engine not installed. Run install.sh or set LURIEN_BIN.” Never spawn `/usr/bin/firefox`. |
| Binary not executable / not a Firefox | Error with path and `file(1)` hint. |
| `DISPLAY` unset and headful | Error: “headful lurien needs DISPLAY. Start Xvfb or export DISPLAY. Headless is weaker; pass headless=true only if you accept that.” |
| BiDi port never accepts | Error after bounded poll (not rustenium’s 500 ms race). Include elapsed + last errno. |
| rustenium `session.new` timeout | Keep the raised timeout for cold lurien. Error names the timeout and how to raise it. |
| Persona incoherent | Error from `enforce_persona_launch_coherence`. No launch. |
| Non-Firefox persona | Error from the engine-family gate. |
| Cross-OS persona on Linux (Windows/macOS fonts/WebGL) | Error or hard warning that blocks v1 default. Do not silently ship a lying canvas. |
| Proxy configured but unreachable | Error on first connect. Do not fall back to direct (host TTL leak). |
| No proxy (proxyless) | Launch allowed. README + launch log: host TCP/TTL still Linux. Not a silent claim of “any geo.” |
| Profile dir locked (another Firefox) | Error: path + “close the other Firefox or pick a new profile_dir.” |
| Profile import: missing `logins.json` / `key4.db` | Import cookies + localStorage; warn that logins were skipped. Do not invent passwords. |
| Profile import: corrupt `cookies.sqlite` | Error. Do not start with a half-copied profile. |
| Remote-control banner still painted | v1 gate fail. Banner patch is required. |
| `navigator.webdriver === true` | v1 gate fail. |
| Managed CF / Turnstile score-class fails | Fail the `goto`. Do not retry with a different persona unless the caller asked. |
| Kind the build does not claim | Refuse with the kind name and the reason: not claimed by this build. Never report it as a pass, never call a third-party solver. Claimed kinds are the ones with a dated scorecard row: `none`, `score`, `checkbox`. |
| Crash / SIGSEGV of the engine | Error with the wrapper log path. Do not restart in a loop. |
| OOM | Error. Publish the idle RSS number so this is diagnosable. |
| MCP client sends `challenge` | Unknown tool. Description already says captcha is automatic. |
| `lurien-mcp` started with no engine | Same missing-binary error on stdio, then exit 1. |

Tests that cannot see a display or binary **skip-loud** (print why, exit 0). The **product** never skips.

### 6.3 What stays hidden

Stock Firefox launch and `foxdriver::drive_browser` exist only for `lurien_gate` (patched vs stock).
A user of `lurien` never hits them.

## 7. Install

```
curl -fsSL https://santh.dev/lurien/install.sh | sh
# or
software/browser/install.sh [/path/to/built/camoufox]
```

`install.sh` does:

1. Resolve a built engine (`$1`, else `LURIEN_BIN`, else newest `software/browser/engine` / staging `camoufox-*/obj-*/dist/bin/camoufox`).
2. `mkdir -p ~/.local/share/lurien` and symlink/copy to `~/.local/share/lurien/lurien`.
3. Put `lurien` and `lurien-mcp` on PATH (`~/.local/bin`).
4. Print `lurien --version`.
5. If nothing found: exit 1 with how to build (`make dir && make build` in `engine/`) or where to set `LURIEN_BIN`.

v1 does not download a multi-GB nightly from a CDN unless we actually host one. If we do not host one, install.sh is “wire a local build” and the README says so. Do not pretend `curl | sh` fetches an engine we have not published.

Playwright (Python or Node) uses `executablePath` / `firefox.launch({ executablePath })` pointed at `~/.local/share/lurien/lurien`. No `pip install`, no `from lurien.sync_api`. That is Camoufox’s product, not ours.

## 8. Faces

- `lurien` CLI — Playwright verbs + `as`
- `lurien-mcp` — stdio, Playwright-MCP tool names, long description is the skill
- Rust: `lurien::Browser::launch` via crate `lurien-browser`
- Any Playwright language: `executablePath` to the installed engine

Ahura keeps `AHURA_GUISE_BRIDGE_URL` for one release, then talks to `lurien-mcp` like any other agent. Hunt tools stay in Ahura.

## 9. The solver (this is the product)

captchaforge is a **page sidecar**. It `evaluate`s detect JS, guesses the
Turnstile checkbox at parent-rect + (28, 32), clicks through BiDi, polls a
hidden input. That is CapSolver with extra steps. Cloudflare's widget is a
cross-origin OOPIF. Page JS cannot see it. Hardcoded offsets rot. The 3x…FF
test key is a dead Firefox oracle (no Firefox builds that iframe). Third-party
HTTP solvers (2captcha / CapSolver) leak the session and the timing.

The best solver is the browser that paints the widget.

### 9.1 Why in-Gecko wins

| Path | What it can see | What it can click | Tell |
|---|---|---|---|
| Page JS / `el.click()` | same-origin DOM | `isTrusted=false` | scored immediately |
| Sidecar BiDi (`performActions` from parent) | parent rect, guessed offset | trusted, wrong frame often | geometry rot; extra BiDi round-trips |
| CapSolver / 2captcha | a screenshot, later | a token from a third party | token source + RTT |
| **Catalog + Observer** | every BrowsingContext attach, including OOPIF / closed shadow | `Input` on the **child** BC, same `EventStateManager` as a real click | none beyond a real user |

We already own the three pieces nobody else ships together: a patched Gecko,
a single persona seed (TLS + UA + mouse), and a BiDi driver. The innovation
is using the chrome-privileged process as the solver, not wrapping another
API around the page.

### 9.2 Pipeline (every `goto`)

```
goto(url)
  NSS handshake (real Firefox, guise persona)
  Observer attaches to every BrowsingContext
  Catalog.classify(chrome signals) → (kind, target, token)
    none      → document usable
    score     → Token.wait (vendor write). Managed CF already does this
    any act   → Prelude(top BC): settle, wander, wheel session, dwell, then the kind
    checkbox  → Input.click(child BC, guise trajectory, catalog target)
    visual    → Snapshot(child BC) → helper → Input on same BC
    slider    → Snapshot → helper axis → Input.drag on same BC
    audio     → media capture → helper STT → Input.type on same BC
    pow       → ChromeWorker lanes in the browser → submit via the [work] address
    fail      → typed error. Never CapSolver. Never a fabricated token
    unknown   → fail closed. Adding a kind without a fixture is a red test
```

No `challenge` MCP tool. Captcha is a property of `goto`.

### 9.3 Extensible architecture (catalog + primitives)

A vendor is not a C++ file. A vendor is a TOML that binds chrome-visible
signals to a **kind**. Kinds are a closed enum. The engine implements kinds,
not Cloudflare.

**Closed kinds** (fail closed on a new member with no fixture):

| Kind | Primitive | Success |
|---|---|---|
| `none` | — | document usable |
| `score` | Token.wait | vendor wrote the named field / cookie |
| `checkbox` | Input.click on catalog target in child BC | Token.wait |
| `visual` | Snapshot → helper → Input.click cells / type | Token.wait |
| `slider` | Snapshot → helper axis → Input.drag | Token.wait |
| `audio` | media → helper STT → Input.type | Token.wait |
| `pow` | worker lanes in the browser → field, callback, or navigation | Token.wait |
| `fail` | — | typed error |

Adding **Turnstile / hCaptcha / reCAPTCHA / Arkose / Geetest / DataDome /
Akamai** is a new `captcha/kinds/<vendor>.toml` that names:

- signals (iframe `src` host/path, custom element, cookie, challenge URL)
- kind (one of the table)
- target (chrome-visible: role, selector, or “first checkbox in this BC”)
- token (hidden input name, `postMessage` shape, cookie name)
- integrity (always-block sitekeys that must refuse)

Adding a **new kind** (a 3D game, a novel drag) is:

1. Add the name to `_schema.toml`.
2. Implement the primitive once in `Input` / `Snapshot` / `HelperSock`.
3. One fixture that fails until the kind is wired.
4. One live-vendor row before the README may name it.

The registry test enumerates `_schema.toml` at run time. A kind with no
fixture is red. A vendor TOML that names an unknown kind is red. A C++
file under `additions/challenge/` whose identifier matches a vendor
(`[Tt]urnstile|[Rr]ecaptcha|[Hh]captcha|[Aa]rkose`) is red.

**Engine primitives** (fixed set, MPL, no vendor strings):

| File | Job |
|---|---|
| `Observer` | attach to every BC; feed Classify |
| `Catalog` | parse kinds/*.toml; reject unknown kind / missing field |
| `Classify` | chrome signals → (kind, target, token spec) |
| `Input` | trusted pointer / key / drag on a **named** BC |
| `Prelude` | the visit before the act: settle, pointer path, wheel session, dwell, in the **top** BC |
| `Token` | success = vendor write, never our JS string |
| `Snapshot` | compositor grab of a named BC (not parent + black iframe) |
| `HelperSock` | bytes to the vision and audio helpers; answer back; helper never sees the page |

Trajectory for `Input` and the reading cadence for `Prelude` come from
`guise::human::{mouse, scroll}`. Native `MouseTrajectories.hpp` stays deleted.

v1 ships Catalog + Token for `score` (and `none` / `fail`). Managed CF
holds because `goto` waits on the token hook, not `auto_solve`.
`checkbox`, `pow` and `slider` are claimed on fixture rows that reproduce what a
live widget checks: trusted events only, per-load randomness, and a refusal for
the shape a scripted solver produces. `visual` and `audio` land per kind when a
row exists for them.

### 9.4 What we do not do

- Do not keep `TurnstileInteractiveSolver`'s (28, 32) offset.
- Do not write `TurnstileObserver.cpp`. Vendor = TOML.
- Do not call 2captcha / CapSolver / any HTTP solver. Delete `third_party.rs`.
- Do not `apply_stealth` inside the solver (double-spoof).
- Do not treat CF `3x…FF` as a product gate. It is a dead Firefox oracle.
- Do not put ONNX in libxul.
- Do not claim a kind on the homepage until it has a live scorecard row.

### 9.5 Fold / yank (packaging only)

`software/captchaforge` is a second workspace. After Catalog + Token for
`score` work, the directory is deleted. crates.io: 36 live versions,
0 yanked, 88855 downloads, newest `0.2.40`. You cannot delete a crates.io crate.

1. Retarget in-tree consumers. wafrift lock is `captchaforge 0.2.38`.
   loginflow `captchaforge` feature dies.
2. Last publish is a deprecation shim pointing at lurien.
3. `cargo yank captchaforge@$v` for every live version:
   `0.2.40 0.2.39 0.2.38 0.2.36 0.2.35 0.2.34 0.2.33 0.2.32 0.2.31 0.2.30 0.2.29 0.2.28 0.2.27 0.2.26 0.2.25 0.2.21 0.2.20 0.2.19 0.2.18 0.2.17 0.2.16 0.2.15 0.2.14 0.2.13 0.2.12 0.2.11 0.2.10 0.2.9 0.2.8 0.2.7 0.2.6 0.2.5 0.2.4 0.2.1 0.2.0 0.1.0`
   Confirm `yanked=true` on all 36.
4. Do not yank `guise` or any `guise-*`.

Keep from captchaforge: vendor selectors as kind TOML, integrity fixtures,
vision/audio as **processes**. Delete: CLI, `serve`, helm, GHCR, 5-arch bins,
third-party solvers, Chromium/CDP leftovers, `auto_solve` as a public API.

### 9.6 Engine patches

v1 (must land before prove):

1. Banner: skip `gRemoteControl.updateVisualCue()`.
2. `MaskConfig.hpp` reads `LURIEN_CONFIG` first.
3. No silent Firefox fallback in any launcher.
4. Catalog + Token for `score`, so `goto` does not call a sidecar.

v1.1 (per kind, measured on a live page):

5. `checkbox` via Input on the child BC.
6. `visual` / `slider` / `audio` / `pow` via Snapshot + HelperSock.

Known leaks that stay in the README: matched-host Linux Firefox only;
cross-OS fonts/WebGL/WebGPU; inert `canvas:seed`; proxyless TTL.

## 11. Prove (before any `git mv`)

On this host, with `LURIEN_BIN` + `DISPLAY`:

- `lurien_gate` green (High/Critical identical to stock FF-150)
- `live_detector_suite` green
- Managed Turnstile: `goto` waits on the token hook; no `auto_solve` call
- Banner absent in a headful screenshot
- Missing binary: launch returns the hard error (unit-test the resolver)
- One idle RSS number vs stock FF-150, written to `bench-results/`

Gates skip-loud without binary/display. Do not ship a “green” that was a skip.

## 12. Move order

Do not rearrange first.

1. **Rename + API lock on current trees**
   - `reynard.rs` → `lurien.rs`, symbols, env, resolver `Result`
   - Banner patch under `software/reynard/patches` (still that path until step 3)
   - Profile import on `FoxBrowserConfig.profile_dir`
   - Bridge: missing engine is fatal
2. **Prove** (section 11)
3. **Move**
   - `git mv software/reynard software/browser/engine` (or the tree in §3)
   - `git mv` guise / foxdriver / bridge / echo
   - Root workspace members + `[workspace.dependencies]`
   - CI path filters
   - install.sh
   - Grep `reynard` / `REYNARD_` / `libs/runtime/guise` / `software/captchaforge` as specified
4. **Observer + yank** (section 9). Delete `software/captchaforge` only after `goto` no longer calls it.
5. **Faces** (section 8)

## 13. Dependents

Every path below is retargeted in the same move commit. No leftover `libs/runtime/guise`.

### 13.1 Move with the product tree

| Today | After |
|---|---|
| `libs/runtime/guise` | `software/browser/guise` |
| `libs/runtime/guise-profiles` | `software/browser/guise-profiles` |
| `libs/runtime/guise-pacing` | `software/browser/guise-pacing` |
| `libs/runtime/guise-choice` | `software/browser/guise-choice` |
| `libs/runtime/guise-oracle` | `software/browser/guise-oracle` |
| `libs/runtime/guise-bridge` | `software/browser/bridge` |
| `libs/runtime/guise-echo` | `software/browser/echo` |
| `libs/runtime/foxdriver` | `software/browser/foxdriver` |
| `software/reynard` | `software/browser/engine` |
| `software/captchaforge` | fold then delete |

Root `Cargo.toml` members + `[workspace.dependencies]` paths update in that commit.

### 13.2 Local path deps that follow

| Consumer | Dep | Today |
|---|---|---|
| guise | guise-{profiles,pacing,choice,oracle}, foxdriver, echo (dev) | relative `../` |
| guise-bridge | guise, foxdriver | `../` |
| captchaforge + `challenge/` | guise, foxdriver | `../../libs/runtime/…` then deleted |
| loginflow | optional guise; `captchaforge` feature | `../guise` — drop captchaforge feature |
| `detonation/jsdet` | guise fingerprint | `../../libs/runtime/guise` |
| `software/santhorchestrator` | optional guise | long relative path |
| `web/truestack`, `web/wptrace` + enumerate | guise / guise-pacing | `../../libs/runtime/…` |
| `detonation/httpdet`, `detonation/sear` | guise-profiles | `../../libs/runtime/guise-profiles` |
| `libs/scanner/scanclient` | guise-profiles, guise-pacing | `../../runtime/…` |
| `libs/offensive/interactsh` | guise-choice, guise-pacing | `../../runtime/…` |
| `libs/performance/io/netshift` | guise-pacing | `../../../runtime/…` |
| `libs/scanner/bugscope` | guise-pacing | `../../runtime/…` |
| `libs/scanner/secmatch` | guise-choice | `../../runtime/…` |
| `libs/runtime/headless` | guise-profiles | `../guise-profiles` |
| rustenium-core vendor | stays | `libs/runtime/vendor/rustenium-core` |

### 13.3 Registry pins (not paths) that must be retargeted before yank

| Consumer | Pin | Action |
|---|---|---|
| `software/wafrift` + cli + captchaforge-bridge | `guise 0.1.2`, `guise-pacing 0.1.0`, `captchaforge 0.2.38`, `runtime-foxdriver 0.1.0` | path to `software/browser/…`; drop captchaforge after daemon |
| `software/ahura` `browser.rs` | guise-bridge URL | daemon, then `lurien-mcp` |
| `software/gossan` | crates.io `guise-pacing 0.1` | leave on registry (we still publish guise-*) |

Absent on disk (do not resurrect): `software/meridian`, `web/calyx`. Delete the stale calyx comment on the captchaforge exclude when that exclude goes.

## 14. Publish

v1 publish set:

- `lurien-browser` on crates.io (CLI + lib + both bins)
- GitHub `santhreal/lurien-browser`
- Engine binary: only if we actually host a tarball. Otherwise “build or set LURIEN_BIN”

Not in v1: PyPI, npm, Node package, macOS/Windows/aarch64 artifacts, `.dev` domain.

## 15. Sophistication bar

### v1

- One launch, lurien engine required
- No `reynard` in shipped trees except the one-release alias table
- Banner gone
- Profile import: cookies, `logins.json`/`key4.db`, localStorage. Not extensions. Not `cert9.db` unless decided
- Playwright Python + Node `executablePath`
- `lurien-mcp` = Playwright-MCP names
- Proxy works; no silent direct fallback
- `lurien_gate` + `live_detector_suite` green on this host
- Managed Turnstile holds via the token hook, not `auto_solve`
- Honest README (Linux matched-host; listed leaks)
- Idle RSS published
- Default headful
- Failure table in §6.2 all have a code path

### v1 is not

- Chromium
- Live fingerprint updates
- TCP/IP OS rewrite
- Ahura hunt tools
- “Solves hard captchas”
- Universal install.sh that fetches an unpublished engine
- npm / Node package / non-Linux

### v1.1

One live hard class, engine-side, measured. Vision helper is a second binary. Until that row is green, homepage captcha claim is managed CF only.

## 16. Acceptance (v1)

- `software/browser/` exists; `software/reynard/` is gone
- `libs/runtime/` has no `guise*`, no `foxdriver`, no `guise-echo`
- every §13 path dep builds against the new paths
- `lurien` and `lurien-mcp` on PATH
- `LURIEN_BIN` / `LURIEN_CONFIG` are the live names
- Grep `\breynard\b` / `REYNARD_` in shipped trees is empty except documented aliases
- Playwright (any language) launches via `executablePath`
- MCP client attaches without prompt changes
- No remote-control banner
- `lurien as --profile ~/firefox-profile` restores cookies + logins
- Missing binary: hard error, never stock Firefox
- `lurien_gate` green with `LURIEN_BIN` + `DISPLAY`
- Managed Turnstile demo still auto-passes
- Proxy works; unreachable proxy does not fall back
- README does not say hard captchas work
- README does not claim a `.dev` or non-Linux install
- wafrift, ahura, loginflow, jsdet, scanclient build
- crates.io `captchaforge`: all 36 versions yanked; `guise` untouched

## 17. Retired docs

Already deleted:

- `MASTER_PLAN/STEALTH_ECOSYSTEM_AUDIT.md`
- `software/captchaforge/ROADMAP_v2.md`
- `PLAN_GUISE_BROWSER_EXTRACTION.md`

Keep (historical; do not execute):

- `MASTER_PLAN/02_stealth.md`, `02_guise.md`, `14_captchaforge_hardening.md`

## 18. CI

Today:

| Workflow | What it does | After |
|---|---|---|
| `.github/workflows/guise-msrv.yml` | MSRV 1.88, `check`/`test -p guise --features browser,http`, guise-echo. Path filter `libs/runtime/guise/**` | Retarget paths to `software/browser/{guise,foxdriver,echo}`. Keep guise-echo. Add `-p lurien-driver` when that crate exists |
| `.github/workflows/guise-telemetry-free.yml` | `cargo tree` http-headers has no reqwest/scanclient/hickory/wreq; `local_echo_regression` | Keep. Path filters follow guise. This is the scanner-safe half |
| `libs/runtime/guise/.github/workflows/ci.yml` | Delegates to `santh-project/santh-ci` | Move with the crate or delete if root CI covers it |
| `software/reynard/.github/workflows/build.yml` | Camoufox **linux/windows/macos × x86_64/arm64/i686**, artifacts named `CamoufoxBuilds-*`, draft GH release, `CAMOUFOX_PASSWD` | v1 ships **linux x86_64 only**. New workflow `software/browser/.github/workflows/engine.yml`: linux x86_64, artifact `lurien-engine-linux-x86_64`. Do not publish Windows/macOS as lurien. Drop Camoufox artifact names |
| `software/reynard/.github/workflows/repo-hygiene.yml` | Fork hygiene | Keep under `engine/` |
| `software/captchaforge/.github/workflows/release.yml` | 5-arch `captchaforge` bins, GHCR `santhreal/captchaforge`, helm chart, `cargo publish` | **Delete** with the crate. Do not port helm, docker, or 5-arch CLI. lurien-browser publish is section 19 |
| captchaforge `ci.yml` / `coverage.yml` / `docs.yml` / `fuzz.yml` / `bench.yml` / `stealth-matrix.yml` / `driver-matrix.yml` / `scorecard.yml` / `keyhog.yml` | Standalone workspace CI | Fold the tests that still matter into `software/browser/captcha` + root CI. Delete the rest |
| `software/browser/docs/stack.sh` | reynard → guise → captchaforge → scorecard. Env `REYNARD_STAGING`, `REYNARD_BIN` | Rename stages and env to lurien. Engine → guise data → captcha (in-tree) → scorecard. `LURIEN_STAGING`, `LURIEN_BIN` |

Path filters that still say `libs/runtime/guise` after the move are a ship-blocker.

Live gates (`lurien_gate`, `live_detector_suite`) stay skip-loud in CI unless the runner has `LURIEN_BIN` + `DISPLAY`. Do not fake them green. A nightlies/self-hosted job on this host is the real gate.

First-run CI on the lurien-browser crate must be green before the first crates.io publish (fleet rule).

## 19. Packaging and release

### 19.1 What we publish in v1

| Artifact | Registry | Name | Notes |
|---|---|---|---|
| Rust lib + CLI | crates.io | `lurien-browser` | `[[bin]] name = "lurien"` and `name = "lurien-mcp"` inside that crate |
| — | PyPI | — | **None.** Playwright talks to the binary. Do not publish `lurien` or `camoufox` as us |
| GitHub repo | github.com | `santhreal/lurien-browser` | Public README lives here |
| Engine tarball | GitHub Release only if we actually build one | `lurien-engine-linux-x86_64-<ver>.tar.gz` | Optional. install.sh must not pretend this exists until it does |

Not in v1: PyPI, npm, Node package, GHCR image, Helm, Windows/macOS/aarch64 engine, `.dev`.

### 19.2 What we stop publishing

| Artifact | Action |
|---|---|
| crates.io `captchaforge` | Deprecation shim, then yank |
| `wafrift-captchaforge-bridge` | One forwarding release, then delete |
| ghcr.io/santhreal/captchaforge | Stop pushing. Leave old tags; do not retag as lurien |
| captchaforge Helm | Delete with the crate |
| 5-arch captchaforge CLI tarballs | Delete. lurien CLI is Linux-first; other OS later if the engine exists there |
| crates.io `guise` | **Keep**. Persona lib. Do not yank, do not rename, do not turn into a browser |
| `runtime-foxdriver` | Keep as internal path dep. Do not market |

### 19.3 Versions

- `lurien-browser` starts at `0.1.0` (or `0.1.0-alpha`). New crate.
- `guise` stays on its own semver (`0.1.6` today). Browser launch symbols moving out is a guise minor if anything public leaves; scanners using `http-headers` must not break.
- Engine version is the Gecko/Camoufox version string (`150.0.2-beta.25` today), not the crate version. `lurien --version` prints both.
- `LURIEN_CONFIG` JSON is versioned. A stale `REYNARD_CONFIG` blob is accepted for one release, then rejected.

### 19.4 Authors / license / NOTICE

- Cargo.toml `authors`: `Santh <64453045+santhreal@users.noreply.github.com>`
- Public crate: MIT OR Apache-2.0
- Engine tree: MPL-2.0 + Camoufox/Firefox NOTICE. Keep `NOTICE`. Engine-side captcha hooks are MPL (accepted).
- No Python wheel.

### 19.5 install.sh honesty

If no engine tarball is hosted, install.sh wires a local build and exits 1 with the build recipe. `curl | sh` that claims to fetch Gecko is a lie until the Release asset exists.

Fonts: `bundle/fonts/` is still not shipped (proprietary). Linux matched-host does not need them. Cross-OS personas stay unsupported in v1.

## 20. Testing

### 20.1 Must stay, renamed

| Today | After | Class |
|---|---|---|
| `reynard_gate` | `lurien_gate` | High/Critical vs stock FF-150 |
| `live_detector_suite` | same name | per-persona live + scorecard |
| `reynard_canvas_audio_farble_live` | `lurien_*` | seed / farble |
| `reynard_window_geometry*` | `lurien_*` | PHANTOM_WINDOW_HEIGHT |
| `reynard_mouse_model_ownership` | `lurien_*` | guise owns mouse |
| `e2e_reynard_bridge.sh` | `e2e_lurien_bridge.sh` | HTTP path |
| other guise-bridge e2e (`dialog`, `mouse`, `sensors`, `upload`, `concurrent`) | keep | skip-loud without engine |
| `local_echo_regression` | keep | Layer-2, no browser |
| foxdriver `cross_origin_click` | keep | trusted click into OOPIF |
| camoufox `software/reynard/tests/` Playwright suite | stay under `engine/tests/` | upstream; not a lurien product |

### 20.2 Must add

- Resolver: missing binary → `Err` with the install sentence. Mutation: reintroduce `Option` + Firefox fallback and this test goes red.
- Every launch path (`Browser::launch`, bridge, MCP, CLI) hits that resolver. Adding a new launcher without it fails a registry test.
- Banner absent (screenshot or pref/probe).
- Profile import: cookies + logins round-trip; missing `key4.db` warns; corrupt `cookies.sqlite` errors.
- Unreachable proxy does not fall back to direct.
- MCP: Playwright tool names present; `challenge` absent.
- Grep gate in CI: `\breynard\b` / `REYNARD_` empty in shipped trees except the alias table.
- `cargo tree -p lurien-driver` does not pull scanclient / wafrift / ahura.

### 20.3 Skip vs fail

Tests skip-loud without `LURIEN_BIN` + `DISPLAY`.
The product never skips. A published binary that cannot find the engine exits 1.

### 20.4 Stock Firefox

`STOCK_FIREFOX_BIN` stays for the oracle only. Not a product fallback. Document where the staged FF-150 lives on this host; do not put `/tmp/firefox-150` in the README.

## 21. Decisions from this thread (do not reopen)

- **Not an agent browser.** Do not compete with browser-use / Comet / Dia / Atlas. Capture Playwright users.
- **The solver is catalog + primitives.** Closed kinds. Vendor = TOML. No vendor identifier in `engine/additions/challenge/`. Sidecar detect+offset-click is not a solver. Delete 2captcha/CapSolver. Ban the (28, 32) offset.
- **Not novel until checkbox/visual land on a live vendor.** Stealth Firefox + Playwright API is Camoufox. `score` token hook is v1. Child-BC Input + Snapshot is the new sentence.
- **MCP description is the skill.** No `SKILL.md`. No `challenge` tool.
- **Ahura hunt stays Ahura.** jwt / wafrift / race / oob do not land in lurien. Ahura calls `lurien-mcp`.
- **Three artifacts, one face.** Engine (MPL) + persona (`guise`, MIT) + driver (foxdriver + bridge). Public word: lurien.
- **Do not fold foxdriver into guise.** Kills the stock-vs-patched oracle and pulls rustenium into scanners.
- **foxdriver is Rust, ours.** Crate `runtime-foxdriver` 0.1.5, BiDi via rustenium. Not Python. Path-dep only after move; do not market. Camoufox `pythonlib/` is upstream tests, not a product.
- **Hard captchas in Gecko.** Page sidecar cannot see chrome-privileged widget state. User accepted MPL on those files.
- **VLM stays a helper process.** Not in libxul. Bytes come from a child-BC compositor grab, not a parent screenshot.
- **v1 captcha claim is managed CF / score-class only.** Interactive Turnstile / recaptcha grid / Arkose are v1.1, measured on a live page, not a CF test key.
- **Honest stealth:** matched-host Linux Firefox. Cross-OS fonts/WebGL/WebGPU, inert `canvas:seed`, proxyless TTL remain.
- **Default headful.** Camoufox headless scores worse.
- **Do not move trees until** launch API, banner, profile import, and a live gate exist on current paths.
- **`guise` crate names stay.** crates.io persona lib. Every guise crate *path* moves to `software/browser/`. `libs/runtime/` holds none after cutover. Scanners retarget path deps.
- **Yank every captchaforge version.** 36 live (0.1.0–0.2.40). Shim, then `cargo yank` all. Name stays reserved. Do not yank `guise`.
- **Title the GitHub/crates crate `lurien-browser`.** Spoken/CLI/MCP: `lurien`. Dead 2021 config crate owns crates.io `lurien`; that is not a browser. No PyPI package.
- **Engine rename is complete.** `LURIEN_BIN`, `LURIEN_CONFIG`, `software/browser/engine/`, test names. One-release aliases for `REYNARD_*` then delete.

## 22. Env and leftover names

| Today | After |
|---|---|
| `AHURA_GUISE_BRIDGE_URL` / `GUISE_BRIDGE_URL` | keep one release, then `LURIEN_BRIDGE_URL` or MCP only |
| `MERIDIAN_GUISE_*` (still in bridge) | `LURIEN_*` or delete with Meridian (absent on disk) |
| `GUISE_BRIDGE_HEADFUL` / `GUISE_BRIDGE_TZ` | `LURIEN_HEADFUL` / `LURIEN_TZ` |
| `MOZ_DISABLE_CONTENT_SANDBOX` | keep (Gecko). Not a product name |
| `CAMOU_CONFIG` | keep as last-resort engine read |
| `CAMOUFOX_PASSWD` | engine fetch secret. Rename in docs to lurien fetch; value can stay |
| rustenium `RUSTENIUM_COMMAND_TIMEOUT_SECS` | keep; comments say lurien |
| `CARGO_TARGET_DIR` | never set; `stack.sh` still hard-fails if target is inside Santh |

## 23. Docs that keep `software/browser/` maintainable

These ship in the same change as the move. A tree with no owners is unmaintainable.

| File | Owns | Must say |
|---|---|---|
| `README.md` (folder root) | public face | what lurien is, crates, install, honest leaks, `score` only until a live row |
| `docs/TREE.md` | folder owners | every directory, who may import whom, one-way: lurien → guise/foxdriver; guise ↛ engine C++; captcha/kinds → Catalog; helpers ↛ page |
| `docs/KINDS.md` | extension | add a vendor (TOML + fixture). add a kind (`_schema.toml` + primitive + fixture). unknown kind is red |
| `docs/ENGINE.md` | build | Camoufox `make`, `LURIEN_BIN`, linux x86_64 only |
| `docs/REBASE.md` | fork life | rebase onto next Firefox/Camoufox; which patches must apply; banner + config + observer register |
| `docs/NOTICE` | license | MPL engine + MIT crate + Camoufox/Firefox |
| crate `--help` + MCP description | skill | no `SKILL.md`. no `challenge` tool |
| `lurien-driver` CHANGELOG | crate | first 0.1.0 |
| captchaforge deprecation CHANGELOG | yank | points at lurien; yank date |
| wafrift README | consumer | delete Chromium / captchaforge sentences |

**Import law (enforced, not prose):**

- `lurien` may depend on guise + foxdriver. It may not depend on captchaforge.
- guise `browser` feature may depend on foxdriver. guise default features may not.
- `engine/additions/challenge/` has no vendor identifiers. CI greps them out.
- `captcha/kinds/*.toml` is the only place a vendor name is allowed as a product binding.
- helpers (`vision` / `audio`) speak HelperSock. They never import foxdriver. `pow`
  needs no helper: the search runs in the browser.
- scanners keep `guise-*` crate names. They do not import `lurien` or `foxdriver`.

Do not write:

- `SKILL.md`
- a homepage that names `checkbox`/`visual` before a live scorecard row
- `lurien.dev` until owned
- Camoufox marketing as the public README

## 24. Extra acceptance

- guise-msrv + telemetry-free path filters match `software/browser/{guise,foxdriver,echo}`
- captchaforge release.yml / helm / GHCR are gone
- all 36 crates.io `captchaforge` versions yanked (`yanked=true` on the API)
- engine workflow publishes linux x86_64 lurien artifacts, not `CamoufoxBuilds-*`
- `stack.sh` speaks lurien
- no `pip install lurien`; Playwright uses `executablePath`
- `cargo add lurien-driver` is the Rust face
- no helm, no captchaforge docker tag reuse
- CHANGELOG + NOTICE + authors line present
- `software/browser/README.md` plus `docs/{PLAN,TREE,KINDS,ENGINE,REBASE}.md` exist; TREE import law matches Cargo.toml
- `_schema.toml` kinds all have a fixture; unknown kind fails closed
- grep of `engine/additions/challenge/` for vendor names is empty
- this file’s §21 decisions are all reflected in README/help
