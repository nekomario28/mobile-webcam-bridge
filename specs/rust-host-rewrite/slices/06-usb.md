# 06 — USB AOA and selected-phone continuity

Question: does Rust support manufacturer-neutral Android selection while preserving AOA access with USB debugging off and the stream transfer boundary?

- Seam: rusb/libusb transport and a small Linux USB-only elevated helper; reuse the wire/video session rather than a separate USB receiver. The privileged helper switches AOA only; reception remains unprivileged and the helper does not link GUI/codec libraries.
- Normal access first, elevation only after ACCESS; select one device. Track its physical port chain through AOA VID/PID/address change. Never identify a re-enumerated phone by "first accessory found".
- Short-lived ListCandidates/SwitchSelected operations follow contracts.md. No blocking vendor calls in the GUI, no periodic scanning in Wi-Fi mode, and no elevated enumeration.
- Replace the Sony-only GUI filter and fixed brand label. Establish read-only candidate metadata on Windows/Linux with a non-Sony phone and an unrelated device; metadata does not prove AOA. Keep an explicit device picker in Details for unclassified devices. Only the selected device receives request 51 at Connect. No maker allowlist, broad USB probe, or blanket all-USB permission rule.
- Test 0/1/multiple candidates, duplicate/missing product names, unclassified-device selection and a candidate disappearing before Connect. Hide selection for one; require a deliberate choice for multiple. Preserve the selected port across mode switch rather than selecting the first accessory.
- Remove dependence on the baseline's `0fce:020d` pre-AOA udev rule: use the selected-device ACCESS path when needed and retain generic post-AOA permissions. Test a non-Sony pre-AOA device without a vendor-specific rule. Windows driver binding before/after AOA remains a separate prerequisite; no capability claim until the selected device can be accessed.
- Host-to-phone framing uses separate header and payload bulk transfers. A read-exact abstraction must not accidentally coalesce those writes. Preserve actual AOA string values and requests 51/52/53.
- Preserve header-transfer stale-data resynchronization, endpoint-sized buffers, zero-length-packet deadlines, payload progress/deadlines and libusb context-before-handle cleanup. USB sends no HELLO. Test these through an instrumented transfer fixture before phone tests.
- Visible artifact: selected device before/after transition and G0.5 deterministic integrity output. Run G0 and 1000 PING/PONG exchanges with USB debugging OFF; record exact device and driver/permission environment.
- Accept: physical device continuity including a second attached phone; timeout/unplug/cancel during transition; Android accessory open; no late setup completion starts reception after Stop. Distinguish access/driver failure, failed/zero AOA protocol reply, and unplug. Run native G0/G0.5 on a non-Sony phone as well as the existing test phone; record exact models, OS and drivers, never claim every Android supports AOA. Current WinUSB/libusb driver prerequisites remain visible and are tested separately.
- Delegated: rusb version/features and fixture seams for partial transfers. nusb substitution and driver installation redesign remain deferred.
- Must preserve: other USB devices and existing setup integration; do not scan unrelated devices with vendor requests.

Dependency: 03 and the shared worker/video owner from 04/05. Next: 07.
