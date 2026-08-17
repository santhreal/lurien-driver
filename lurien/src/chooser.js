// Catch the file chooser a page opens, instead of letting a native dialog block
// the session.
//
// A file input opens the OS picker as the default action of a click, and a
// headless or unattended session has nobody to answer it: the page stalls with a
// dialog nothing will close. Cancelling that default action leaves the input
// exactly where it was, so the driver can attach the caller's files to it and the
// page's own change handler runs as it would have.
//
// Concatenated after `locator.js`, whose `lurienPath` is what turns the caught
// input into an address the driver can act on. Armed for one chooser at a time:
// nothing is intercepted unless a caller asked for it, so a page that opens a
// picker on its own behaves normally.

function lurienArmChooser() {
  if (!window.__lurienChooser) {
    const state = { armed: false, path: "", tag: "" };
    window.__lurienChooser = state;
    document.addEventListener(
      "click",
      event => {
        if (!state.armed) {
          return;
        }
        const target = event.target;
        const input =
          target && target.closest
            ? target.closest('input[type="file"]')
            : null;
        if (!input) {
          return;
        }
        // Only the default action is cancelled. The page's own listeners still
        // see the click, so a script that opens the chooser and then tracks the
        // element keeps working.
        event.preventDefault();
        state.armed = false;
        state.path = lurienPath(input);
        state.tag = input.getAttribute("name") || input.id || "file input";
      },
      true
    );
  }
  const state = window.__lurienChooser;
  state.armed = true;
  state.path = "";
  state.tag = "";
  return "armed";
}

/** The input whose chooser was caught, or an empty answer while none has been. */
function lurienCaughtChooser() {
  const state = window.__lurienChooser;
  if (!state || !state.path) {
    return JSON.stringify({ ok: false });
  }
  return JSON.stringify({ ok: true, path: state.path, tag: state.tag });
}
