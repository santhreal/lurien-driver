# lurien

A Firefox you drive like Playwright. Persona is coherent from TLS to the
pixel. Captchas are a property of `goto`, classified by kind, solved inside
the engine.

crates.io / GitHub: **lurien-browser**. Spoken / CLI / MCP: **lurien**.

v1 is Linux x86_64, headful. Playwright talks to the installed engine via
`executablePath`. There is no PyPI package.

```
firefox.launch({ executablePath: "~/.local/share/lurien/lurien" })
```

or

```json
{ "mcpServers": { "playwright": { "command": "lurien-mcp" } } }
```

The verb surface is one registry with three transports. The CLI, the MCP server,
and the HTTP face read the same specs, so none can offer a verb or an argument the
others lack:

```
lurien goto https://example.com     # CLI
lurien-mcp                          # MCP over stdio
lurien serve                        # HTTP, many named sessions
```

Verb reference: [`docs/VERBS.md`](docs/VERBS.md), generated from the registry.

## Challenges

Classification and clearing happen inside the engine, in the browsing context
that owns the widget, using the same event path a hand produces. No sidecar
browser, no third-party solving API, no vendor name compiled into the engine: a
vendor is a row in `captcha/kinds/*.toml` that names a kind, a signal, a target,
and the token to watch for.

`goto` reports the kind it saw and, when the engine acted, what it observed.
A kind is claimed only when [`docs/bench-results/challenge-scorecard.md`](docs/bench-results/challenge-scorecard.md)
carries a dated row for it; an unclaimed kind is refused rather than reported as
a pass, and a test enforces that. Claimed today: `none`, `score`, `checkbox`.

Honest leaks: matched-host Linux Firefox only. Cross-OS fonts/WebGL/WebGPU,
inert `canvas:seed`, and proxyless TTL remain.

## Install

v1 does not download Gecko. Wire a local build:

```
./install.sh [/path/to/engine/build]
```

or set `LURIEN_BIN`. Missing engine is an error. There is no Firefox fallback.
Rust: `cargo add lurien-browser`. Bins: `lurien`, `lurien-mcp`.

This tree is one Cargo workspace, so a clone builds with `cargo build` and tests
with `cargo test --workspace`. Tests that need the engine binary skip loud. The
engine itself is a separate repository (a Gecko fork, MPL) and is not vendored
here.

Plan: [`docs/PLAN.md`](docs/PLAN.md).
How to add a vendor or a kind: [`docs/KINDS.md`](docs/KINDS.md).
Who may import whom: [`docs/TREE.md`](docs/TREE.md).
Build / rebase: [`docs/ENGINE.md`](docs/ENGINE.md), [`docs/REBASE.md`](docs/REBASE.md).

## Goal

One installable browser. Not an agent loop. Not a CapSolver wrapper.

- Engine (MPL Gecko fork) paints the page and classifies challenges.
- Persona (`guise`) is one seed: UA, TLS, headers, mouse.
- Driver (`foxdriver`) is BiDi. Playwright-shaped verbs.

A vendor is a TOML kind binding, not engine source. A new kind is a schema
row + one primitive + a fixture.

## Crates

Each crate below is usable on its own. The product is the composition.
Crate *names* stay. Paths are under this folder.

| Crate | Role | Alone |
|---|---|---|
| `lurien-browser` | Public face. `Browser::launch`, `lurien` CLI, `lurien serve`, `lurien-mcp` | Needs the engine binary |
| `guise` | Persona compiler + (optional) launch | Yes. Default features are data: fingerprint, human timing, HTTP headers. Feature `browser` pulls foxdriver |
| `guise-profiles` | UA / profile ids | Yes. Pure constants. Scanners already depend on this |
| `guise-pacing` | Retry backoff, jitter, Retry-After | Yes. No browser |
| `guise-choice` | Sampling / stealth-safe tokens | Yes. No browser |
| `guise-oracle` | Surface taxonomy for the differential oracle | Yes. Types only |
| `runtime-foxdriver` | Firefox BiDi `Page` | Yes. Stock or patched Firefox. No persona |
| `guise-echo` | Local TLS+H2+TCP reflector | Yes. Test fixture, not a product |
| engine (not a crate) | Patched Gecko. MPL. Separate process | Yes as a binary. Never linked into the MIT crates |

`guise` without `browser` does not launch Firefox. scanclient, sear,
httpdet, interactsh, netshift, bugscope, secmatch, and headless keep
using `guise-*` that way.

## Layout

```
lurien-browser/
  Cargo.toml                workspace root
  README.md                 this file
  lurien/                   crate lurien-browser
  lurien/kinds              symlink to captcha/kinds, so the catalog ships
  engine/                   Gecko fork, separate repository
  guise/                    persona
  guise-profiles/ guise-pacing/ guise-choice/ guise-oracle/
  foxdriver/                BiDi
  echo/                     test reflector
  captcha/kinds/            vendor TOML
  docs/                     plan + maintainability
  install.sh
```

