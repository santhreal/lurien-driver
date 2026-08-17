# Rebase

The engine is a Camoufox patch series on Firefox. A rebase is a product
operation, not a side quest.

## Must apply after every rebase

1. `banner-no-visual-cue.patch` — skip `gRemoteControl.updateVisualCue()`.
2. `lurien-config.patch` — `MaskConfig.hpp` reads `LURIEN_CONFIG` first.
3. `challenge-register.patch` — Observer attaches; Catalog loads `kinds/*.toml`.

If a patch fails to apply, stop. Do not ship a binary that is missing one.

## Sequence

1. Fetch the next Camoufox / Firefox tag into the staging tree.
2. Replay `engine/patches/` in listed order.
3. Rebuild (`docs/ENGINE.md`).
4. `lurien_gate` vs stock FF of the **same** major. High/Critical identical.
5. `live_detector_suite` on this host.
6. Managed CF `score` still writes a token on `goto`.
7. Bump the engine version string. `lurien --version` prints both crate and engine.

## Do not

- Do not silently drop a patch that "looks unused".
- Do not reintroduce `MouseTrajectories.hpp`. guise owns the mouse model.
- Do not add a vendor-named `.cpp` during rebase "while you're there".
