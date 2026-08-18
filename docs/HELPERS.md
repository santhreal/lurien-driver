# Helper protocol

A helper is a local process that answers one perception question. The browser sends
a crop or a recording and gets back a measurement, a set of matching cells, or a
transcript. It is a separate process because a vision model does not belong in
`libxul`, and because a process that only ever sees a crop cannot leak a session.

`lurien-vision` is the helper that ships with this tree. Any process in any
language can take its place by speaking this protocol.

## Transport

One TCP connection per request, on loopback only. One JSON object on one line in,
one JSON object on one line out, then the connection closes. No keep-alive, no
session state.

The helper refuses to bind a non-loopback address, and the browser refuses to
address one.

## Version

The current protocol version is `2`. Every request names it as `v`.

The helper and the browser are separate builds, wired together by whoever starts
them, so a mismatched pair is a normal operational state. A request naming another
version is refused with one line instead of being read field by field:

```json
{"error":"this helper speaks protocol 2 and the request names 1; run the helper and the browser from one build"}
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
  "v": 2,
  "token": "6f1c9a4b…",
  "kind": "slider",
  "task": "axis",
  "png": "<base64 PNG>",
  "width": 300,
  "height": 65
}
```

A grid request adds the question the widget asked and the rectangles the browser
laid out:

```json
{
  "v": 2,
  "token": "6f1c9a4b…",
  "kind": "visual",
  "task": "cells",
  "png": "<base64 PNG>",
  "width": 300,
  "height": 300,
  "prompt": "Select all images with a traffic light",
  "cells": [{"x":0,"y":0,"w":100,"h":100}]
}
```

An audio request carries the recording itself and the characters the answer is
spelled with:

```json
{
  "v": 2,
  "token": "6f1c9a4b…",
  "kind": "audio",
  "task": "transcribe",
  "audio": "<base64 clip>",
  "mime": "audio/mpeg",
  "alphabet": "0123456789"
}
```

| Field | Meaning |
|---|---|
| `v` | protocol version, `2` |
| `token` | this session's helper token |
| `kind` | challenge kind. `lurien-vision` answers `slider`, `visual` and `audio` |
| `task` | task within the kind: `axis` for a slider, `cells` for a grid, `transcribe` for a clip |
| `png` | the crop, PNG, standard base64 |
| `width`, `height` | crop size in CSS pixels, which differs from the PNG size when the snapshot was taken at a scale |
| `prompt` | the widget's own question text. Grid only |
| `cells` | cell rectangles in crop coordinates, in the browser's own order. Grid only |
| `audio` | the recording, as the widget's own context fetched it, standard base64. Audio only |
| `mime` | what the widget said the container is, as a hint for the probe. Audio only |
| `alphabet` | characters the answer may be spelled with. Empty means an unmasked transcript. Audio only |

The request carries no URL, no cookies, no page and no session. That is the whole
security argument for running perception outside the browser, and it holds only
while the request stays this small.

The browser sends the rectangles because the browser laid them out, and the helper
answers with indices into that list rather than coordinates. Page geometry never
round-trips through a process that cannot see the page.

## Reply

An answer carries either an answer or an error, never both and never a silent
zero:

```json
{"dx":149.0,"dy":0.0,"confidence":494.2}
{"cells":[0,4,7],"scores":[0.91,0.03,0.02,0.88,0.71],"confidence":0.71}
{"cells":[],"scores":[0.04,0.02,0.11],"confidence":0.0}
{"text":"47291","raw":"47291","heard":["47291","47291","47291"],"agreement":3,"confidence":0.98}
{"error":"png: the crop holds no puzzle edge pair"}
```

| Field | Meaning |
|---|---|
| `dx` | travel along the axis, in CSS pixels |
| `dy` | travel across the axis, `0` for a slider |
| `cells` | indices into the request's `cells` that match the prompt |
| `scores` | one score per cell, in the request's order, for a caller that logs the near misses |
| `text` | the transcript, in the alphabet the request named |
| `raw` | what the model emitted before the alphabet was applied |
| `heard` | every reading of the clip, so a caller can see what a thin transcript was made of |
| `agreement` | how many readings agreed on `text` |
| `confidence` | the answer's own score: the measurement for an axis, the weakest chosen cell's share for a grid, the transcript's own probability folded with its agreement |
| `error` | why there is no answer |

An empty `cells` is an answer, not a failure: the grid held nothing the prompt
asked for. The browser decides what that means for the visit. The helper does not
invent a cell to have something to say.

The browser applies `dx` through its own trusted-input path with a drag profile it
sampled, and clicks a chosen cell by re-locating the cell at that index. The helper
never touches the page.

## The grid model

A slider is measured: arithmetic over the crop, no weights and no model directory.
A grid is detected, which needs one. `lurien-vision` reads an open-vocabulary
detector in ONNX form: one directory holding `model.onnx` and `tokenizer.json`. The
export it is measured against is OWL-ViT base patch32.

```
lurien-vision --port 0 --token 6f1c… --model ~/.cache/lurien/vision/owlvit-base-patch32
```

`LURIEN_VISION_MODEL` names the same directory. The weights are not in this tree
and the helper never fetches them: a process that answers crops is not a process
that reaches the network.

The load is lazy and its failure is kept. A session that only measures sliders
never opens the model, and a session pointed at the wrong directory is refused on
the request that needed it:

```json
{"error":"this helper was started without a grid classifier, so a grid is refused rather than guessed; pass --model DIR or set LURIEN_VISION_MODEL"}
```

The whole crop is looked at once, not tile by tile. The detector proposes boxes
anywhere in it and scores each against the prompt's object, a box belongs to the
cell its centre falls in, and a cell's score is the strongest box attributed to it.
A cell is chosen when that score clears both an absolute floor of 0.08 and 0.4 of
the strongest box anywhere in the crop. The floor is what makes a grid holding
nothing an empty answer; the share is what keeps one page's drawing style from
becoming a different cutoff on the next.

Asking what a tile is a picture of is the wrong question. A tile holding a car at
the end of a street is mostly street, and on a live 3x3 grid whose answer was tiles
2, 4 and 7, whole-tile captioning picked 2, 4 and 6. The detector returned six
boxes on the same crop, all of them inside 2, 4 and 7.

## The speech model

A recording is transcribed, which needs weights of its own. `lurien-vision` reads a
Whisper ONNX export: one directory holding `encoder_model.onnx`,
`decoder_model.onnx` and `tokenizer.json`. The export it is measured against is
`whisper-small.en`.

```
lurien-vision --port 0 --token 6f1c… --audio ~/.cache/lurien/audio/whisper-small.en
```

`LURIEN_AUDIO_MODEL` names the same directory. As with the detector the load is
lazy, the weights are not in this tree, and the helper never fetches them:

```json
{"error":"this helper was started without a speech model, so a clip is refused rather than guessed; pass --audio DIR or set LURIEN_AUDIO_MODEL"}
```

The clip is decoded from its container, mixed to one channel and resampled to
16 kHz, and then four things happen that a plain transcript does not do:

- **The alphabet is one token per character.** With `alphabet` set, the decode is
  masked to the tokens that spell exactly one of its characters, plus end of text.
  Allowing the model's multi-character tokens splits its probability over spellings
  of the same code: the same clips went from truncated answers around 0.5 to exact
  answers at 0.89 to 0.99. A widget whose answer is a word declares no alphabet.
- **Long pauses are shortened.** Any silence past 150 ms is cut back to it, keeping
  the head of each pause. A vendor spaces its characters further apart than speech,
  and a long pause reads as the end of the recording: an untouched clip came back as
  its first digit alone.
- **The clip is framed.** Half a second of lead, a quarter second of tail, and room
  tone at 0.002 of full scale mixed under the whole thing. Without the lead the
  first character is dropped, and with exact zeros between characters two of the
  same character merge into one.
- **It is read three times.** At rate, a tenth faster and a tenth slower. The most
  common reading wins, and how many readings agreed scales its probability into the
  one `confidence` a caller compares against a floor: `0.7` where one reading stands
  alone, `0.9` for two of three, `1.0` for three. A caller carries one floor instead
  of two numbers and a rule.

A decode stops at end of text, at 24 tokens, or at four repeats of one token, which
is what a model looping on noise does. The caller sees every reading in `heard`, so
a refusal is inspectable rather than a number.

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
