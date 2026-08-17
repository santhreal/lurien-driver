# Challenge scorecard

Every kind this engine claims, and the run that proves it. A kind with no row
here is refused at run time with a typed error, and no document may name a vendor
whose kind has no row.

Two row classes, kept apart because they prove different things:

- **fixture**: a widget served from a second host, in its own browsing context,
  that writes its token only for a trusted event sequence. Proves the engine
  reached a cross-origin context and drove it through the real event path.
- **live vendor**: a production page of a real deployment. Proves the same chain
  against a widget that is also looking for us. Test keys do not qualify.

## Claimed kinds

| Kind | Class | Date | Engine | Result | Evidence |
|---|---|---|---|---|---|
| `none` | live vendor | 2026-08-16 | 150.0.2-beta.25 | document usable, no widget seen | `lurien --headless goto https://example.com/` reports `{"kind":"none"}` |
| `score` | live vendor | 2026-08-16 | 150.0.2-beta.25 | vendor wrote its token, no interaction | managed deployment at `demo.turnstile.workers.dev` reports `{"kind":"score"}`; the observer folded to `none` because the token was already written |
| `checkbox` | fixture | 2026-08-17 | 150.0.2-beta.25 | solved in 98 ms across 2 contexts, `via: field` | `lurien/tests/e2e_challenge.sh`; the widget refuses an untrusted click and a click with fewer than three trusted pointer moves |
| `fail` | n/a | 2026-08-16 | 150.0.2-beta.25 | typed error, no fabricated token | `tests/verb_fail_closed.rs` |

## Not claimed

`visual`, `slider`, `audio`, and `pow` have primitives in the engine and no row
here, so the solver refuses them:

```
kind visual is not claimed by this build; it is refused rather than reported as a pass
```

Each needs a local helper (`HelperSock`) plus a row above before it will run. The
refusal is deliberate: a solver that reports success on a kind it cannot finish
teaches its caller to trust a number that means nothing.

## What the fixture rules out

The `checkbox` row is a fixture rather than a live deployment, so it proves the
mechanism and not yet the arms race. It does rule out the ways a page-script or
outside-driver solver fails:

- The token is written only when `event.isTrusted` is true for both `mousedown`
  and `click`, so `element.click()` and `dispatchEvent` are refused.
- At least three trusted `mousemove` events must precede the press, so a
  teleport-and-click is refused.
- The widget is on a different host than the page, so the parent document cannot
  read it: the fixture records that `contentDocument` access throws.
- The catalog names the target as "first checkbox in this BC" with no coordinates,
  so a hardcoded offset would not find it.
