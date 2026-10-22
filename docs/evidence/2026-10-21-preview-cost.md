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

## Correction (five interleaved runs)

A single run of the same code read 320 µs for 1280x720; five interleaved runs give
**366 µs median, spread 343–373**. The first number was an outlier and the figure below
replaces it. The ledger (`docs/performance.md`) is where the method lives.

| source mode | preview conversion (median, spread) | publish | share of one core at 10 Hz |
|---|---|---|---|
| 320x240 | 484 µs (467–505) | 6.6 µs | 0.49 % |
| 640x480 | 488 µs (476–504) | 6.7 µs | 0.49 % |
| 1280x720 | 366 µs (343–373) | 4.9 µs | 0.37 % |
| 1920x1080 | 358 µs (339–381) | 5.0 µs | 0.36 % |

Beyond the worker: the GUI's `ColorImage::from_rgba_unmultiplied` for the same 320x180
picture costs 112 µs per update (0.11 % of a core at 10 Hz), and one received access
unit's payload allocation costs 0.45 µs (0.001 % at 30 fps).

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
