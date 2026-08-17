# lurien

A Firefox you drive like Playwright. Persona is coherent from TLS to the
pixel. Challenges are a property of `goto`, classified by kind, cleared inside
the browser.

Three names, one product:

| Name | What it is |
|---|---|
| **lurien-browser** | The browser: a Gecko fork that paints the page, holds the persona in every realm, and clears challenges in the widget's own browsing context. [`santhreal/lurien-browser`](https://github.com/santhreal/lurien-browser), MPL, its own repository and release cadence. |
| **lurien-driver** | The Rust crate you drive it with: `lurien::Browser`, the `lurien` CLI, `lurien-mcp`, `lurien serve`. This repository. |
| **lurien** | What you type, and the name of the installed browser binary. |

The driver requires the browser. There is no Firefox fallback and no silent
degradation: a missing browser is an error that names the install path.

v1 is Linux x86_64, headful. Playwright talks to the installed browser via
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

`lurien serve` names each session with a `browser_context_id`, reports every open
one with its age, state and idle time, and closes a session nobody has touched for
`LURIEN_SESSION_IDLE_MS` (default 15 minutes, `0` to disable), so a client that
dies does not leave a browser running.

## Selectors

A `selector` is a CSS selector or one of five semantic forms:

```
lurien click 'role:button=Log in'
lurien click 'text:Continue to checkout'
lurien fill 'label:Email' me@example.com
lurien click 'placeholder:you@example.com'
lurien click 'testid:submit'
```

A verb that acts waits for its element, so a button the page adds after the
navigation needs no explicit wait. A semantic form that fits two visible buttons
is refused with both named, rather than pressing the first one. When nothing
matches, the error lists what is on screen.

`snapshot` reports the page as roles, names and handles rather than source, and a
handle acts:

```
- button "Log in" [ref=e2]
```

```
lurien click ref:e2
```

Reference: [`docs/SELECTORS.md`](docs/SELECTORS.md).

Several verbs in one call, stopping at the first failure:

```
lurien batch "goto url=https://example.com/login" \
             "fill selector=label:Email text=me@example.com" \
             'click selector="role:button=Log in"'
```

Reference: [`docs/BATCH.md`](docs/BATCH.md).

## Challenges

Classification and clearing happen inside the engine, in the browsing context
that owns the widget, using the same event path a hand produces. No sidecar
browser, no third-party solving API, no vendor name compiled into the engine: a
vendor is a row in `captcha/kinds/*.toml` that names a kind, a signal, a target,
and the token to watch for.

`goto` reports the kind it saw and, when the engine acted, what it observed.
A kind is claimed only when [`docs/bench-results/challenge-scorecard.md`](docs/bench-results/challenge-scorecard.md)
carries a dated row for it; an unclaimed kind is refused rather than reported as
a pass, and a test enforces that. Claimed today: `none`, `score`, `checkbox`,
`pow`, `slider`, `visual`. A proof of work is computed in the browser itself, in
worker lanes, with no helper process and no external service. A slider is measured
from the rendered image by a loopback helper and dragged with sampled dynamics. A
tile grid is cropped in the widget's own context, read by the same helper with a
local object detector, and answered by index, so no coordinate and no picture
leaves the machine; that claim is a fixture grid, and a live vendor grid is refused
by name until a binding can open it and answer its rounds, which the scorecard
measures. Every act is preceded by a visit: the page is settled, scrolled and
crossed by the pointer from a plan sampled per session, because a page nobody read
scores as a machine however trusted the click is.

Honest leaks: matched-host Linux Firefox only. Cross-OS fonts/WebGL/WebGPU,
inert `canvas:seed`, and proxyless TTL remain.

## Install

v1 does not download Gecko. Wire a local build:

```
./install.sh [/path/to/engine/build]
```

or set `LURIEN_BIN`. Missing engine is an error. There is no Firefox fallback.
Rust: `cargo add lurien-driver`. Bins: `lurien`, `lurien-mcp`.

This tree is one Cargo workspace, so a clone builds with `cargo build` and tests
with `cargo test --workspace`. Tests that need the engine binary skip loud. The
engine itself is a separate repository (a Gecko fork, MPL) and is not vendored
here.

Plan: [`docs/PLAN.md`](docs/PLAN.md).
What this driver does differently, and what proves each of it: [`docs/COMPARISON.md`](docs/COMPARISON.md).
Selectors and the wait: [`docs/SELECTORS.md`](docs/SELECTORS.md).
Several verbs in one call: [`docs/BATCH.md`](docs/BATCH.md).
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
| `lurien-driver` | Public face. `Browser::launch`, `lurien` CLI, `lurien serve`, `lurien-mcp` | Needs the engine binary |
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
lurien-driver/
  Cargo.toml                workspace root
  README.md                 this file
  lurien/                   crate lurien-driver
  lurien/kinds              symlink to captcha/kinds, so the catalog ships
  engine/                   lurien-browser, separate repository
  guise/                    persona
  guise-profiles/ guise-pacing/ guise-choice/ guise-oracle/
  foxdriver/                BiDi
  echo/                     test reflector
  captcha/kinds/            vendor TOML
  docs/                     plan + maintainability
  install.sh
```

