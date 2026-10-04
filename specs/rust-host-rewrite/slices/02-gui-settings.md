# 02 — Native GUI and remembered connection

Question: can the Qt replacement provide a small native form with reliable persisted settings and Japanese interaction?

- Seam: `gui` reads immutable session view; `SettingsStore` is the only reader/writer of the existing native store. A development-only state driver exercises connecting/waiting/streaming/failure; do not ship mock states as product behavior.
- Native layout follows the UX contract in README. One renderer; enable accessibility and actual system-font support. No camera preview, per-frame repaint, WebView, or permanent timer when idle.
- Copy gate: the idle surface contains connection mode, Connect, and collapsed Details. Wi-Fi shows its address; USB shows a phone selector only for multiple candidates. Exercise 0/1/multiple candidates with the state driver; there is no fixed brand/mode choice. Saving/restoring adds no success message. A blocked/error state gives one short next action; logs and prototype controls are collapsed. Keep labels and actionable failures readable rather than replacing them with ambiguous icons.
- Read/write actual Qt-written Linux INI and Windows registry fixtures, including saved IP, existing transform/decode/output fields, fallback precedence and unknown fields. Verify Qt -> Rust -> Qt -> Rust round trips. No new store or migration procedure.
- Visible artifact: native window with editable remembered Wi-Fi address, keyboard-operable primary action, Details and errors. The HTML prototype is a review aid, not native acceptance.
- Accept: restart, rollback and failed-connect save cases pass; parse/atomic-write/registry failures preserve prior data and are visible; Unicode/escaped values work; Japanese IME, display scaling and native Windows/Linux X11/Wayland startup are checked. Measure GUI/package size and idle repaint behavior before committing the dependency choice.
- Visual gate: review label/input legibility and primary-action clarity at normal and enlarged text sizes; compare the prototype and candidate with compare-screenshots, then run an unprimed screenshot-critique as the final visual check. No user sign-off is required to proceed with a reversible layout choice.
- Delegated: exact spacing/colors, released eframe/system-font feature pins, the product-key INI implementation after verified fixtures. Reopen GUI choice only for a concrete size/startup/IME/accessibility failure.

Dependency: 01's build/version facts. Next useful checkpoint: the native settings/UI can be reviewed before camera integration.
