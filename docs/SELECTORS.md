# Selectors

Every verb that takes a `selector` accepts a CSS selector, one of five semantic
forms, or a handle from the last snapshot. The form is the prefix before the
first colon. A selector with no known prefix is CSS, so every selector written
before these forms existed still means what it meant.

| Form | Example | Matches |
|---|---|---|
| `role:` | `role:button`, `role:button=Log in` | An element with that ARIA role, explicit or implicit, optionally with that accessible name |
| `text:` | `text:Continue to checkout` | The innermost element whose visible text contains the value |
| `label:` | `label:Email` | A form control whose `<label>`, `aria-label` or `aria-labelledby` is that text |
| `placeholder:` | `placeholder:you@example.com` | A control with that placeholder |
| `testid:` | `testid:submit` | `data-testid`, `data-test-id` or `data-test` |
| `ref:` | `ref:e7` | The node that handle named in the last `snapshot` |
| CSS | `#login > button.primary` | The first element the page matches |

`role:`, `label:` and `placeholder:` compare the whole name, case-insensitively
and with runs of whitespace collapsed, so `role:button=log  in` finds a button
named `Log in`. `text:` matches a substring, because page text is prose and a
caller quotes the part they read. `testid:` compares the attribute exactly: a
test id is an identifier, not language.

The accessible name is computed in the page, in this order:
`aria-labelledby`, `aria-label`, the associated `<label>`, the value of an
`input`, `alt`, visible text, then `title` or `placeholder`.

## Handles

`snapshot` reports the page as roles, names and handles:

```
- heading "Account" [level=1] [ref=e1]
- button "Log in" [ref=e2]
- textbox "Email" [ref=e4]
```

`ref:e2` then acts on that button. The handle table lives in the driver, not in
the page: nothing is tagged with an attribute page script could read.

A handle is checked before it is used. If the node it named is gone, or now has
a different role or name, the act is refused and says what changed, because a
page that re-rendered underneath a handle would otherwise take a click meant for
something else:

```
ref:e2: the page changed under the handle: button "Log in" is now button "Sign out" after 0ms.
take a fresh snapshot and use the handle it reports
```

Handles are numbered per snapshot, so the next snapshot renumbers them. Take one
snapshot, act from its handles, and take another when the page changes.

## One element, or a refusal

A semantic form must resolve to exactly one visible, enabled element. A
description that fits three buttons is not a description, and clicking the
first of them is how the wrong button gets pressed. The error names the
candidates:

```
role:button=Send: 2 visible elements fit that description after 3ms.
narrow it, or use one of: div > button "Send message"; div > button:nth-of-type(2) "Send invite"
```

CSS keeps its first-match contract. A CSS selector is a precise machine query
and callers depend on that.

An element that is present but invisible, or visible but disabled, is refused
rather than clicked. An element parked off the canvas with `left: -9999px` is
not visible: no scroll can bring it into the viewport. An element below the
fold is visible, because the page scrolls to it.

`count` is the exception to all of this: it reports how many elements fit the
description and how many of those are visible, and never refuses.

## The wait

A verb that acts waits for its element to be actionable. There is no separate
wait verb to call first and no sleep to guess at:

```
lurien click 'role:button=Publish'
```

clicks a button that the page adds two seconds after the navigation.

The default deadline is 10 seconds. `LURIEN_TIMEOUT_MS` moves it for a session;
`timeout_ms` moves it for one call. Resolution is retried every 100 ms until the
deadline.

Three failures do not wait, because waiting cannot change them: an invalid
selector, an unknown form, and an ambiguous description.

A read (`text`, `upload`) accepts an element that is present but not visible.
An act does not.

## The failure

Every unresolved selector produces the same shape: what was asked, what the page
had, how long it was given, and what to do next.

```
<selector>: <what went wrong> after <waited>ms. <what to do>
```

When nothing matched, "what to do" lists what was on screen instead, up to eight
elements by role and accessible name, so the next call can be written without a
round trip through `snapshot`:

```
role:button=Nothing Here: no element matched after 4000ms.
on screen now: button "Log in"; textbox "Email"; button "Send message"
```

## Verification

`lurien/tests/e2e_locator.sh` drives the real engine over the HTTP face against
`captcha/kinds/fixtures/locator_forms.html`, whose elements carry ids like `a1`
so that only the semantic form can find them. It asserts that each form clicks
or focuses the intended element, that the click arrives as a trusted event, that
a button appended 1.5 s after load is clicked without an explicit wait and not
before it exists, and that the ambiguous, invisible, disabled and missing cases
are refused with an explanation.

`lurien/tests/e2e_snapshot.sh` drives the same fixture and asserts that the
default snapshot is a node list rather than markup, that a handle from it clicks
the node its line described as a trusted event, that the same handle is refused
once the fixture renames that button, and that source is still reachable.
