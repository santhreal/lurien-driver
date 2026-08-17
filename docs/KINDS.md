# Kinds

The engine implements kinds, not vendors. A vendor is a TOML that binds
chrome-visible signals to a closed kind.

## Closed kinds

| Kind | Primitive | Success |
|---|---|---|
| `none` | — | document usable |
| `score` | Token.wait | vendor wrote one of the named channels |
| `checkbox` | Input.click on catalog target in child BC | Token.wait |
| `visual` | Snapshot on the grid → helper → Input.clickCell per named tile | Token.wait |
| `slider` | Snapshot on the puzzle → helper axis → Input.drag on the handle | Token.wait |
| `audio` | media → helper STT → Input.type | Token.wait |
| `pow` | nonce search in browser worker lanes → the `[work]` submit address | Token.wait |
| `fail` | — | typed error |

Unknown kind fails closed. A kind with no fixture is a red test: one classification
fixture named after the kind, and, for a kind this build claims, a scorecard row
naming a script in `lurien/tests/` that solves it end to end. A claim whose script
is missing or renamed is red rather than quietly unproven.

A kind whose answer is a string types it, and typing is timed. `Keys.sys.mjs` plans
a gap per pair of keys and a hold per key from the deck the session shipped, so
`pow`, `visual` and `audio` all reach a field through the same rhythm. A primitive
that assigns `value` instead is a solve a page can measure and reject.

A `visual` grid is answered with tile indices. The context that laid the grid out
measures it, the parent crops exactly the box it reports and asks the vision helper
which tiles match the widget's question, and the same context re-locates each named
tile to click it, so no coordinate crosses a process boundary. The helper needs an
object detector on disk and refuses the request by name without one, which
`docs/HELPERS.md` describes.

## Add a vendor

1. Copy `captcha/kinds/_schema.toml` fields into `captcha/kinds/<vendor>.toml`.
2. Name signals (iframe `src` host/path, custom element, cookie, challenge URL).
3. Name one kind from the table.
4. Name the chrome-visible target and where the vendor writes its token. Four
   channels: `token_inputs` (form fields), `token_cookies`, `token_storage` (a key
   in the widget's own origin), and `token_messages` (a dotted path into a payload
   posted to the page, `detail.token`). Name every channel the vendor uses; one is
   enough to load the binding.
   A `slider` also names `handle`: the element a hand grabs, which is not the
   element being measured. A `pow` also carries a `[work]` table, and a `visual` a
   `[grid]` table naming the question, the tiles, and the submit control. A
   `visual` binding with no `[grid]` table is still recognized and then refused by
   name, rather than bound to guessed class names.
   A target on a kind this build claims must be a form the engine resolves:
   `first checkbox in this BC`, `first canvas in this BC`,
   `first draggable in this BC`, `role:<name>`, or a CSS selector. Prose is for a
   kind nothing acts on yet.
5. List always-block sitekeys under integrity.
6. Add a fixture that fires on the positive HTML and rejects the negative.
7. A live scorecard row is required before README may name this vendor.

Do not add a C++ file. A vendor identifier under `engine/additions/challenge/` is red.

## Add a kind

Nine places, in this order. Each one has a test that turns red while it is missing,
which is the reason the list is worth following rather than remembering.

| Step | Where | Red without it |
|---|---|---|
| 1. Name the kind | `captcha/kinds/_schema.toml` | nothing yet: the schema is the source every later law reads |
| 2. Place it in the order that gates a page | `KIND_SEVERITY` in `engine/additions/challenge/Kinds.sys.mjs` | `engine_package.rs::every_kind_has_a_place_in_the_order_that_gates_a_page` |
| 3. Implement the primitive once | `Input`, `Snapshot`, `HelperSock` or `Pow` in `engine/additions/challenge/` | its own fixture, step 5 |
| 4. Bind a vendor to it | `captcha/kinds/<vendor>.toml` | `kinds_registry.rs::every_vendor_toml_names_a_closed_kind` and `every_interactive_kind_has_at_least_one_vendor_binding` |
| 5. One fixture | `captcha/kinds/fixtures/<kind>.html` | `kinds_registry.rs::every_closed_kind_has_a_fixture` |
| 6. One runnable proof | `lurien/tests/e2e_<kind>.sh` | `kinds_registry.rs::every_claimed_kind_names_a_proof_that_can_be_run_again` |
| 7. A dated scorecard row | `docs/bench-results/challenge-scorecard.md` | `kinds_registry.rs::every_claimed_kind_has_a_dated_scorecard_row` and `every_claim_names_the_build_that_proved_it` |
| 8. Claim it | `CLAIMED_KINDS` in `lurien/src/challenge.rs` | `challenge.rs::every_claimed_kind_is_a_kind_the_catalog_can_present`; until this step the engine refuses the kind with a typed error instead of acting |
| 9. Give it a budget | `KIND_BUDGET_MS` in `lurien/src/challenge.rs` | `challenge.rs::a_page_is_watched_for_as_long_as_the_engine_was_given`; a kind with no row of its own is bounded by the page budget |

Two things the fixture has to do, because a fixture is the only thing standing
between a claim and a page nobody tested:

- Fail until the kind is wired, and refuse the shape a scripted solver produces
  rather than only checking the answer. A widget that accepts an untrusted click is
  not evidence about a solver that dispatches one.
- Be reachable from a script in `lurien/tests/` with at least one phase that must be
  refused. A claim proven only by a page somebody once visited is not replayable.

A live-vendor row is required before any document names a vendor for the kind. It
never replaces the fixture row: a vendor can change overnight, and the fixture is
what says whether the mechanism still works.

The registry test enumerates `_schema.toml` at run time, so a kind added there and
nowhere else turns the suite red at step 2 rather than shipping half-wired.
