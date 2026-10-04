# 06 — USB AOA and selected-phone continuity

Question: does Rust preserve AOA access with USB debugging off and the Android stream transfer boundary?

- Seam: rusb/libusb transport and a small Linux USB-only elevated helper; reuse the wire/video session rather than a separate USB receiver. The privileged helper switches AOA only; reception remains unprivileged and the helper does not link GUI/codec libraries.
- Normal access first, elevation only after ACCESS; select one device. Track its physical port chain through AOA VID/PID/address change. Never identify a re-enumerated phone by "first accessory found".
- Short-lived ListCandidates/SwitchSelected operations follow contracts.md. No blocking vendor calls in the GUI, no periodic scanning in Wi-Fi mode, and no elevated enumeration.
- Host-to-phone framing uses separate header and payload bulk transfers. A read-exact abstraction must not accidentally coalesce those writes. Preserve actual AOA string values and requests 51/52/53.
- Preserve header-transfer stale-data resynchronization, endpoint-sized buffers, zero-length-packet deadlines, payload progress/deadlines and libusb context-before-handle cleanup. USB sends no HELLO. Test these through an instrumented transfer fixture before phone tests.
- Visible artifact: selected device before/after transition and G0.5 deterministic integrity output. Run G0 and 1000 PING/PONG exchanges with USB debugging OFF; record exact device and driver/permission environment.
- Accept: physical device continuity including a second attached phone; timeout/unplug/cancel during transition; Android accessory open; no late setup completion starts reception after Stop. Current WinUSB/libusb driver prerequisites remain visible and are tested separately.
- Delegated: rusb version/features and fixture seams for partial transfers. nusb substitution and driver installation redesign remain deferred.
- Must preserve: other USB devices and existing setup integration; do not scan unrelated devices with vendor requests.

Dependency: 03 and the shared worker/video owner from 04/05. Next: 07.
