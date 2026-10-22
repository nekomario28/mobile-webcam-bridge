# The preview's cost (2026-10-21)

The worker converts a preview frame at a fixed 10 Hz whether or not a consumer is
attached, so the conversion sits on the streaming path. This records what it costs.

## Method

`host-rs/examples/preview_cost.rs`, which asks the real code path
(`preview::from_sink` — the same function the worker calls — and
`preview::Producer::publish` into a real record file), 200 iterations per mode, mean
of the per-iteration wall clock. Machine: AMD Ryzen 7 9800X3D, 16 threads, Ubuntu with
kernel 6.x. Source at `24aca6d`.

```
cargo build --release --features pipewire --example preview_cost
./target/release/examples/preview_cost
```

## Result

| source mode | preview conversion | publish (record write) | share of one core at 10 Hz |
|---|---|---|---|
| 320x240 | 426 µs | 7.4 µs | 0.43 % |
| 640x480 | 426 µs | 6.0 µs | 0.43 % |
| 1280x720 | 320 µs | 4.8 µs | 0.33 % |
| 1920x1080 | 321 µs | 4.9 µs | 0.33 % |

The cost follows the *output* size, not the input: the preview is scaled to 320
columns, so 1280x720 and 1920x1080 both produce 320x180 and cost the same ~320 µs.
The debug build measures 1.7–2.3 ms for the same work, five times slower; the shipped
binary is a release build, and the debug figure is not the one to quote.

## What this does not say

- It is not an end-to-end CPU figure: decode, the sink conversion and the transport
  dominate, and none of them are in this number.
- It is not measured with the window open (texture upload happens in the GUI process,
  once per repaint) or with a phone attached (the same function, different source
  frames).
- One machine, one run of the mean; the spread was not recorded.
