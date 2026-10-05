# 02 — Native GUI and remembered connection

Question: can the Qt replacement provide a small native form with reliable persisted settings and Japanese interaction?

- Seam: `gui` reads immutable session view; `SettingsStore` is the only reader/writer of the existing native store. A development-only state driver exercises connecting/waiting/streaming/failure; do not ship mock states as product behavior.
- Native layout follows the UX contract in README. One renderer; enable accessibility and actual system-font support. No camera preview, per-frame repaint, WebView, or permanent timer when idle.
- Copy gate: the idle surface contains connection mode, a short phone-side instruction, Connect, and collapsed Details. Wi-Fi labels the phone's IP; USB keeps unclassified devices selectable and asks for a selection when needed. Exercise 0/1/multiple candidates; there is no fixed brand/mode choice. Waiting for video points to Start camera on the phone. Saving/restoring adds no success message. A blocked/error state gives one short next action; raw errors stay in Details. A smaller window scrolls rather than clipping its controls.
- EN/JA gate: follow the primary OS UI locale without a language control or saved preference. Verify Japanese/English/unsupported or missing locales; all labels/actions/waits/errors/summaries are paired, including persistence failure. Language and copy never alter stable option values or session state. Test different PC/Android locales and Japanese IME; verify the native locale API before adding a dependency.
- Read/write actual Qt-written Linux INI and Windows registry fixtures, including saved IP, existing transform/decode/output fields, fallback precedence and unknown fields. Verify Qt -> Rust -> Qt -> Rust round trips. No new store or migration procedure.
- Visible artifact: native window with editable remembered Wi-Fi address, keyboard-operable primary action, Details and errors. The HTML prototype is a review aid, not native acceptance.
- Accept: restart, rollback and failed-connect save cases pass; parse/atomic-write/registry failures preserve prior data and are visible; Unicode/escaped values work; Japanese IME, display scaling and native Windows/Linux X11/Wayland startup are checked. Measure GUI/package size and idle repaint behavior before committing the dependency choice.
- Visual gate: review label/input legibility and primary-action clarity at normal and enlarged text sizes; compare the prototype and candidate with compare-screenshots, then run an unprimed screenshot-critique as the final visual check. No user sign-off is required to proceed with a reversible layout choice.
- Delegated: exact spacing/colors, released eframe/system-font feature pins, the product-key INI implementation after verified fixtures. Reopen GUI choice only for a concrete size/startup/IME/accessibility failure.

Dependency: 01's build/version facts. Next useful checkpoint: the native settings/UI can be reviewed before camera integration.
