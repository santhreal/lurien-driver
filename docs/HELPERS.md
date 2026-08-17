# Helper protocol

A helper is a local process that answers one perception question. The browser
sends a crop and gets back a number. It is a separate process because a vision
model does not belong in `libxul`, and because a process that only ever sees a
300x65 crop cannot leak a session.

`lurien-vision` is the helper that ships with this tree. Any process in any
language can take its place by speaking this protocol.

## Transport

One TCP connection per request, on loopback only. One JSON object on one line in,
one JSON object on one line out, then the connection closes. No keep-alive, no
session state.

The helper refuses to bind a non-loopback address, and the browser refuses to
address one.

## Version

The current protocol version is `1`. Every request names it as `v`.

The helper and the browser are separate builds, wired together by whoever starts
them, so a mismatched pair is a normal operational state. A request naming another
version is refused with one line instead of being read field by field:

```json
{"error":"this helper speaks protocol 1 and the request names 2; run the helper and the browser from one build"}
```

## Authentication

Every request carries `token`, and the helper answers only for the token it was
started with. Loopback is not access control: every process on the host can
connect to the port, including whatever a page just got the browser to run, and an
unauthenticated helper is a perception service anything local can queue work on and
read the answers from.

The token is minted per session by whoever starts both processes: 24 bytes of
entropy as hex. The driver mints one in `lurien::challenge::HelperEndpoint`. Start
the helper with the same value:

```
lurien-vision --port 0 --token 6f1c…            # or LURIEN_HELPER_TOKEN=6f1c…
```

The comparison is constant time. A helper started with no token answers nothing:

```json
{"error":"this helper was started with no session token, so it answers nothing; pass --token, or LURIEN_HELPER_TOKEN, the same value the session names"}
```

A request whose token does not match is refused without the crop being looked at:

```json
{"error":"the token does not match this helper's session; name the same token in the session's helper configuration"}
```

## Request

```json
{
  "v": 1,
  "token": "6f1c9a4b…",
  "kind": "slider",
  "task": "axis",
  "png": "<base64 PNG>",
  "width": 300,
  "height": 65
}
```

| Field | Meaning |
|---|---|
| `v` | protocol version, `1` |
| `token` | this session's helper token |
| `kind` | challenge kind. `lurien-vision` answers `slider` |
| `task` | task within the kind. `lurien-vision` answers `axis` |
| `png` | the crop, PNG, standard base64 |
| `width`, `height` | crop size in CSS pixels, which differs from the PNG size when the snapshot was taken at a scale |

The request carries no URL, no cookies, no page and no session. That is the whole
security argument for running perception outside the browser, and it holds only
while the request stays this small.

## Reply

An answer carries either a measurement or an error, never both and never a silent
zero:

```json
{"dx":149.0,"dy":0.0,"confidence":494.2}
{"error":"png: the crop holds no puzzle edge pair"}
```

| Field | Meaning |
|---|---|
| `dx` | travel along the axis, in CSS pixels |
| `dy` | travel across the axis, `0` for a slider |
| `confidence` | the measurement's own score, for a caller that logs it |
| `error` | why there is no answer |

The browser applies `dx` through its own trusted-input path with a drag profile it
sampled. The helper never touches the page.

## Configuring the browser

The session names the helper in `LURIEN_CHALLENGE`:

```json
"helper": { "host": "127.0.0.1", "port": 41231, "token": "6f1c9a4b…" }
```

A helper named without a token is refused at construction rather than used, on both
sides. Kinds that need a helper are refused when none is configured:

```
kind visual needs a local helper and none is configured
```

## Writing another helper

1. Bind a loopback port. Print the port if the caller passed `0`.
2. Take a session token on the command line or in the environment. Refuse to start
   without one.
3. Read one line, parse it, check `v`, compare `token` in constant time, answer,
   close.
4. Bound the line you accept. `lurien-vision` reads at most 32 MiB, which bounds
   the crop a caller can push.
5. Answer an unreadable request with `{"error": …}` rather than closing: a session
   waiting on a helper spends its budget on silence.
