# Tree

Every directory under `software/browser/` has one owner. Imports are one-way.

`lurien` is the product. `guise`, `guise-*`, and `foxdriver` are dependencies a
user never names: `guise` is a published persona library with consumers outside
this tree, so it stays a separate crate and never learns that lurien exists.

| Directory | Owner | May import | Must not import |
|---|---|---|---|
| `lurien/` | public face | guise, foxdriver | captchaforge, engine C++, helpers |
| `engine/` | Gecko fork (MPL) | nothing in this tree | guise, foxdriver, lurien |
| `engine/additions/challenge/` | kind primitives | Catalog TOML at runtime | vendor names as identifiers |
| `guise/` | persona compiler | guise-*, foxdriver (feature `browser` only) | engine C++, lurien |
| `guise-profiles/` | UA / profile ids | nothing | any browser crate |
| `guise-pacing/` | backoff / jitter | nothing | any browser crate |
| `guise-choice/` | sampling | nothing | any browser crate |
| `guise-oracle/` | surface taxonomy | nothing | any browser crate |
| `foxdriver/` | BiDi Page | rustenium | guise, lurien, captchaforge |
| `echo/` | test reflector | rustls | foxdriver, lurien |
| `captcha/kinds/` | vendor TOML | — (data) | — |
| `lurien/kinds` | symlink to `captcha/kinds` | — | — |
| `captcha/vision/` | `lurien-vision`: the slider measurement helper, one crop in, one axis out | png, serde | foxdriver, page, guise, lurien |
| `docs/` | plan, verb reference, selectors, batch, kinds, axes and what proves them (`COMPARISON.md`), helper protocol (`HELPERS.md`), and this file | — | — |

All of these directories live under `software/browser/`.

`lurien/kinds` is a symlink, not a copy: `cargo package` follows it, so the
catalog travels inside the published crate while the tree keeps exactly one copy
of the data. `build.rs` and the registry test read `lurien/kinds`, which resolves
in the tree and in an unpacked tarball alike.

Scanners keep `guise-*` crate names. They do not import `lurien` or `foxdriver`.

## Inside `lurien/`

| Path | Owner | Law |
|---|---|---|
| `src/verb/<domain>/<verb>.rs` | one verb | Exactly one `pub static SPEC`, its `run`, and its unit tests. A new verb is this file plus one line in the domain's `SPECS`. |
| `src/verb/<domain>/mod.rs` | domain | Lists `SPECS`. Shared helpers for that domain only. |
| `src/verb/mod.rs` | registry | `VerbSpec`, `Domain`, `Output`, `registry()`, `lookup()`. Knows no individual verb. |
| `src/verb/args.rs` | decoding | The only argument decoder. Unknown argument, missing required, wrong type all fail closed here. |
| `src/verb/schema.rs` | generators | One spec becomes a JSON Schema, a clap command, an HTTP decode, and the `docs/VERBS.md` row. |
| `src/session.rs` | the API | `Session::call(verb, args)` is the only entry point any face may use. |
| `src/locator.rs`, `src/locator.js` | selectors and the wait | The only place that decides what a `selector` means and how long an act waits for it. `locator.js` is evaluated in the page, mutates nothing, and answers with a CSS path, so acting goes through the ordinary element path. Reference: `docs/SELECTORS.md`. |
| `src/snapshot.rs`, `src/snapshot.js` | the page an agent acts from | Walks the page into role/name/handle nodes and owns the handle table. Nothing is tagged in the page; a handle is checked against the role and name it was captured with before it acts. |
| `src/verb/session/batch.rs` | one call, several verbs | Parses and type-checks every step against its verb's spec before running any, then calls `Session::call` in order and stops at the first failure. Reference: `docs/BATCH.md`. |
| `src/verb/net/mod.rs`, `src/verb/net/har.rs` | what traffic a face may see | One redaction rule for every view of the network: credential headers, sensitive query values wherever a URL appears including inside a header, and cookie values. `har.rs` writes a HAR 1.2 log through that same rule, and carries a request body only in the shapes it can redact. |
| `src/frame.rs` | names for frames | A handle is minted the first time a context is seen and never reused, so `f2` is the same frame after it navigates. An index shifts and a URL changes; both then resolve to a different document, which is the failure this prevents. A handle whose context is gone is refused with what it was, and every verb that takes a frame resolves through the same table. |
| `src/download.rs` | downloads | Owns the per-session download directory, the prefs that point Firefox at it with no prompt, and the wait: a download is finished when its bytes are on disk, not when the browser says so. |
| `src/chooser.rs`, `src/chooser.js` | the chooser a page opens | Arms one interception, cancels the default action of the click that would open the native picker, and attaches the caller's files to the input the page meant. The page's own listeners still run; nothing is intercepted unless a caller asked. |
| `src/shot.rs` | what a picture covers | Turns viewport, whole document, rectangle and element into one area, measured in the document that owns it. Nothing scrolls: an element below the fold is a document-origin clip, so the page is left where the caller left it. Also reads a PNG's own size, which is what every face reports back. |
| `src/geo.rs`, `src/control.rs` | where the browser thinks it is | The engine applies a position in the process that owns the tab, which is the only place `navigator.geolocation` reads one, so a loaded page moves without a reload. `control.rs` is the client for that channel: a loopback line protocol whose port is chosen before launch and whose token keeps the open port private to the session. `geo.rs` owns what the session serves; the starting position is the persona's own region, so the coordinates cannot contradict the clock. |
| `src/clock.rs` | what time the browser thinks it is | The shift lives in the engine, in the compartment of the page that reads it, so a page's own first script already reads the session's date. This module owns the times a human may type, the round trip back out, and the reading a face reports. Monotonic time, pending timers and workers stay on the host clock. |
| `src/route.rs` | what happens to a request before it is sent | A route is a URL glob and one of fulfil, abort or continue. The engine applies it on the channel in the parent process, so a fulfilled request never reaches the network and a page cannot see the edit. This module owns the shape of a route, the refusals, and the order: the table is set whole and the most recently added route is tried first. |
| `src/permission.rs` | what a page is allowed to ask for | Every permission is written into the profile at launch, denied unless the caller granted it, and reported from the same table the flags parse. Gecko reads these at startup, so a live session refuses a change and names the launch argument. |
| `src/mcp.rs`, `src/serve.rs`, `bins/lurien.rs` | faces | Transports. They read the registry; they never match on a verb name and never import `verb::<domain>::`. `serve.rs` also owns the legacy wire names, and each maps onto a verb rather than reimplementing one. It also owns session lifecycle: every named session carries an age and an idle clock, `sessions` reports both, and a session untouched for `LURIEN_SESSION_IDLE_MS` is closed by the reaper rather than leaked. |
| `src/launch.rs`, `resolve.rs`, `goto.rs` | launch contract | Engine required, missing binary is `Err`, captcha is a property of `goto`. |
| `build.rs`, `src/catalog.rs` | vendor catalog | `build.rs` compiles `captcha/kinds/*.toml` into a table; `catalog.rs` turns it into probe selectors and token hooks, addressed by kind only. No Rust source names a vendor, and a test proves it. |
| `src/challenge.rs` | engine handshake | Owns `LURIEN_CHALLENGE`: the catalog JSON, the evidence path and its schema version, the budgets, the claimed kinds, the sampled dynamics decks, and the helper endpoint with its token. Reads evidence rows back; never solves anything itself. |
| `src/token.rs` | tokens for local channels | One minter for the control channel and the helper: 24 bytes of OS entropy as hex. Loopback is not access control, so a channel is private only for as long as its token is unguessable. Reference: `docs/HELPERS.md`. |

`docs/VERBS.md` is generated from the registry. A stale copy fails
`cargo test -p lurien-driver --test verb_registry`.

Enforced by that suite, not by prose: unique verb tokens, a dotted
`domain.verb` alias per verb, documented arguments, required arguments before
optional ones, every verb file registered, no face importing a verb module,
every verb failing closed without an engine, every legacy wire name mapping onto
a verb whose spec accepts the arguments the mapping produces, and no vendor name
in `goto.rs` or `catalog.rs`.

## Inside `engine/additions/challenge/`

Chrome-privileged ES modules loaded from the `lurien-challenge` resource jar and
started by the remote agent. One concern per module, no vendor name anywhere in
the directory.

| Module | Owner |
|---|---|
| `Kinds.sys.mjs` | the closed kind set and which kinds need interaction |
| `Catalog.sys.mjs` | validates the JSON catalog; refuses a binding with no chrome-visible signal or no token hook |
| `Classify.sys.mjs` | sighting to kind, per browsing context, then reduced across contexts |
| `Observer.sys.mjs` | page state keyed by top browsing context; the only writer of the evidence file |
| `ChallengeParent.sys.mjs` `ChallengeChild.sys.mjs` | the actor pair; the child walks closed shadow roots and reports what it can see |
| `Input.sys.mjs` | trusted pointer and key events through the widget event path |
| `Keys.sys.mjs` | timing for typed text: one gap per pair class and one hold per character class, dealt per keystroke from the deck the config carries, classified per digraph |
| `Dynamics.sys.mjs` | deals one sampled path, drag profile or visit plan per interaction from the deck the config carries, in a seeded order |
| `Pow.sys.mjs` | reads a `[work]` table, runs the nonce search in lanes, submits through the address the binding named |
| `PowWorker.js` | one grinding lane: SHA-256 plus the difficulty predicate, off the main thread |
| `Token.sys.mjs` | observes a vendor token arriving on one of four channels, and names which one: a field, a cookie, a storage key in the widget's own origin, or a dotted path into a posted message; read-only |
| `Prelude.sys.mjs` | the visit before the act, dispatched in the top document: settle, pointer path, wheel session, dwell |
| `Snapshot.sys.mjs` | per-context compositor snapshot as PNG |
| `HelperSock.sys.mjs` | loopback-only line protocol to a helper process |
| `Solver.sys.mjs` | the pipeline; every claimed kind ends in a token write or a typed refusal |
| `Geo.sys.mjs` | the shape and range of a position, and the one call that applies it to a top-level context |
| `Clock.sys.mjs` | the wall clock: page source compiled into the page's own compartment, and the shared state a new window reads before its first script |
| `Net.sys.mjs` | routes: the ordered table, glob matching, and the channel work for fulfil, abort and continue |
| `Control.sys.mjs` | the driver's control channel: loopback socket, token per session, one JSON line in and out |
| `Bootstrap.sys.mjs` | reads the config out of the environment, registers the actor, idempotent start |

CI (`.github/workflows/ci.yml`):

- `cargo test --workspace --all-targets`, plus the `lurien-browser` doc tests.
- `cargo tree -p lurien-driver` pulls no scanner crate and no browser sidecar.
- guise without default features does not pull the driver.
- every vendor binding names a kind `_schema.toml` closes, every `pow` binding
  carries a `[work]` table whose algorithm, difficulty format and addresses the
  engine implements, every `slider` binding names the handle a hand grabs, and
  every binding of a claimed kind names a target the engine can resolve rather
  than prose.
- `node lurien/tests/pow_sha256.mjs` checks the grinding worker's digest against
  a reference implementation.
- `cargo test -p lurien-vision` measures a crop the browser rendered, so the
  slider arithmetic is pinned against a real snapshot and not only synthetic
  images.
- `node --check` on `lurien/src/locator.js` and `lurien/src/snapshot.js`: both are
  strings until a browser parses them, so their syntax is gated on its own.

The vendor-name grep over `engine/additions/challenge/` runs in the engine
repository, which owns that directory. `cargo test -p lurien-driver --test
kinds_registry` also runs it whenever the engine tree is present.
