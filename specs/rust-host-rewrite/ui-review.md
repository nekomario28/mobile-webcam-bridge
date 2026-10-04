# UI sketch checks

2026-10-04. Target: connection mode, endpoint and one primary action; short guidance when action is needed. Settings and sketch controls start collapsed.

Browser observations:

- Edited IP restored after reload; save/restore produced no success text.
- Connect -> Cancel -> idle worked. Idle status is hidden; camera-start wait exposes Stop.
- Empty IP shows one prompt, focuses the field and keeps Connect.
- All six state examples retained their action and endpoint-editing behavior.
- Linux exposes its output field and VAAPI; Windows-only consumer wait is disabled.
- Android's actual Japanese button is `カメラ開始`; the sketch prompt uses that label.

Same-viewport idle captures differed after reload. The candidate shows a single card without the explanatory heading, paragraphs, OS badge, success message, ready panel or scenario column. DOM and JavaScript checks cover the sketch only; native Rust/IME/DPI/camera acceptance remains NOT RUN.

Adversarial self-review (independent image critique unavailable): hidden guidance could leave users without a next step, but waiting/failure states retain explicit actions. Collapsed Details could conceal transforms, but its labeled control remains visible. Removing success text could conceal save failures, so an independent inline save error remains visible on failure in either transport.

The candidate meets the minimal-copy target. Native visual acceptance still requires compare-screenshots and an unprimed screenshot-critique in slice 02.
