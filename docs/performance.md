# Performance ledger

Work that repeats or grows, what it costs, and what was dismissed with a number rather
than a feeling. Numbers are medians of **five interleaved runs** on an AMD Ryzen 7
9800X3D (Ubuntu, kernel 6.x), release build, at `0e15dc2` plus the measurement
extensions in `host-rs/examples/preview_cost.rs`. Reproduce with:

```
cargo build --release --features pipewire --example preview_cost
for i in 1 2 3 4 5; do ./target/release/examples/preview_cost; done
```

The spread matters: a single run of the same code read 320 µs where five interleaved
runs gave 366 µs (343–373). One number was an outlier, and the ledger uses the median.

## Hot paths, per second of streaming

| Path | Rate | Cost | Share of one core |
|---|---|---|---|
| Receive one access unit (allocate + copy payload) | 30/s | 0.45 µs | 0.001 % |
| Preview convert (`preview::from_sink`, 1280x720 → 320x180) | 10/s | 366 µs | 0.37 % |
| Preview publish (record write, 230 KB) | 10/s | 5 µs | 0.005 % |
| GUI preview image (`ColorImage::from_rgba_unmultiplied`, 320x180) | 10/s | 112 µs | 0.11 % |
| Stats event (JSON line) | 1/s | not measured | — |
| Decode + swscale + sink submit | 30/s | **not measured** | **the bulk, unknown** |

The conversion cost follows the *output* size, not the input: 320x240 costs 484 µs for
76 800 pixels and 1280x720 costs 366 µs for 57 600 — both ~6.3 ns per output pixel.
That agreement is the sanity check that the number means what it says.

## Findings

**P1 — the preview runs whether or not anyone watches (0.37 % of a core).**
The worker converts at 10 Hz by design: publishing is a property of the stream, not of
the sink, which is what makes the preview appear before any application attaches to the
camera. *Optional fix:* the GUI could set a "wants frames" byte in the record and the
worker skip the conversion when it is clear. *Not taken:* 0.37 % for an extra piece of
shared state and a new failure mode (stale flag → no preview). Owner: worker.
Acceptance seam: the preview still appears on first attach and after a stall.

**P2 — the GUI rebuilds the preview image per update (0.11 % of a core).**
`ColorImage::from_rgba_unmultiplied` allocates and unmultiplies 230 KB per update; alpha
is always 255, so the unmultiply is wasted arithmetic. *Optional fix:* keep a
`ColorImage` and copy into it. *Not taken:* 0.11 % against a new mutable field and the
risk of a stale-size image. Owner: GUI. Seam: the capture gate's streaming image.

**P3 — the receive path allocates one payload per frame (0.001 %).**
Measured: 0.45 µs for a 100 KB access unit. Dismissed by the measurement, not by an
argument.

## Dismissals (checked, with the reason)

- **Android `updateControls` → `clients()`** — I suspected a store decode per UI update.
  `PhoneAuthorization.clients()` returns the repository's in-memory list
  (`snapshot()` reads a field), and `updateControls` runs on state changes, not per
  frame. No disk I/O.
- **Android encoder copy** — one copy per access unit (`copyRange`); `H264AnnexB.normalize`
  returns the *same array* when the buffer already starts with a start code, which is
  the common case, so there is no second copy.
- **PipeWire sink copy** — `output/pipewire.rs::submit` copies the frame into the shared
  slot, but the worker only submits while `ready()` reports a consumer
  (`State::Streaming`). Nothing is copied for a node nobody watches.
- **`settings.clone()` and `quality.modes.clone()` per repaint** — the same class as P3
  (small allocations, sub-microsecond), and they buy the change detection the UI needs.
- **Per-frame `switch.step()` and `now_ms()`** — one small `Vec` and one vDSO call per
  frame; same class as P3.
- **GUI repaint at 10 Hz while a session runs** — this is a *fix*, not a cost: measured
  earlier, the window otherwise sleeps after the last stats line and the preview and
  the silence banner both freeze.
- **Font loading** — once, in `App::new`; `fontdb` walks system fonts there and not per
  frame.

## Loops and retries: forward progress

- **Auto-reconnect** — at most 8 attempts, 2/4/8 then 15 s apart, reset on success and
  suppressed for failures no retry can fix. The attempt and deadline are on screen.
- **USB inventory** — a thread that parks for 2 s and wakes on demand; no spin.
- **Quality deadline** — a 250 ms tick that exists only while a switch is pending.
- **Session poll** — drains a channel; an empty poll allocates nothing.

## What this ledger does not claim

The bulk of the work is decode, swscale and the sink submit, and none of it is measured
here. A claim that "the preview costs a third of a percent" says nothing about whether
the pipeline is efficient; it says the *added* work is small. The next honest step is a
profile of the worker with a phone attached, which needs the phone.
