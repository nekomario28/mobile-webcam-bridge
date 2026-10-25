# G0's "USB debugging does not have to be off", under load (2026-10-25)

The third review left this open: the G0 precondition in `docs/gates.md` says USB debugging need not be
off, because the phone then exposes two interfaces and the host claims the accessory one while adb
holds its own — but it had only been observed with adb **idle** (a `dumpsys`, an `am instrument`). The
question was whether the claim holds while adb is actually carrying traffic.

## What was measured

A 20 MiB `adb push` (`--sync`, byte-exactness verified with `sha256sum` on both sides) on the XQ-GE44
with the phone in accessory mode (`18d1:2d01`) and adb attached, in five states:

| Arm | Phone side | Arm's push |
|---|---|---|
| 1 | no session, nothing open | **39.6 MB/s** |
| 2 | frame-owner instrument: one blocked reader, host writes one frame | **3.2 MB/s** |
| 3 | shipping app launched after the switch (no attach intent, so nothing open) | 40.1 MB/s |
| 4 | nothing open (app stopped) | 40.1 MB/s |
| 5, 6 | arm 2 repeated | **3.2 MB/s**, **3.2 MB/s** |
| 7, 8 | shipping app **received and handled the accessory attach intent**, then pushed | 40.7 MB/s, 40.6 MB/s |

Every run with the instrument also produced the accessory-side result the instrument exists for: the
host's 24-byte frame was consumed by the blocked reader, `PC authorization proof rejected`, with the
frame's identity confirmed (`type=0x0e sequence=4660`).

## What this answers, and what it does not

**The claim holds, and the product path shows no cost.** With debugging on: the phone re-attaches to
adb within 2-4 s of the accessory switch, adb carries a 20 MiB push **byte-exact** while the accessory
session runs and the host writes to it, and the pushes in arms 3, 4, 7 and 8 all ran at ~40 MB/s —
including the arm where the app actually received `USB_ACCESSORY_ATTACHED` and handled it.

**The instrument's presence cost 12x, in every run, and the cause is not attributed.** Arms 2, 5 and 6
are three separate runs of the same shape: 3.2 MB/s each, reproduced. Which part of the instrument does
it — the blocked read on the accessory endpoint, the descriptor churn as iterations close and re-arm,
the test process itself, or `am instrument -w`'s own stream over the same adb connection — was **not
separated**, and this document does not claim a mechanism. It is recorded because it is reproducible
and because it is a trap for the next person who measures adb throughput while an instrumentation is
running: they will attribute the 12x to the thing under test.

## Evidence boundary

One device (XQ-GE44, API 37), one host, one payload, three repetitions of the slow arm and one of each
other arm. Rates are wall-clock for `adb push --sync` of an incompressible 20 MiB file to
`/data/local/tmp`, which includes adb's own handshake; they are not a USB bus measurement. The
sha256 check covers the *transfer*, not the accessory's own frames, which the instrument verified
separately.
