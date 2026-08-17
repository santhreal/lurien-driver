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
| `pow` | fixture | 2026-08-17 | 150.0.2-beta.25 | 4 hex zeros cleared in 172920 hashes across 7 lanes, submitted and accepted in 275 ms, `via: field` | `lurien/tests/e2e_pow.sh`; the page mints a fresh challenge and a random difficulty per load and accepts only a digest it verified itself, typed key by key with trusted events. `lurien/tests/pow_sha256.mjs` pins the worker digest against a reference implementation |
| `slider` | fixture | 2026-08-17 | 150.0.2-beta.25 | notch measured from pixels, 117 to 175 px travel over three runs, dragged in 20 moves, accepted in 704 ms, `via: field` | `lurien/tests/e2e_slider.sh`; the notch moves every load, the widget refuses a travel with fewer than eight moves, one step size, no correction, or under 80 ms, and the same run with an evenly spaced profile is refused. `captcha/vision/tests/real_crop.rs` pins the measurement against a crop the browser rendered |
| `fail` | n/a | 2026-08-16 | 150.0.2-beta.25 | typed error, no fabricated token | `tests/verb_fail_closed.rs` |

## Not claimed

`visual` and `audio` have primitives in the engine and no row here, so the solver
refuses them:

```
kind visual is not claimed by this build; it is refused rather than reported as a pass
```

Each needs a local helper (`HelperSock`) plus a row above before it will run. The
refusal is deliberate: a solver that reports success on a kind it cannot finish
teaches its caller to trust a number that means nothing.

## What the fixtures rule out

The `checkbox`, `pow`, and `slider` rows are fixtures rather than live
deployments, so they prove the mechanism and not yet the arms race. The `checkbox`
fixture rules out the ways a page-script or outside-driver solver fails:

- The token is written only when `event.isTrusted` is true for both `mousedown`
  and `click`, so `element.click()` and `dispatchEvent` are refused.
- At least three trusted `mousemove` events must precede the press, so a
  teleport-and-click is refused.
- The widget is on a different host than the page, so the parent document cannot
  read it: the fixture records that `contentDocument` access throws.
- The catalog names the target as "first checkbox in this BC" with no coordinates,
  so a hardcoded offset would not find it.

The `pow` fixture rules out the ways a computed answer can be faked:

- The challenge string and the difficulty are minted per load from
  `crypto.getRandomValues`, so a recorded nonce is worth nothing on the next run.
- The page verifies the digest itself with `crypto.subtle`, so an engine that
  computes SHA-256 incorrectly is refused rather than believed.
- Every `keydown` that built the response must be trusted and there must be as
  many of them as the response has characters, so assigning `input.value` or
  pasting the answer is refused.
- The engine reads the challenge from the page's own global through the catalog
  address, so a binding that names the wrong address is refused with the address
  it could not read rather than falling back to a guess.

The `slider` fixture rules out the ways a drag can be faked:

- The notch is minted per load from `crypto.getRandomValues`, so a constant
  offset lands outside the three-pixel tolerance.
- The answer is read from the rendered image only. The page keeps the notch
  column in a closure and exposes no global, so there is nothing to read but
  pixels, and the crop comes from the widget's own browsing context, which the
  parent document cannot reach.
- Every pointer event must be trusted, so `dispatchEvent` and a driver-side
  `dragAndDrop` are refused.
- The travel must carry at least eight moves, a step-size spread of at least two
  pixels, at least one direction reversal, and at least 80 ms. A straight travel
  at constant speed is refused, which the third phase of the test asserts by
  replacing the sampled profile with ten even steps and requiring no token.
- The binding names the puzzle as "first canvas in this BC" and the handle as
  "first draggable in this BC", so no coordinate and no vendor class name is
  involved in finding either.
