# Verbs

Generated from the registry by `cargo test -p lurien-driver verbs_doc`. Do not edit by hand.

Every verb is reachable identically from the `lurien` CLI, `lurien-mcp`, and `lurien serve`: one spec, three transports.

A `selector` argument accepts a CSS selector or one of the semantic forms in [`SELECTORS.md`](SELECTORS.md). Verbs that act wait for the element; `timeout_ms` bounds that wait.

`batch` runs several of these verbs in one call; its step syntax is in [`BATCH.md`](BATCH.md).

## page

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `back` | - | text | stable | Go back one history entry. |
| `forward` | - | text | stable | Go forward one history entry. |
| `goto` | `url` | json | stable | Navigate. Captcha is automatic (score-class only in v1). No challenge tool. |
| `reload` | - | none | stable | Reload the active document. |
| `screenshot` | `path?` | png | stable | Capture a viewport PNG. Writes the file when path is given. |
| `snapshot` | `format?`, `limit?` | text | stable | The page as roles, names and handles. Handles act as `ref:eN` selectors. |
| `stop` | - | text | stable | Stop loading the active document. |
| `title` | - | text | stable | Document title of the active browsing context. |
| `url` | - | text | stable | Current document URL. |
| `wait` | `ms?` | text | stable | Sleep ms milliseconds. |

## dom

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `click` | `selector`, `timeout_ms?` | text | stable | Click an element, waiting for it to be actionable. |
| `count` | `selector` | json | stable | Number of elements matching a selector, total and visible. |
| `fill` | `selector`, `text`, `timeout_ms?` | text | stable | Focus a field and type text, waiting for it to be actionable. |
| `select` | `selector`, `value`, `timeout_ms?` | text | stable | Select an option by value, waiting for the control to be actionable. |
| `text` | `selector`, `timeout_ms?` | text | stable | Visible text of an element, waiting for it to appear. |
| `type` | `text` | text | stable | Type text into the focused element. |
| `upload` | `selector`, `files`, `timeout_ms?` | text | stable | Attach files to a file input. |

## input

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `mouse` | `x`, `y` | text | stable | Move the pointer to x, y along a human curve. |
| `press` | `key` | text | stable | Press a key in the active context. |
| `scroll` | `dx?`, `dy?` | text | stable | Wheel scroll by dx, dy. |

## frame

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `click-in` | `frame`, `selector` | text | stable | Click a selector inside a named frame, including a cross-origin one. |
| `eval` | `script`, `frame?` | json | stable | Evaluate JavaScript in the main document or a named frame. |
| `frame-tree` | - | json | stable | Browsing-context tree with parent and depth, including OOPIFs. |
| `frames` | - | json | stable | List browsing contexts (main document and every iframe). |
| `type-in` | `frame`, `selector`, `text` | text | stable | Focus a selector inside a named frame and type into it. |

## storage

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `clear-cookies` | - | text | stable | Delete all cookies for the current page. |
| `cookies` | - | json | stable | List all cookies including HttpOnly. |
| `delete-cookie` | `name` | text | stable | Delete one cookie by name. |
| `set-cookie` | `name`, `value`, `domain`, `path?`, `expires?`, `secure?`, `http_only?` | text | stable | Set one cookie via BiDi storage. |

## state

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `state` | - | json | stable | Snapshot cookies, localStorage, and sessionStorage for restore. |
| `state-clear` | - | json | stable | Clear web storage, unregister service workers, and delete caches. |
| `state-set` | `snapshot` | json | stable | Restore a state snapshot: cookies first, then local and session storage. |

## net

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `net` | `limit?`, `headers?` | json | stable | Recent network requests with status and redacted headers. |
| `net-clear` | - | text | stable | Empty the network log so the next read shows only new traffic. |
| `net-tokens` | `limit?` | json | stable | Where credentials appear in captured traffic: header, query, or cookie. |

## dialog

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `dialog` | `action`, `text?`, `frame?` | text | stable | Accept or dismiss the open dialog, optionally with prompt text. |
| `dialog-clear` | - | text | stable | Empty the dialog log so the next read shows only new dialogs. |
| `dialogs` | - | json | stable | Dialogs captured, dialogs still open, and downloads. |

## observe

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `console` | - | json | stable | Console entries and uncaught errors captured by the sensor grid. |
| `signals` | `clear?` | json | stable | DOM-XSS sinks, console, errors, CSP violations, and postMessage traffic. |

## profile

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `as` | `profile`, `dest?`, `headless?` | json | stable | Import a real Firefox profile (cookies, logins, localStorage) and switch to it. |

## context

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `close-context` | `context_id` | text | stable | Close a browser context by id. |
| `contexts` | - | json | stable | List active browser contexts (sessions). |
| `new-context` | `url?` | text | stable | Create a new browser context. Navigates to url if given. |
| `switch-context` | `context_id` | text | stable | Switch to a browser context by id. |

## session

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `batch` | `steps` | json | stable | Run several verbs in one call, stopping at the first failure. |

## intercept

| Verb | Arguments | Output | Stability | Summary |
|---|---|---|---|---|
| `clear-intercepts` | - | text | preview | Clear all request/response interception rules. |
| `delete-header` | `name` | text | preview | Delete a request header override. |
| `get-headers` | - | json | preview | Get the request headers that would be sent on the next navigation. |
| `intercept-request` | `pattern`, `headers?`, `body?` | text | preview | Intercept requests matching a URL pattern with header/body replacement. |
| `intercept-response` | `pattern`, `headers?`, `body?` | text | preview | Intercept responses matching a URL pattern with header/body replacement. |
| `set-extra-headers` | `headers` | text | preview | Set multiple extra request headers from a JSON object string. |
| `set-header` | `name`, `value?` | text | preview | Set a request header override for subsequent navigations. |
