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
| `captcha/vision/` `audio/` `pow/` | helper processes | HelperSock protocol | foxdriver, page, guise |
| `docs/` | plan + this file | — | — |

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
| `src/mcp.rs`, `src/serve.rs`, `bins/lurien.rs` | faces | Transports. They read the registry; they never match on a verb name and never import `verb::<domain>::`. `serve.rs` also owns the legacy wire names, and each maps onto a verb rather than reimplementing one. |
| `src/launch.rs`, `resolve.rs`, `goto.rs` | launch contract | Engine required, missing binary is `Err`, captcha is a property of `goto`. |
| `build.rs`, `src/catalog.rs` | vendor catalog | `build.rs` compiles `captcha/kinds/*.toml` into a table; `catalog.rs` turns it into probe selectors and token hooks, addressed by kind only. No Rust source names a vendor, and a test proves it. |
| `src/challenge.rs` | engine handshake | Owns `LURIEN_CHALLENGE`: the catalog JSON, the evidence path, the budget, the claimed kinds, and the approach path sampled from guise. Reads evidence rows back; never solves anything itself. |

`docs/VERBS.md` is generated from the registry. A stale copy fails
`cargo test -p lurien-browser --test verb_registry`.

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
| `Token.sys.mjs` | observes a vendor token appearing in a field or a cookie; read-only |
| `Snapshot.sys.mjs` | per-context compositor snapshot as PNG |
| `HelperSock.sys.mjs` | loopback-only line protocol to a helper process |
| `Solver.sys.mjs` | the pipeline; every claimed kind ends in a token write or a typed refusal |
| `Bootstrap.sys.mjs` | reads the config out of the environment, registers the actor, idempotent start |

CI (`.github/workflows/ci.yml`):

- `cargo test --workspace --all-targets`, plus the `lurien-browser` doc tests.
- `cargo tree -p lurien-browser` pulls no scanner crate and no browser sidecar.
- guise without default features does not pull the driver.
- every vendor binding names a kind `_schema.toml` closes.

The vendor-name grep over `engine/additions/challenge/` runs in the engine
repository, which owns that directory. `cargo test -p lurien-browser --test
kinds_registry` also runs it whenever the engine tree is present.
