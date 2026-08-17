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

A row names the build that produced it: the browser it ran against and the driver
that drove it. A claim whose row names an older browser, or an older driver minor
version, is refused by `lurien/tests/kinds_registry.rs` until the run is repeated
and the row rewritten. A proof belongs to a build, not to a feature.

| Kind | Class | Date | Engine | Driver | Result | Evidence |
|---|---|---|---|---|---|---|
| `none` | live vendor | 2026-08-16 | 150.0.2-beta.25 | 0.1.0 | document usable, no widget seen | `lurien --headless goto https://example.com/` reports `{"kind":"none"}` |
| `score` | live vendor | 2026-08-16 | 150.0.2-beta.25 | 0.1.0 | vendor wrote its token, no interaction | managed deployment at `demo.turnstile.workers.dev` reports `{"kind":"score"}`; the observer folded to `none` because the token was already written |
| `checkbox` | fixture | 2026-08-17 | 150.0.2-beta.25 | 0.1.0 | solved in 7088 ms across 2 contexts, `via: field`; most of that is the visit the vendor is owed before the click | `lurien/tests/e2e_challenge.sh`; the widget refuses an untrusted click and a click with fewer than three trusted pointer moves |
| `pow` | fixture | 2026-08-17 | 150.0.2-beta.25 | 0.1.0 | 4 hex zeros cleared in 143 hashes across 7 lanes, typed and accepted, whole page 9689 ms, `via: field` | `lurien/tests/e2e_pow.sh`; the page mints a fresh challenge and a random difficulty per load and accepts only a digest it verified itself, typed key by key with trusted events. `lurien/tests/pow_sha256.mjs` pins the worker digest against a reference implementation |
| `slider` | fixture | 2026-08-17 | 150.0.2-beta.25 | 0.1.0 | notch measured from pixels, 259 px travel dragged in 20 moves, accepted, whole page 4791 ms, `via: field` | `lurien/tests/e2e_slider.sh`; the notch moves every load, the widget refuses a travel with fewer than eight moves, one step size, no correction, or under 80 ms, and the same run with an evenly spaced profile is refused. `captcha/vision/tests/real_crop.rs` pins the measurement against a crop the browser rendered |
| `fail` | n/a | 2026-08-16 | 150.0.2-beta.25 | 0.1.0 | typed error, no fabricated token | `tests/verb_fail_closed.rs` |

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

The `prelude` fixture rules out a solve that clicks a page nobody read:

- The widget writes its token only when the parent reports at least six trusted
  `mousemove` events outside the widget rectangle, at least one trusted `wheel`,
  and a non-zero `scrollY`. Moves inside the rectangle are the approach, not
  reading, and are not counted.
- A pointer inside the widget within 300 ms of load is refused, so a session that
  lands on the widget at load time fails on timing alone.
- The evidence crosses the origin boundary by `postMessage` from the parent,
  because the widget cannot see the page's own events; a page-script solver cannot
  forge it, since every counted event had `isTrusted` true.
- The second phase ships an empty prelude, dispatches the same trusted click, and
  requires no token. Measured on 2026-08-17 against engine 150.0.2-beta.25: 14
  moves and 6 wheel events cleared the widget in 8.6 s, and the unread run was
  refused.

The `dynamics` fixture rules out one curve replayed for every interaction:

- The widget is solved twice in one session. Each verdict row names the deck entry
  the interaction took, and two entries in one session must differ; the engine
  never deals the same entry twice in a row.
- A third and fourth solve run under the same `LURIEN_DYNAMICS_SEED` and must deal
  the same two entries in the same order, so the motion of a scored solve can be
  reproduced.
- The widget records every trusted move in its own coordinates and refuses a click
  that arrived with fewer than three, so each visit is a real approach through the
  event path. The record is a sample, not the dispatched curve: `mousemove` is
  coalesced, which is why the dealt entry is what the test compares.
- Measured on 2026-08-17 against engine 150.0.2-beta.25: entries 6 then 4, replayed
  as 6 then 4, four token writes, 24 trusted moves recorded per visit.
- Three mutations were checked. A constant deck index made both solves in a session
  take entry 0 and the test red; an unseeded (`Math.random`) order broke the replay
  and the test red; shipping one `trajectory` instead of a deck left every row with
  no entry to name and the test red.

Every row above is only worth reading if the reader and the writer agree on what a
row means. Each row carries `v`, its schema version, and the driver refuses a row
it does not read:

- Measured on 2026-08-17 against engine 150.0.2-beta.25: the `checkbox` phase
  passed with `"v":1` on both the `taken` row and the verdict row.
- An engine that appends no `v` was built and run against the same driver:
  navigation failed with `the engine writes evidence schema 0 and this build reads
  1. Reinstall the engine with install.sh so the driver and the browser match.`
  rather than reporting the page as an unsolved challenge.
- A driver that ignores `v` read a stale row as a `checkbox` pass and turned the
  unit test red. Bumping the engine constant alone turned the cross-repository
  version law red.

The `budget` fixture rules out one flat timeout for every kind. Its widget takes a
trusted click and never writes a token, which is what a vendor that is unhappy or
broken looks like:

- The same page is visited twice, differing only in whether the config names a
  budget for `checkbox`. Measured on 2026-08-17 against engine 150.0.2-beta.25: the
  run with a 1500ms kind budget refused after 4368ms, the run on the flat 8000ms
  page budget refused after 11004ms, and each refusal named the number it was given.
- Both runs read the page for about the same time (2856ms and 2838ms). Reading is a
  property of the page, so a short kind budget must not buy a shorter visit.
- Three mutations were checked. A solver that ignores `kind_budget_ms` refused both
  runs on the page budget and the test red; paying for the visit out of the kind
  budget cut the read to 558ms and the test red; a driver that ships no table turned
  the unit test that holds the table against the claimed kinds red.

The `classify` fixture rules out a page solved at the wrong widget. It holds a
checkbox and a slider, both cross-origin, and the checkbox binding matches two
signals against the slider's one, so signal count and severity disagree:

- The puzzle frame gets its source 250 ms after paint, the way a vendor loader
  that chooses a widget does, so the cheap widget is the first context to report.
- Phase one gives each binding elements that exist only inside its own frame, so
  every context holds one candidate and the kind is decided when the contexts are
  merged. Phase two gives both bindings elements of the top document alone, so one
  context holds both candidates and no merge can repair a wrong choice.
- Measured on 2026-08-17 against engine 150.0.2-beta.25 and driver 0.1.0: the page
  was reported as `slider`, solved by dragging 170.0 px in 20 moves, and the row
  named both widgets, the slider first.
- Five mutations were checked. Reducing across contexts by signal count solved the
  checkbox and left the puzzle standing, and the test red; classifying inside one
  context by signal count reported that context as `checkbox`, and the test red;
  acting on the first sighting rather than opening the settle window took the page
  as `checkbox`, and the test red; a settle window of nothing did the same; a row
  that names only the widget the engine acted on turned the second claim red.

The `token channels` fixtures rule out a solve that is only ever seen in a form
field. Both widgets clear the same way, by a trusted click after real motion, and
then answer somewhere a field poller never looks:

- `challenge_storage_child.html` writes `localStorage["fixture-token"]` in its own
  origin. The page cannot read that key, so only the widget's context can report it,
  and the fixture clears the key on load so a stale value cannot pass for a solve.
- `challenge_message_child.html` posts `{detail: {token}}` to the page once and keeps
  no copy. There is nothing left to poll, so the observation has to be a listener
  installed before the click, in the context the payload was posted to.
- Each phase requires `via` to name its own channel, which is what rules out a pass
  taken from a field or cookie the run happened to find.
- A third phase names `nope.token`, a path the page never writes, under a 26000ms
  kind budget: the click lands, nothing arrives, and the run is refused with an error
  naming that budget. It is required to end after 26 s and before 45 s.
- Measured on 2026-08-17 against engine 150.0.2-beta.25 and driver 0.1.0: `via`
  `storage` in 6988 ms and `via` `message` in 7563 ms, both across 2 contexts, and
  the unwritten channel refused in 29561 ms.
- Five mutations were checked. A child that never reads a storage key turned phase
  one red; a child that skips installing the message listener turned phase two red;
  a wait that asks only the widget's own context and not the top document turned
  phase two red, because the payload is delivered to the page; a payload path read
  whole instead of split on `.` turned phase two red; and a driver that waits a
  fixed 25 s for a verdict instead of the budgets it granted reported the refused
  page as `none` and exited zero, which turned phase three red.
