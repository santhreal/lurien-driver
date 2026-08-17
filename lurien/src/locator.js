// The selector resolver, evaluated in the page.
//
// Two entry points share one matcher: `lurienResolve` answers "which element do
// I act on", `lurienCount` answers "how many fit". They answer with a CSS path
// rather than a handle, so acting on the result goes through the same trusted
// element path a plain CSS selector does, and they mutate nothing: a resolver
// that tagged the element with an attribute would be visible to page script.
//
// Both return a JSON string.

const LURIEN_NORM = s => (s || "").replace(/\s+/g, " ").trim();
const LURIEN_LOWER = s => LURIEN_NORM(s).toLowerCase();

// Roles worth addressing, with the elements that carry them implicitly. A role
// the table does not name still resolves through an explicit [role=...].
const LURIEN_ROLES = {
  button:
    "button, [role=button], input[type=button], input[type=submit], input[type=reset], summary",
  link: "a[href], area[href], [role=link]",
  textbox:
    "input:not([type]), input[type=text], input[type=email], input[type=password], " +
    "input[type=search], input[type=tel], input[type=url], textarea, " +
    "[role=textbox], [contenteditable=true], [contenteditable='']",
  spinbutton: "input[type=number], [role=spinbutton]",
  checkbox: "input[type=checkbox], [role=checkbox]",
  radio: "input[type=radio], [role=radio]",
  // A multi-select is a listbox, so it is tested before the plain select.
  listbox: "select[multiple], [role=listbox]",
  combobox: "select, [role=combobox]",
  option: "option, [role=option]",
  slider: "input[type=range], [role=slider]",
  heading: "h1, h2, h3, h4, h5, h6, [role=heading]",
  img: "img[alt]:not([alt='']), [role=img]",
  list: "ul, ol, [role=list]",
  listitem: "li, [role=listitem]",
  table: "table, [role=table]",
  row: "tr, [role=row]",
  cell: "td, [role=cell], [role=gridcell]",
  columnheader: "th, [role=columnheader]",
  form: "form, [role=form]",
  search: "form[role=search], [role=search]",
  navigation: "nav, [role=navigation]",
  main: "main, [role=main]",
  banner: "header, [role=banner]",
  contentinfo: "footer, [role=contentinfo]",
  dialog: "dialog, [role=dialog], [role=alertdialog]",
  alert: "[role=alert], output",
  status: "[role=status]",
  progressbar: "progress, [role=progressbar]",
  switch: "[role=switch]",
  tab: "[role=tab]",
  tabpanel: "[role=tabpanel]",
  menuitem: "[role=menuitem], [role=menuitemcheckbox], [role=menuitemradio]",
  separator: "hr, [role=separator]",
  paragraph: "p",
};

function lurienExplicitRole(el) {
  const attr = LURIEN_NORM(el.getAttribute("role"));
  return attr ? attr.split(/\s+/)[0].toLowerCase() : "";
}

// The role an element carries, explicit or implicit. The table above is the only
// place roles and elements are related, so this reads it rather than repeating it
// inverted.
function lurienNodeRole(el) {
  const own = lurienExplicitRole(el);
  if (own) {
    return own;
  }
  for (const role of Object.keys(LURIEN_ROLES)) {
    try {
      if (el.matches(LURIEN_ROLES[role])) {
        return role;
      }
    } catch (e) {
      // A selector the page cannot parse is not this element's role.
    }
  }
  return el.localName;
}

// The accessible name, in the order a screen reader computes it.
function lurienName(el) {
  const byRefs = el.getAttribute("aria-labelledby");
  if (byRefs) {
    const parts = byRefs
      .split(/\s+/)
      .map(id => document.getElementById(id))
      .filter(Boolean)
      .map(node => LURIEN_NORM(node.innerText || node.textContent));
    if (parts.length) {
      return LURIEN_NORM(parts.join(" "));
    }
  }
  if (LURIEN_NORM(el.getAttribute("aria-label"))) {
    return LURIEN_NORM(el.getAttribute("aria-label"));
  }
  if (el.labels && el.labels.length) {
    const label = LURIEN_NORM(el.labels[0].innerText || el.labels[0].textContent);
    if (label) {
      return label;
    }
  }
  if (el.localName === "input") {
    const type = (el.getAttribute("type") || "").toLowerCase();
    if (type === "submit" || type === "button" || type === "reset") {
      return LURIEN_NORM(el.value || type);
    }
  }
  if (el.localName === "img" || el.localName === "area") {
    return LURIEN_NORM(el.getAttribute("alt"));
  }
  const text = LURIEN_NORM(el.innerText || el.textContent);
  if (text) {
    return text;
  }
  return LURIEN_NORM(el.getAttribute("title") || el.getAttribute("placeholder"));
}

function lurienVisible(el) {
  const rect = el.getBoundingClientRect();
  if (rect.width < 1 || rect.height < 1) {
    return false;
  }
  const style = getComputedStyle(el);
  if (style.visibility === "hidden" || style.visibility === "collapse") {
    return false;
  }
  if (style.display === "none" || Number(style.opacity) < 0.05) {
    return false;
  }
  if (el.checkVisibility && !el.checkVisibility()) {
    return false;
  }
  // `left: -9999px` is the oldest way to keep an element rendered and
  // unreachable. Its box is real and its style is visible, so the only thing
  // that tells it apart is that it sits outside the document, which is also why
  // no scroll can bring it into the viewport and no click can land on it. An
  // element merely below the fold stays visible: the page scrolls to it.
  const doc = document.documentElement;
  const left = rect.left + window.scrollX;
  const top = rect.top + window.scrollY;
  if (left + rect.width <= 0 || top + rect.height <= 0) {
    return false;
  }
  const width = Math.max(doc.scrollWidth, window.innerWidth);
  const height = Math.max(doc.scrollHeight, window.innerHeight);
  return left < width && top < height;
}

function lurienEnabled(el) {
  if (el.disabled) {
    return false;
  }
  if (el.getAttribute("aria-disabled") === "true") {
    return false;
  }
  return !el.closest("[inert], fieldset[disabled]");
}

// A path that identifies this element and nothing else, so the act can be
// dispatched through the ordinary element lookup.
function lurienPath(el) {
  const esc = s => (window.CSS && CSS.escape ? CSS.escape(s) : s);
  const parts = [];
  for (let node = el; node && node.nodeType === 1; node = node.parentElement) {
    if (node.id && document.querySelectorAll("#" + esc(node.id)).length === 1) {
      parts.unshift("#" + esc(node.id));
      break;
    }
    const tag = node.localName;
    const parent = node.parentElement;
    if (!parent) {
      parts.unshift(tag);
      break;
    }
    const twins = Array.from(parent.children).filter(c => c.localName === tag);
    parts.unshift(
      twins.length > 1 ? tag + ":nth-of-type(" + (twins.indexOf(node) + 1) + ")" : tag
    );
  }
  return parts.join(" > ");
}

// What is on screen, for an error that tells the caller what to ask for instead
// of only what was missing.
function lurienOnScreen() {
  const rows = [];
  const seen = new Set();
  for (const el of document.querySelectorAll(
    "button, a[href], input, select, textarea, [role]"
  )) {
    if (rows.length >= 8 || !lurienVisible(el)) {
      continue;
    }
    const label = lurienName(el).slice(0, 40);
    const role = lurienNodeRole(el);
    const key = role + "/" + label;
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    rows.push(label ? role + ' "' + label + '"' : role);
  }
  return rows;
}

/** Every element the description fits, or why the description is unusable. */
function lurienMatches(form, value) {
  const query = selector => {
    try {
      return { matches: Array.from(document.querySelectorAll(selector)) };
    } catch (e) {
      return { why: "invalid", detail: String(e && e.message) };
    }
  };
  if (form === "css") {
    return query(value);
  }
  if (form === "role") {
    const cut = value.indexOf("=");
    const role = LURIEN_LOWER(cut < 0 ? value : value.slice(0, cut));
    const wanted = cut < 0 ? "" : LURIEN_LOWER(value.slice(cut + 1));
    const found = query(LURIEN_ROLES[role] || "[role=" + role + "]");
    if (!found.matches) {
      return found;
    }
    return {
      matches: found.matches.filter(el => {
        const own = lurienExplicitRole(el);
        if (own && own !== role) {
          return false;
        }
        return wanted ? LURIEN_LOWER(lurienName(el)) === wanted : true;
      }),
    };
  }
  if (form === "text") {
    const wanted = LURIEN_LOWER(value);
    const all = Array.from(document.querySelectorAll("body *")).filter(el =>
      LURIEN_LOWER(el.textContent).includes(wanted)
    );
    // The innermost element holding the text is the one a person would click.
    return { matches: all.filter(el => !all.some(other => other !== el && el.contains(other))) };
  }
  if (form === "label") {
    const wanted = LURIEN_LOWER(value);
    const found = query("input, select, textarea, [aria-label], [aria-labelledby]");
    if (!found.matches) {
      return found;
    }
    return { matches: found.matches.filter(el => LURIEN_LOWER(lurienName(el)) === wanted) };
  }
  if (form === "placeholder") {
    const wanted = LURIEN_LOWER(value);
    const found = query("[placeholder]");
    if (!found.matches) {
      return found;
    }
    return {
      matches: found.matches.filter(
        el => LURIEN_LOWER(el.getAttribute("placeholder")) === wanted
      ),
    };
  }
  if (form === "testid") {
    const found = query("[data-testid], [data-test-id], [data-test]");
    if (!found.matches) {
      return found;
    }
    return {
      matches: found.matches.filter(el =>
        ["data-testid", "data-test-id", "data-test"].some(
          attr => el.getAttribute(attr) === value
        )
      ),
    };
  }
  return { why: "unknown form", detail: form };
}

/** The one element to act on, or why there is not exactly one. */
function lurienResolve(form, value, need) {
  const done = fields => JSON.stringify(Object.assign({ ok: false, matched: 0 }, fields));
  const found = lurienMatches(form, value);
  if (!found.matches) {
    return done({ why: found.why, detail: found.detail });
  }
  const matches = found.matches;
  if (!matches.length) {
    return done({ why: "none", candidates: lurienOnScreen() });
  }
  const usable = need ? matches.filter(el => lurienVisible(el) && lurienEnabled(el)) : matches;
  if (!usable.length) {
    const hidden = matches.filter(el => !lurienVisible(el)).length;
    return done({
      why: hidden ? "hidden" : "disabled",
      matched: matches.length,
      candidates: lurienOnScreen(),
    });
  }
  // A description that fits several elements on screen is not a description. A
  // CSS selector is a machine query, so it keeps its first-match contract.
  if (form !== "css" && usable.length > 1) {
    return done({
      why: "ambiguous",
      matched: usable.length,
      candidates: usable
        .slice(0, 8)
        .map(el => lurienPath(el) + ' "' + lurienName(el).slice(0, 40) + '"'),
    });
  }
  return JSON.stringify({ ok: true, path: lurienPath(usable[0]), matched: usable.length });
}

/** How many elements the description fits, and how many of those are visible. */
function lurienCount(form, value) {
  const found = lurienMatches(form, value);
  if (!found.matches) {
    return JSON.stringify({ ok: false, why: found.why, detail: found.detail, matched: 0 });
  }
  return JSON.stringify({
    ok: true,
    matched: found.matches.length,
    visible: found.matches.filter(lurienVisible).length,
  });
}
