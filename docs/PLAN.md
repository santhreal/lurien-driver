# Lurien product spec

One installable browser, one driver, one registry of verbs behind every face.

The word is **lurien** everywhere a human types it. The browser is
**lurien-browser**, a Gecko fork under MPL-2.0 in its own repository. The Rust
crate that drives it is **lurien-driver**, and it exposes a CLI (`lurien`), an
MCP server (`lurien-mcp`), an HTTP face (`lurien serve`), and a library.

A crate that cannot paint a page is not the browser. `lurien-driver` requires an
installed `lurien-browser` and says so when it is missing.

## 0. Shipped

| Item | State |
|---|---|
| `santhreal/lurien-driver` | the control tree: driver crate, persona crates, catalog, docs |
| `santhreal/lurien-browser` | the Gecko fork |
| crates.io `guise` family | unified at `0.1.8` |
| crates.io `captchaforge` | 36 versions yanked; `0.2.41` stays as the retirement notice |
| challenge subsystem | `engine/additions/challenge/`, packaged, proven by `lurien/tests/e2e_challenge.sh` |
| claimed kinds | `none`, `score`, `checkbox`, `visual`, `audio`, `pow`, `slider`, each with a dated scorecard row |
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
| a start that is asserted | the browser appends one `started` row before any page, `goto` refuses a silent session instead of reporting a clean page, and the patched hook that starts the subsystem is held by `lurien/tests/engine_package.rs`, proven by `lurien/tests/e2e_bootstrap.sh` |
| selectors and the wait | `role:`/`text:`/`label:`/`placeholder:`/`testid:` or CSS, one resolver, acts wait for their element, ambiguity is refused with candidates, proven by `lurien/tests/e2e_locator.sh` |
| the agent's page | `snapshot` answers with roles, names and handles; `ref:eN` acts, and a handle whose node changed is refused, proven by `lurien/tests/e2e_snapshot.sh` |
| one call, several verbs | `batch` runs a step list, validates it before running any of it, stops at the first failure and says how far it got, on all three faces, proven by `lurien/tests/e2e_batch.sh` |
| the environment the page reads | geolocation, wall clock, permissions and locale are set over the engine control channel, not by page script, proven by `lurien/tests/e2e_geo.sh` and `lurien/tests/e2e_clock.sh` |
| the network view | routes are applied on the channel in the parent, and one redaction rule serves the log, the HAR and the route view, proven by `lurien/tests/e2e_route.sh` and `lurien/tests/e2e_har.sh` |
| a handle that means one context | a frame handle is minted once per browsing context and refused when the context is gone, proven by `lurien/tests/e2e_frames.sh` |
| a grid answered by looking | a tile challenge is cropped in the widget's own context, read with a local object detector, and clicked by index with a dealt pace, proven by `lurien/tests/e2e_visual.sh` |
| a recording answered by hearing | the control is pressed and the clip is fetched in the widget's own context, transcribed by a local Whisper export, and typed key by key, proven by `lurien/tests/e2e_audio.sh` |

`lurien-driver` is not published while `audio` has no mutation-gated scorecard row.

## 1. Names

| Face | Token |
|---|---|
| Spoken, CLI, MCP | `lurien`, `lurien-mcp` |
| The browser | `lurien-browser`, GitHub `santhreal/lurien-browser`, MPL-2.0, own repository |
| The driver | `lurien-driver`, GitHub `santhreal/lurien-driver`, crates.io `lurien-driver` |
| Installed browser binary | `~/.local/share/lurien/lurien` |
| Persona crates | `guise`, `guise-profiles`, `guise-pacing`, `guise-choice`, `guise-oracle` |
| Driver internals | `runtime-foxdriver`, `lurien-vision`, `guise-echo` |
| Browser environment | `LURIEN_BIN`, `LURIEN_CONFIG`, `LURIEN_CHALLENGE`, `LURIEN_CONTROL` |

Names that are not available and not us:

- crates.io `lurien` is a dead 2021 config-file crate.
- crates.io `guise` is the persona library, not a browser.
- `reynard-browser` on GitHub is an unrelated Gecko iOS browser.

Do not print `lurien.dev` until the domain is owned. The install URL is GitHub
raw or `santh.dev`.

## 2. Goal

Someone who already drives Playwright installs lurien and either keeps their
script, pointing `executablePath` at the installed browser, or adds:

```json
{ "mcpServers": { "playwright": { "command": "lurien-mcp" } } }
```

The MCP tool names match `@playwright/mcp`, and the tool description is the only
skill. Captchas that are really scores pass because the persona holds. Kinds
that need an act are solved inside the browser, not by a sidecar crate and never
by a third-party HTTP solver.

## 3. Tree and licence

`docs/TREE.md` owns the directory layout, every folder's owner, and the import
law. It is the file to read before adding a crate or a module, and it is
enforced by `lurien/tests/verb_registry.rs` and the `import-law` CI job rather
than by prose here.

The layering that does not change:

- The browser is a separate process under MPL-2.0. libxul is never linked into
  the MIT crate.
- `lurien-driver` may depend on `guise` and `runtime-foxdriver`. Nothing else in
  its graph is allowed.
- `guise` without the `browser` feature does not pull the driver, so a scanner
  that wants headers does not build a browser stack.
- A vendor name appears in `captcha/kinds/*.toml` and nowhere else. The modules
  under `engine/additions/challenge/` implement kinds.
- Helpers speak the helper protocol over a socket. They never import the driver
  and never see the page.

The public crate is MIT OR Apache-2.0. Modules under
`engine/additions/challenge/` are MPL-2.0 with the rest of the fork. Kind TOML
is MIT: a new vendor is data, not a patch.

## 4. Public surface

```rust
lurien::Browser::launch(profile) -> Browser   // resolve, spawn, BiDi, Page
browser.session().call(verb, args) -> Output  // the only way a verb runs
```

Sixty-four verbs across thirteen domains (`state`, `net`, `storage`, `input`,
`profile`, `context`, `intercept`, `observe`, `dialog`, `dom`, `page`, `frame`,
`session`). `docs/VERBS.md` is generated from the registry and a test fails when
it is stale, so that file is the reference and this one does not repeat it.

One `VerbSpec` per verb declares its name, its arguments, their types and
whether each is required. Every face reads the same specs: the CLI parses into
them, the MCP server derives its JSON Schema from them, and `lurien serve`
routes into them. A face that imported a verb module directly would fail a
registry law.

Argument names are lower snake case. Schemas set `additionalProperties: false`.
Required arguments precede optional ones, and an argument is never both required
and defaulted.

Default is headful. Headless is a documented weaker mode, not the demo.

v1 is Linux x86_64 only.

## 5. Launch contract

### 5.1 What a launch does

```
install.sh
  -> ~/.local/share/lurien/lurien exists and is executable
  -> lurien --version prints the crate version and the Gecko version
Browser::launch(profile)
  -> resolve the engine binary
  -> enforce persona coherence
  -> refuse a non-Firefox persona
  -> align the UA major to the engine version
  -> write LURIEN_CONFIG, LURIEN_CHALLENGE and LURIEN_CONTROL into a unique temp dir
  -> spawn, then ask session.status until the browser answers
  -> apply the session-age seed
  -> return a Browser
goto url
  -> real NSS handshake
  -> the subsystem must have appended its started row
  -> classify: none, score, checkbox, slider, pow, or a refusal
  -> document usable
```

Readiness is a command answered, not a port bound. A dead attach is a relaunch,
never a reconnect, and there are three launch attempts before the error.

Concurrent launches get unique temp directories.

### 5.2 Failures

Every failure is a typed variant in `lurien/src/error.rs`, and every variant's
message names the corrective action. `every_error_shows_what_it_captured` holds
that: a variant with no captured detail, or with no next action, is red.

| Class | What the caller is told |
|---|---|
| `EngineMissing` | the browser is not installed; run install.sh or set `LURIEN_BIN` |
| `EngineNotExecutable`, `NotFirefox` | the path, and how to fix or repoint it |
| `DisplayUnset` | headful needs a display; headless is weaker and must be asked for |
| `BidiTimeout`, `SessionTimeout` | the elapsed time, the last error, and the timeout to raise |
| `PersonaIncoherent`, `NonFirefoxPersona`, `CrossOsPersona` | the field that does not hold, and the stock persona that does |
| `ProxyUnreachable` | the proxy URL and the connect error. There is no fall back to direct |
| `ProfileLocked`, `CookiesCorrupt`, `LoginsSkipped` | the path, and whether the import continued without it |
| `ChallengeNotStarted` | the browser never started its solver, so a clean page cannot be told from a blind one |
| `HardCaptcha` | an interactive widget the engine reported nothing about, by name. Never a pass, never a third-party call |
| `ChallengeRefused` | the kind the engine drove and could not clear, with the engine's own reason and what to do about it |
| `ScoreFailed` | the classification or the token wait that ran out, and the budget it was given |
| `EvidenceVersion` | the version the row carried and the version this build reads |
| `GeolocationUnavailable`, `ControlUnavailable` | which control call failed, on which port, and why |
| `DownloadDirUnusable`, `DownloadFailed` | the directory or the file, and what happened instead |
| `Unresolved` | the selector as written, how long it waited, and the candidates when it was ambiguous |
| `BadArgs`, `UnknownVerb`, `UnknownMcpTool` | the verb or tool, what was wrong, and where the registry is |
| `BatchFailed` | the step index, the verb, how far it got, and how many steps did not run |
| `EngineCrash` | the wrapper log path. There is no restart loop |

Tests that cannot see a display or a browser skip loud: they print why and exit
0. The product never skips.

### 5.3 What stays hidden

A stock Firefox launch and the raw BiDi driver exist for the patched-versus-stock
oracle. `STOCK_FIREFOX_BIN` names the staged build on a developer host. It is
not a product fallback and a user of `lurien` never reaches it.

## 6. Install

### 6.1 What install.sh does

```
software/browser/install.sh [/path/to/built/browser]
```

1. Refuse anything but Linux x86_64.
2. Resolve a built browser: the argument, else `LURIEN_BIN`, else the newest
   `camoufox-*/obj-*/dist/bin/camoufox` under the engine tree or `LURIEN_STAGING`.
3. Symlink it to `~/.local/share/lurien/lurien`.
4. Put `lurien` and `lurien-mcp` on the path under `~/.local/bin`.
5. Print `lurien --version`.
6. Find nothing, and exit 1 with the build recipe.

There is no hosted browser tarball. `curl | sh` that claimed to fetch Gecko
would be a lie until a release asset exists, so install.sh wires a local build
and says so.

Fonts under `engine/bundle/fonts/` are not shipped. A matched-host Linux persona
does not need them, and cross-OS personas are unsupported.

### 6.2 Names still honoured

The installed browser reads the older names, so the driver and install.sh still
accept them for one release. This is a live decision, not a leftover.

| Read | Also accepted |
|---|---|
| `LURIEN_BIN` | `REYNARD_BIN`, `GUISE_REYNARD_BIN` |
| `LURIEN_CONFIG[_n]` | `REYNARD_CONFIG[_n]`, then `CAMOU_CONFIG[_n]` |
| `~/.local/share/lurien/lurien` | `~/.cache/lurien/lurien`, `/opt/lurien/lurien` |

`CAMOU_CONFIG` stays as the upstream read of last resort. The fork still
self-reports its upstream branding in the about dialog; the installed binary is
named `lurien` and the branding patch is not written yet.

## 7. Faces

- `lurien` on the command line. Verbs and their arguments come from the registry.
- `lurien-mcp` over stdio. Playwright-MCP tool names. There is no `challenge`
  tool, because a captcha is a property of `goto`.
- `lurien serve`, one HTTP process, no separate daemon and no verb passthrough on
  the command line. Sessions have an idle deadline and report their own
  telemetry.
- Rust, through `lurien::Browser`.
- Any Playwright language, through `executablePath`.

There is no PyPI package, no npm package and no Node package. Playwright talks to
the binary.

## 8. The solver

### 8.1 Why the browser is the solver

A page sidecar evaluates detect JS, guesses a widget offset inside a
cross-origin frame, clicks through the driver and polls a hidden input. Page JS
cannot see that frame, hardcoded offsets rot, and a third-party HTTP solver
leaks the session and the timing.

| Path | What it sees | What it clicks | Tell |
|---|---|---|---|
| Page JS | same-origin DOM | `isTrusted=false` | scored immediately |
| Sidecar from the parent frame | a parent rect and a guessed offset | trusted, often the wrong frame | geometry rot, extra round trips |
| A third-party solver | a screenshot, later | a token somebody else made | token source and round-trip time |
| The chrome process | every browsing context, including cross-origin frames and closed shadow roots | the child context, through the same event path as a real click | none beyond a real visit |

Three pieces have to be owned together for this to work: a patched Gecko, one
persona seed behind TLS, UA and motion, and a driver that speaks to both. The
solver is the privileged process, not another wrapper around the page.

### 8.2 The pipeline of one goto

```
goto(url)
  real handshake, guise persona
  Observer attaches to every browsing context and announces its start
  the first sighting opens a settle window, so a late widget is still seen
  Classify: chrome signals -> kind, target, token channel
    none      -> document usable
    score     -> wait for the vendor's own write
    any act   -> Prelude in the top context: settle, wander, wheel, dwell
    checkbox  -> a trusted click in the child context, along a dealt path
    slider    -> snapshot the widget, measure the axis, drag along a profile
    pow       -> worker lanes in the browser, submit through the bound address
    unclaimed -> refuse by name
  reduce by severity: the page is solved at the widget that gates it
  report every widget the page held
```

### 8.3 Catalog and primitives

A vendor is data. `captcha/kinds/*.toml` binds chrome-visible signals to a kind,
a target, and the channels a token can arrive on. Kinds are a closed set in
`_schema.toml`, and the catalog reaches the browser as JSON in
`LURIEN_CHALLENGE`.

| Kind | Primitive | Solved when |
|---|---|---|
| `none` | none | the document is usable |
| `score` | token wait | the vendor wrote its own field, cookie, storage key or message |
| `checkbox` | trusted click in the child context | the token arrives |
| `slider` | snapshot, measure, drag | the token arrives |
| `pow` | worker lanes in the browser | the answer is accepted at its bound address |
| `visual` | snapshot the grid, ask the vision helper, click each named tile | the token arrives. A binding with no `[grid]` table is refused by name |
| `audio` | press play, fetch the clip in the widget's context, ask the helper to transcribe, type the answer | the token arrives. A reading under the floor asks for another recording. A binding with no `[audio]` table is refused by name |
| `fail` | none | never. The classification itself is the refusal |

`docs/KINDS.md` is the procedure for both extensions, and it names the law that
goes red at each step: adding a vendor is a TOML plus a fixture, adding a kind is
nine steps from the schema name to the budget. A vendor TOML naming an unknown
kind is red. A claimed kind with no runnable proof is red. A new kind that
nobody wired is red rather than silently absent.

The modules are fixed and carry no vendor strings: `Bootstrap`, `Observer`,
`Catalog`, `Classify`, `Kinds`, `Solver`, `Input`, `Keys`, `Dynamics`,
`Prelude`, `Token`, `Snapshot`, `Pow`, `HelperSock`, `Control`, `Clock`, `Geo`,
`Net`, and the parent and child actors. They are chrome-privileged ES modules,
packaged through `jar.mn`, and `lurien/tests/engine_package.rs` holds the
packaging: a module missing from the manifest, an import that does not resolve,
or a lost start hook is a red test rather than a browser that launches and
observes nothing.

Motion, typing rhythm and reading cadence come from `guise`. The driver owns the
corpus and the sampler, deals a deck, and ships a seed; the browser owns the
order and records which entry it used. The dealt index, not the observed motion,
is the checkable claim, because Gecko coalesces pointer moves.

### 8.4 What the solver does not do

- No offset guessed from a parent rect.
- No vendor identifier in the browser's modules.
- No call to any third-party solving service.
- No fabricated token, and no retry with a different persona unless the caller
  asked for one.
- No machine-learning runtime inside libxul. Vision and audio are helper
  processes.
- No kind named in the README before it has a dated scorecard row.

### 8.5 The patches the browser carries

`engine/patches/` holds the fork. Three patches are load-bearing for this
product and `docs/REBASE.md` is the runbook when they stop applying:

- `challenge-register.patch` starts the subsystem after the BiDi agent, adds the
  module directory to the build, and adds the packaging rows.
- `config.patch` reads `LURIEN_CONFIG` before the upstream name.
- `browser-init.patch` and the banner work keep the remote-control cue off the
  chrome.

The rest are the upstream fingerprinting patches, which are inherited, not ours.

## 9. Claimed and not claimed

Claimed means there is a fixture, a runnable script, and a dated row in
`docs/bench-results/challenge-scorecard.md`. Today that is `none`, `score`,
`checkbox`, `visual`, `audio`, `slider`, `pow`.

`visual` is claimed against a fixture grid and needs an object detector on disk,
which the vision helper loads on the first request and refuses by name without.
The shipped `hcaptcha`, `recaptcha` and `arkose` bindings carry no `[grid]` table,
so those widgets are recognized and their solve refused until one does. The
scorecard records why with numbers: perception is no longer the obstacle, since the
detector answers a live reCAPTCHA crop exactly, but reCAPTCHA opens its grid in a
different browsing context than the anchor that was pressed and asks again after an
answer, and hCaptcha renders one canvas with counting and comparison tasks rather
than a tile grid. A live row for `visual` needs an open step and rounds in the
binding, not another selector.

`audio` is claimed against a fixture that speaks a code and needs a speech model
on disk, which the vision helper loads on the first request and refuses by name
without. The shipped `hcaptcha_audio` binding carries no `[audio]` table, because
what the audio task renders is minted per session, so the widget is recognized and
the solve is refused by name. A live row for `audio` needs a binding that names the
control, the source, the answer field and the alphabet a real vendor uses.

Not built:

- Browser branding still says upstream. A full engine build is required to
  change it, and a full build currently fails on an unrelated Rust component.
- The browser repository has diverged from its remote and is committed locally
  only.
- `lurien-driver` is unpublished.

Not in scope:

- Chromium.
- An agent browser. This is a driver for people who write scripts.
- Live fingerprint updates, or a TCP/IP stack rewrite.
- Hunt tooling. That stays in its own product and calls `lurien-mcp`.
- Anything but Linux x86_64.

Known leaks, stated in the README:

- Matched-host Linux Firefox only.
- Cross-OS fonts, WebGL and WebGPU.
- An inert `canvas:seed`.
- Host TTL without a proxy.

## 10. Publishing

| Artifact | Where | Name | State |
|---|---|---|---|
| driver crate, two binaries | crates.io | `lurien-driver` | unpublished, gated on a claimed `audio` |
| persona crates | crates.io | `guise` family | published at `0.1.8` |
| driver internals | crates.io | `runtime-foxdriver` | published, internal, not marketed |
| retired crate | crates.io | `captchaforge` | 36 versions yanked, `0.2.41` is the notice |
| the browser | GitHub | `santhreal/lurien-browser` | source only, no hosted tarball |

Versions:

- `lurien-driver` starts at `0.1.0`.
- The `guise` family keeps its own semver.
- The browser's version is the Gecko version string, not a crate version.
  `lurien --version` prints both.
- `LURIEN_CONFIG` is versioned. Evidence rows carry their own version and the two
  repositories are held equal by a test.

`Cargo.toml` authors is `Santh <64453045+santhreal@users.noreply.github.com>`.
The public crate is MIT OR Apache-2.0. The fork keeps its MPL notice, and
`docs/NOTICE` carries both.

## 11. CI

`.github/workflows/ci.yml` runs on every push to main and every pull request.
Seven jobs, each one cheap and each one guarding a class that a local run misses:

| Job | What it proves |
|---|---|
| `test` | `cargo test --workspace --all-targets` and the doc tests. Tests that need the browser skip loud |
| `import-law` | the driver's graph reaches nothing outside guise and foxdriver, and guise without `browser` does not pull the driver |
| `no-vendor-in-engine` | every vendor binding names a kind the schema closes |
| `pow-worker` | the worker's own SHA-256 matches a reference over hundreds of inputs |
| `resolver` | the scripts evaluated in the page parse, which no Rust test can see |
| `e2e-scripts` | every live script parses, so a syntax error does not read as a skip |
| `slider-measurement` | the measurement is arithmetic over a crop, including one the browser rendered |

Two rules hold the whole file:

- A workspace member must resolve on a fresh clone. A path dependency pointing
  outside the repository makes the tree unloadable on every machine but one, and
  that is a defect, not a convenience.
- A cross-repository proof skips loudly where the other repository is absent. The
  packaging laws are vacuous without the browser checkout, and the worker digest
  check prints a skip and exits 0.

The live gates stay out of CI. They need a browser and a display, and a run on
this host is the real gate. A skip is never reported as green.

## 12. Testing policy

A test defends an observable contract and fails on a plausible bug. It does not
assert source text, existence for its own sake, or a value read back out of the
thing it is testing.

- Every claim in this file that names a script or a law is runnable. A guide that
  cites a test name that no longer exists is red.
- A regression test goes red against the pre-fix code. A fix is reported with the
  variants that were re-injected and whether each was caught.
- A number that lives in two repositories gets a test: the evidence version, the
  helper protocol version, the completeness of the severity table, the prelude's
  share of the budget.
- Anything with a deadline, a budget or a retry asserts that it ends and asserts
  the bound. Every refusal names the budget it was given.
- A fixture that can be fooled stops being evidence. The adversarial page refuses
  an untrusted click, a press with no approach, a forged token, and a read across
  the origin boundary.
- Statistical claims are asserted where the sample is.

## 13. Decisions

- **The browser is the solver.** A page sidecar cannot see a cross-origin
  widget's state, so the work happens in the privileged process.
- **A vendor is data, a kind is code.** Kinds are a closed set. Adding a vendor
  is a TOML and a fixture. There is no vendor identifier in the browser's
  modules.
- **One registry behind every face.** One `VerbSpec` per verb, `Session::call` as
  the only entry, `docs/VERBS.md` generated, a face that bypasses it is red.
- **Evidence is append-only.** A diagnostic row is never a verdict, and a verdict
  belongs to one visit.
- **A claim needs a proof that can be run again.** Fixture plus script plus a
  dated scorecard row, or the kind is refused by name.
- **A silent browser is not a clean page.** The subsystem announces its start on
  every run, and `goto` refuses a session that never did.
- **Dynamics are never a constant.** The driver samples and deals; the browser
  orders and records. Decks are salted apart.
- **A page is solved at the widget that gates it.** Severity outranks signal
  count, and every widget the page held is reported.
- **A solve is observed where the vendor chose to answer.** An empty named
  channel is a refusal, not a pass.
- **Failures are typed and name a next action.** No error is a bare string.
- **Helpers are processes.** No model runtime in libxul. Bytes come from a
  compositor grab of the widget's own context.
- **The driver and the persona library stay apart.** Folding the driver into
  guise would kill the stock-versus-patched oracle and pull the BiDi stack into
  scanners.
- **Default headful.** Headless scores worse and is a documented weaker mode.
- **Honest stealth.** Matched-host Linux Firefox, with the leaks in section 9
  written down rather than papered over.
- **A published crate's public surface is documented or the build fails.** A doc
  line states what a field name cannot: a body the protocol never delivers, a
  phase that is absent, a value kept as base64.
