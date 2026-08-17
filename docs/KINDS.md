# Kinds

The engine implements kinds, not vendors. A vendor is a TOML that binds
chrome-visible signals to a closed kind.

## Closed kinds

| Kind | Primitive | Success |
|---|---|---|
| `none` | — | document usable |
| `score` | Token.wait | vendor wrote the named field / cookie |
| `checkbox` | Input.click on catalog target in child BC | Token.wait |
| `visual` | Snapshot → helper → Input.click cells / type | Token.wait |
| `slider` | Snapshot → helper axis → Input.drag | Token.wait |
| `audio` | media → helper STT → Input.type | Token.wait |
| `pow` | helper compute → inject | Token.wait |
| `fail` | — | typed error |

Unknown kind fails closed. A kind with no fixture is a red test.

## Add a vendor

1. Copy `captcha/kinds/_schema.toml` fields into `captcha/kinds/<vendor>.toml`.
2. Name signals (iframe `src` host/path, custom element, cookie, challenge URL).
3. Name one kind from the table.
4. Name the chrome-visible target and the token write (hidden input, `postMessage`, cookie).
5. List always-block sitekeys under integrity.
6. Add a fixture that fires on the positive HTML and rejects the negative.
7. A live scorecard row is required before README may name this vendor.

Do not add a C++ file. A vendor identifier under `engine/additions/challenge/` is red.

## Add a kind

1. Add the name to `captcha/kinds/_schema.toml`.
2. Implement the primitive once (`Input` / `Snapshot` / `HelperSock`).
3. One fixture that fails until the kind is wired.
4. One live-vendor row before README may name it.

The registry test enumerates `_schema.toml` at run time.
