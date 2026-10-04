# UI sketch review boundary

2026-10-04. Reviewed `visualizations/ui.html` in the Codex in-app browser. This is a self-contained HTML sketch, not the Rust/native GUI.

## Interaction observations

- Edited the example address to `192.168.1.77`, pressed Connect, stopped, reloaded: the browser sketch restored the edited address.
- Connect showed Cancel and disabled endpoint changes; the simulated timer moved to the phone-camera waiting state with Stop.
- Exercised all six scenario controls: idle/connecting/phone-wait/consumer-wait/submitting/error had Connect/Cancel/Stop/Stop/Stop/Reconnect respectively; endpoint editing was enabled only while idle/failed.
- Linux selection exposed `/dev/video10` in Details and disabled the Windows-only consumer-wait example.

These observations validate the sketch's controls only. Product persistence is still the native QSettings-compatible contract; no camera/network/GPU operation ran.

## Visual review

The initial viewport screenshot showed the full product card, a legible address field, one blue primary action and the separate scenario column. An independent image critique is **NOT RUN**: the native subagent interface was unavailable and an attempted local screenshot-export aid using a data URL was rejected by browser URL policy. That route was stopped, not bypassed. The interactive HTML remains the review artifact.

Adversarial self-check, based on that visible screenshot:

| Feature | Strongest visible case against it | Design decision |
|---|---|---|
| Primary action | The scenario column adds several competing buttons to the full screenshot | Keep the simulator visibly outside the product card; it is never shipped in the app |
| Saved-IP hint | Its smaller gray text is weaker than the address and status heading | Treat as secondary feedback, keep readable native text scaling/contrast as slice 02 checks |
| Details discovery | Collapsing Details hides transform/decode controls on first view | Keep a labeled disclosure below the primary action; verify keyboard/large-text discovery in 02 |
| State recognition | The gray status dot alone communicates little | State title and next action carry meaning; the dot is supplemental |

This is a provisional design judgment, not visual acceptance of a native implementation. Native screenshots, DPI/IME/accessibility and an unprimed screenshot-critique remain explicit slice 02 gates. Do not use the mock or this self-check to mark those gates passed.
