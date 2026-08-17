// The page as a list of addressable nodes, evaluated in the page.
//
// Loaded after `locator.js`, whose role table, name computation and visibility
// rules this reuses: a snapshot that named elements differently from the way a
// selector finds them would be a second convention, and a handle taken from one
// would not resolve through the other.
//
// A node carries a `path` (a CSS path) and a handle `eN`. The driver keeps the
// handle table, so the page is never tagged with an attribute a script could
// read, and a handle is checked against the role and name it was captured with
// before it is acted on.
//
// Returns a JSON string.

// Elements worth an entry even when they hold no text: they are what an act
// verb can address.
const LURIEN_ADDRESSABLE =
  "a[href], area[href], button, input, select, textarea, summary, option, " +
  "progress, output, dialog, [role], [contenteditable=true], [contenteditable='']";

// Text long enough to be prose is cut: a snapshot is a map of the page, and an
// agent that wants the article reads it with `text`.
const LURIEN_TEXT_CAP = 160;

// Regions worth a line even when they carry no text of their own: they are what
// makes the list a map of the page rather than a pile of controls.
const LURIEN_STRUCTURE = new Set([
  "form",
  "search",
  "navigation",
  "main",
  "banner",
  "contentinfo",
  "dialog",
  "table",
  "list",
  "tabpanel",
]);

// The role a snapshot line carries. An element with no role of its own that is
// listed for its text is `text`, and both the walk and the handle check read
// this, or a text node's handle would never verify.
function lurienEntryRole(el) {
  const role = lurienNodeRole(el);
  const known = LURIEN_ROLES[role] !== undefined && role !== "paragraph";
  return known || el.matches(LURIEN_ADDRESSABLE) ? role : "text";
}

function lurienNodeState(el, role) {
  const state = [];
  if (el.disabled || el.getAttribute("aria-disabled") === "true") {
    state.push("disabled");
  }
  if (role === "checkbox" || role === "radio" || role === "switch") {
    const on = el.checked === undefined ? el.getAttribute("aria-checked") === "true" : el.checked;
    state.push(on ? "checked" : "unchecked");
  }
  if (el.getAttribute("aria-expanded")) {
    state.push("expanded=" + el.getAttribute("aria-expanded"));
  }
  if (role === "heading") {
    const level = el.getAttribute("aria-level") || el.localName.slice(1);
    if (/^[1-9]$/.test(level)) {
      state.push("level=" + level);
    }
  }
  if (el.localName === "input" || el.localName === "textarea") {
    const type = (el.getAttribute("type") || "text").toLowerCase();
    if (type === "file") {
      state.push("file");
    }
    if (el.required) {
      state.push("required");
    }
  }
  return state;
}

// The text a node contributes on its own, which is only the text no child of it
// already reports.
function lurienOwnText(el) {
  let own = "";
  for (const node of el.childNodes) {
    if (node.nodeType === 3) {
      own += node.nodeValue;
    }
  }
  return LURIEN_NORM(own);
}

/**
 * The page as an ordered node list.
 *
 * `limit` bounds the list, because a snapshot an agent pays for by the token
 * must have a ceiling: the answer says how many nodes were dropped rather than
 * pretending the page ended.
 */
function lurienSnapshot(limit) {
  const nodes = [];
  let dropped = 0;
  let counter = 0;

  const walk = (el, depth, carried) => {
    let mine = depth;
    let held = carried;
    const role = lurienNodeRole(el);
    const addressable = el.matches(LURIEN_ADDRESSABLE);
    const name = LURIEN_NORM(lurienName(el)).slice(0, LURIEN_TEXT_CAP);
    const own = lurienOwnText(el).slice(0, LURIEN_TEXT_CAP);
    const known = LURIEN_ROLES[role] !== undefined && role !== "paragraph";
    // A label whose text is already the name of its control says nothing twice.
    const label = el.localName === "label" && el.control &&
      LURIEN_LOWER(lurienName(el.control)) === LURIEN_LOWER(name || own);
    // A span inside a button is not a second node: the button already reports
    // that text, and an agent charged per token should not read it twice.
    const echo = !addressable && !known && own && carried.includes(LURIEN_LOWER(own));
    const region = LURIEN_STRUCTURE.has(role);
    const worth = !label && (addressable || region || (known && (name || own)) || (own && !echo));
    if (worth && lurienVisible(el)) {
      if (nodes.length >= limit) {
        dropped += 1;
      } else {
        counter += 1;
        const entry = {
          ref: "e" + counter,
          role: lurienEntryRole(el),
          name: name || own,
          path: lurienPath(el),
          depth,
          state: lurienNodeState(el, role),
        };
        const value = el.value;
        if (typeof value === "string" && value && el.type !== "password") {
          entry.value = value.slice(0, LURIEN_TEXT_CAP);
        }
        nodes.push(entry);
        mine = depth + 1;
        held = LURIEN_LOWER(entry.name);
      }
    }
    for (const child of el.children) {
      walk(child, mine, held);
    }
  };

  if (document.body) {
    walk(document.body, 0, "");
  }
  return JSON.stringify({
    title: document.title,
    url: location.href,
    nodes,
    dropped,
  });
}

/**
 * Is the handle's element still the element the handle was captured for?
 *
 * A handle is a promise about one node, and a page that re-rendered underneath
 * it can leave the path pointing at something else. Checking role and name
 * turns that into a refusal instead of a click on whatever moved into place.
 */
function lurienVerify(path, role, name) {
  let el = null;
  try {
    el = document.querySelector(path);
  } catch (e) {
    return JSON.stringify({ ok: false, why: "invalid", detail: String(e && e.message) });
  }
  if (!el) {
    return JSON.stringify({ ok: false, why: "gone" });
  }
  const now = lurienEntryRole(el);
  const label = LURIEN_NORM(lurienName(el)) || lurienOwnText(el);
  if (now !== role || LURIEN_LOWER(label) !== LURIEN_LOWER(name)) {
    return JSON.stringify({ ok: false, why: "changed", role: now, name: label });
  }
  return JSON.stringify({ ok: true });
}
