# Batches

`batch` runs several verbs in one call against one page. A login is four verbs
that need no decision between them, and an agent pays a model round trip for
each one.

```
lurien batch "goto url=https://example.com/login" \
             "fill selector=label:Email text=me@example.com" \
             "fill selector=label:Password text=hunter2" \
             'click selector="role:button=Log in"'
```

## A step

A step is a verb name followed by `key=value` pairs, named exactly as
[`VERBS.md`](VERBS.md) names them. Quote a value that holds spaces:

```
click selector="role:button=Log in"
```

Inside a quoted value, use JSON-style escapes for a quote, backslash, newline,
carriage return, or tab:

```
eval script="return {\"ok\": true};"
```

A list value is comma separated:

```
upload selector=#photos files=/tmp/a.png,/tmp/b.png
```

Values are typed by the verb's own spec, so `wait ms=soon` is refused rather
than sent to the page as a string.

## Failure

Steps are parsed and checked against their specs before the first one runs. A
typo in the fifth step is caught while the page is still untouched:

```
batch: step 5: click has no argument "selecter"; accepts ["selector", "timeout_ms"]
```

A step that fails at run time stops the batch, and the error says how far the
page got:

```
batch step 2 (click) failed: role:button=Nothing Here: no element matched after
4000ms. on screen now: button "Log in"; textbox "Email". ran: 1 title; 1 step(s)
not run
```

A batch cannot contain a batch. Flatten the steps into one list.

## On the wire

The three faces run identical batches. `lurien serve` accepts the steps as a
JSON array, as one step per line, or as an array encoded in a string:

```json
{ "command": "batch",
  "args": { "steps": ["goto url=https://example.com/", "title"] } }
```

A successful batch answers with one row per step:

```json
{ "ran": 2,
  "steps": [ { "step": 1, "verb": "goto", "output": { … } },
             { "step": 2, "verb": "title", "output": "Example" } ] }
```

## Verification

`lurien/tests/e2e_batch.sh` drives the real engine and asserts that a four-step
batch fills and submits a form with trusted events, that a batch stopping at
step two names the step, what ran and what was skipped, that a step after the
failure did not run, that an unknown verb is refused before anything runs, and
that the CLI runs the same step list as the HTTP face.
