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
| Decode (H.264 720p, software) | 30/s | 0.22 ms | 0.7 % |
| Swscale to the sink layout (720p YUYV) | 30/s | 0.61 ms | 1.8 % |
| Whole frame loop, fake sink | 30/s | 1.47 ms/frame → 660–680 fps | **4.4 %** |

The conversion cost follows the *output* size, not the input: 320x240 costs 484 µs for
76 800 pixels and 1280x720 costs 366 µs for 57 600 — both ~6.3 ns per output pixel.
That agreement is the sanity check that the number means what it says.

## The pipeline itself (measured 2026-10-22)

`host-rs/src/video.rs::worker_throughput`, an ignored in-crate test that runs the real
`receive_stream` over a committed fixture — 90 access units of 1280x720 H.264, one per
frame, with the sink replaced by a counter:

```
cargo test --release --features pipewire --lib worker_throughput -- --ignored --nocapture
frames 90  decode 0.22 ms/frame  convert 0.61 ms/frame  pipeline 1.47 ms/frame  (660–680 fps)
```

Three interleaved runs: decode 0.18–0.23 ms, convert 0.59–0.61 ms, pipeline
1.47–1.52 ms. At 30 fps the loop occupies **~4.4 % of one core** — 22x the headroom the
frame rate needs. The pipeline figure is a little more than decode plus convert because
it includes the loop's own step (a small allocation per frame, `now_ms`, the transport
poll) and a watchdog that ends the run within 5 ms (≤0.06 ms per frame).

Caveats, stated rather than implied: the sink is a counter, so the copy into a v4l2 or
PipeWire buffer is **not** in these numbers; hardware decode is not exercised
(`decode: "off"`); and the fixture is synthetic at CRF 30, so real camera bitrates
cost more in the decode line and not in the others.

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

## Phone side (read 2026-10-22)

Read through `android/app/src/main/java/dev/nekomario/amb/` (4 556 lines) for work that
repeats or grows. Two findings, one fixed; the rest is a list of things that are right.

**Fixed — the binding was computed the long way on every connection.** Each accepted
Wi-Fi connection read the certificate out of the Android Keystore, hashed it, formatted
32 bytes as hex, and then parsed that hex straight back into bytes; the access key was
hex-decoded per connection too. `SecureLanIdentity` now caches the certificate DER (and
clears it if the key is regenerated), answers `fingerprintBytes()`, derives the hex form
from those bytes, and keeps the decoded secret — refreshed on rotation. One
constant-time compare remains (`MessageDigest.isEqual`), and a wiring test fails if the
session goes back to `fingerprint().chunked(2)`.

**Reported, needs the device — screen-off capture is unmeasured, and the flag does not
mean what it looks like.** `MainActivity` sets `FLAG_KEEP_SCREEN_ON` while capture runs.
An earlier version of this ledger said that keeps a two-hour screen awake for a two-hour
stream; **that was wrong** — the flag applies only while its window is visible, so a
backgrounded session does not hold the screen on at all. What it does do is keep the
screen awake while the user is *looking at the app*. The open question is different and
unrelated to the flag: whether capture and the encoder survive the screen turning off on
this handset, which is unmeasured and is what the roadmap's two-hour item has to answer.
Until then the flag stays, because it costs nothing when the app is in the background and
removing it would not test anything.

**Micro, dismissed:** `Wire.writeFrame` allocates a 24-byte `ByteBuffer` per sent frame
(30/s) — the same class as the 0.45 µs measured on the Rust side. `FramedOutbound`'s
`wake` semaphore can keep permits belonging to frames that were dropped by
`clearVideo()`; the writer consumes them as empty wakeups, so it is a few spurious
iterations and not a leak.

**Waste that is not there** (checked, so nobody has to re-check): the outbound queues
are bounded (2 video frames, 32 control) and overflow clears the queue and requests an
IDR rather than growing; the encoder copies each access unit once and
`H264AnnexB.normalize` returns the same array when it is already Annex-B; TLS handshakes
are limited to four workers with an absolute 5 s deadline, and closing the server wakes
them; `refreshQualityCapabilities()` (a Camera2 + MediaCodec capability scan) runs on
four events — resume, permission result, USB attach, LAN start — and never per frame.
`MainActivity` is 1 096 lines and carries the UI, session, camera and quality wiring;
that is a structure note for a future split, not a defect.

## Soak: does the host drift over time?

`host-rs/src/video.rs::worker_soak`, ignored like the throughput test: a fake transport
paces the same 720p fixture at 30 fps for `SOAK_SECONDS`, the preview record is
published at its usual 10 Hz, and resident memory and throughput are sampled every 15 s.

```
SOAK_SECONDS=300 cargo test --release --features pipewire --lib worker_soak -- --ignored --nocapture
t=  15.0s frames=   454 rss=   28.0 MB
t= 120.0s frames=  3631 rss=   28.0 MB (+0.00 MB/min) 30.26 fps
t= 300.0s frames=  9076 rss=   28.0 MB (+0.00 MB/min) 30.25 fps
soak done: 300 s, 9076 access units, result Ok(())
```

Five minutes, 9 076 frames: **resident memory flat at 28.0 MB** (28.3 MB in the sample
taken after the stop), **30.25 fps throughout** with no downward drift. No leak
signature in the host worker under this load.

What it is not: five minutes is not two hours, the transport is synthetic (no phone, no
link), the sink counts instead of writing to a device, and the post-stop sample shows
28.8 fps only because it averages the teardown tail. A leak that appears at hour one
would not show up here.

**The CPU instrument was removed rather than reported.** The first version read
`/proc/self/stat` (and then `getrusage`) and printed **0.0 s** for a process that had
been decoding for minutes. Rather than publish a number nothing could distinguish from
a broken probe, the column is gone and the per-frame cost comes from `worker_throughput`
(1.47 ms, 4.4 % of a core at 30 fps).

## What this ledger does not claim

- The sink's own copy is out of scope (the benchmark counts submissions instead of
  writing into a device buffer), so a v4l2 or PipeWire path adds its copy on top.
- Hardware decode, the phone's encoder, USB or Wi-Fi throughput, and the phone's power
  draw are all somebody else's measurement. The 4.4 % above says the *host* has room;
  it says nothing about whether a phone can keep it fed.
- The fixture is synthetic. A real camera at a higher bitrate shifts cost into the
  decode line.
- Long-run behaviour (thermal, leaks over hours) is not measured at all.
