# Kinds

The engine implements kinds, not vendors. A vendor is a TOML that binds
chrome-visible signals to a closed kind.

## Closed kinds

| Kind | Primitive | Success |
|---|---|---|
| `none` | — | document usable |
| `score` | Token.wait | vendor wrote one of the named channels |
| `checkbox` | Input.click on catalog target in child BC | Token.wait |
| `visual` | Snapshot → helper → Input.click cells / type | Token.wait |
| `slider` | Snapshot on the puzzle → helper axis → Input.drag on the handle | Token.wait |
| `audio` | media → helper STT → Input.type | Token.wait |
| `pow` | nonce search in browser worker lanes → the `[work]` submit address | Token.wait |
| `fail` | — | typed error |

Unknown kind fails closed. A kind with no fixture is a red test.

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
   element being measured. A `pow` also carries a `[work]` table.
   A target on a kind this build claims must be a form the engine resolves:
   `first checkbox in this BC`, `first canvas in this BC`,
   `first draggable in this BC`, `role:<name>`, or a CSS selector. Prose is for a
   kind nothing acts on yet.
5. List always-block sitekeys under integrity.
6. Add a fixture that fires on the positive HTML and rejects the negative.
7. A live scorecard row is required before README may name this vendor.

Do not add a C++ file. A vendor identifier under `engine/additions/challenge/` is red.

## Add a kind

1. Add the name to `captcha/kinds/_schema.toml`.
2. Implement the primitive once (`Input` / `Snapshot` / `HelperSock` / `Pow`).
3. One fixture that fails until the kind is wired, and which refuses the shape a
   scripted solver produces rather than only checking the answer.
4. A dated scorecard row before the build claims the kind, and a live-vendor row
   before a document names a vendor for it.

The registry test enumerates `_schema.toml` at run time.
