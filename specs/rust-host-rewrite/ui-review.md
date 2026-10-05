# UI sketch checks

2026-10-04. Target: connection mode, endpoint and one primary action; short guidance when action is needed. Settings and sketch controls start collapsed.

Browser observations:

- Edited IP restored after reload; save/restore produced no success text.
- Connect -> Cancel -> idle worked. Idle status is hidden; camera-start wait exposes Stop.
- Empty IP shows one prompt, focuses the field and keeps Connect.
- All six state examples retained their action and endpoint-editing behavior.
- Linux exposes its output field and VAAPI; Windows-only consumer wait is disabled.
- Android's actual Japanese button is `カメラ開始`; the sketch prompt uses that label.
- USB with one fixture phone has no selector or brand/mode option; Connect is available and the idle card reads only Mobile Webcam, USB/Wi-Fi, Connect and Details.
- Zero fixture phones shows a USB-connect prompt and disables Connect. Two show distinguishable port labels and require a choice before enabling Connect. Cancel remains available during setup; the one-phone camera-start wait retains Stop.
- Browser automatic mode initialized in Japanese. EN/JA preview checks covered all six states, labels, Details and blank-IP guidance. Switching language preserved the selected phone, decoder value, IP and failure kind; during a pending connection it retained Cancel, reached the translated camera-start wait and retained Stop. Reload restored the saved IP and returned to automatic language, with both expandable sections closed.
- The actual sketch locale function passed six isolated Node fixtures: Japanese region/script tags, English primary preference, unsupported primary preference, navigator.language fallback and missing locale. All 47 copy entries contain both languages and every localized DOM key resolves. Linux output/backend and USB zero-phone guidance were checked in English too.

The USB single-phone screenshot shows the selector omitted; the multiple/zero-phone DOM checks verify the conditional controls. The candidate shows a single card without the explanatory heading, paragraphs, OS badge, success message, ready panel or scenario column. Fixture port names stand in for actual device descriptors; the unclassified-device picker in contracts.md is not simulated. DOM and JavaScript checks cover the sketch only; native discovery, Rust/IME/DPI/camera acceptance remains NOT RUN.

Adversarial self-review (independent image critique unavailable): hidden guidance could leave users without a next step, but waiting/failure states retain explicit actions. Collapsed Details could conceal transforms, but its labeled control remains visible. Removing success text could conceal save failures, so an independent inline save error remains visible on failure in either transport.

Hiding a sole phone could conceal which device will be used; native discovery must exclude unrelated devices from automatic selection and revalidate the selected port at Connect. The fallback picker remains explicit when metadata is inconclusive. Port suffixes distinguish same-name fixture phones but native labels still require actual descriptor/hotplug checks in 06.

English labels can expand beyond the Japanese layout; the English Details screenshot remains legible in this browser viewport. Automatic language removes a setup choice; only the collapsed sketch fixtures expose an override. Native EN/JA layout, locale retrieval and mixed PC/phone language checks remain NOT RUN.

The candidate meets the minimal-copy target. Native visual acceptance still requires compare-screenshots and an unprimed screenshot-critique in slice 02.
