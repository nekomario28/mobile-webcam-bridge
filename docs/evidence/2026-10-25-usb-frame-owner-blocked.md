# The frame-owner measurement: attempted, not obtained (2026-10-25)

The one mode the intermittent failures were seen in: an *old* accessory reader holding the endpoint
while a **host** drives frames. `docs/evidence/2026-10-25-usb-reader-release.md` measured what that
guard costs (closing the descriptor releases a blocked reader, 10/10, and two `openAccessory` calls
both succeed) but not this, because it needs a host that writes.

Two instruments were written and both ran; **the measurement itself is not obtained**, and this
document records what was measured, what stopped it, and one wrong diagnosis of mine that the
instruments' own output corrected.

## What was measured, and holds

- **The endpoint carries two sessions' challenges.** With an old session blocked and a second
  session opened on top of it, the host read **two `CLIENT_CHALLENGE` frames (type `0x0a`,
  `sequence=1` each)** off one endpoint. A host that expects one exchange sees two, which is the
  session seam, measured from the host's side for the first time.
- **An extra reader is refused by the driver, not starved of frames.** Every additional reader's
  first read failed immediately with `IOException: read failed: EIO (I/O error)`, in the raw
  two-descriptor measurement and again here. The old reader keeps the endpoint; the newcomer gets
  nothing at all. That is the mechanism behind the documented host symptoms, and it is refusal
  rather than frame theft.
- **Nothing reaches a blocked reader on an idle endpoint.** An armed reader is killed only by its
  own absolute 5 s proof deadline (`PC authorization deadline expired`), which is the control the
  design needed and could not otherwise assume.

## What stopped it

**The host cannot write to the accessory in this configuration.** `send-hello.py` claimed the
accessory pair (`host->phone 0x01`, `phone->host 0x81` — the same endpoints the shipping host
prints with `AMB_USB_TRACE=1`) and read the phone's frames, but its write of one 24-byte
`HOST_HELLO` timed out three times, including once after `clear_halt` and a retry.

That is not a defect of the python script and not the two-session state: the control run with the
phone idle and no session open timed out the same way, and so did the repository's own host in the
same state — `mobile-webcam --usb-probe --frames 2 --budget-ms 6000` reached the accessory, printed
`input ep=0x81 output ep=0x01`, and then reported "retrying after a host hello: deadline expired"
followed by "usb probe could not run: Operation timed out". On 2026-10-24 the same host completed
1000 round trips, so this is a state this environment is in now, not a property of the code.

So the frame never arrived, and the question this document set out to answer — *which session
consumes it* — has no answer here. Saying "the old reader consumed it" would be false: the only
thing that ended the blocked reader was its own deadline.

## A wrong diagnosis, corrected by the instruments

The first version of this attempt blamed the environment for holding the accessory interface: it
tried `claim_interface(device, 1)` — the interface with subclass `0x42` and protocol `1` — got
`EBUSY`, and concluded that adb held the accessory.

That was wrong. `0x42`/`1` is **adb's** interface descriptor; adb holds it legitimately, which is
exactly why the claim failed. The accessory is interface 0, class `0xff` / subclass `0xff`, and it
claims fine — which `host-rs/src/usb.rs` says in its endpoint selection, preferring
`class == 0xff && sub_class == 0xff`. The corrected script claims interface 0 and reads the phone's
frames successfully. The lesson is the one this repository keeps re-learning: identify a surface
from the description the code uses, not from the first descriptor that looks plausible.

## Incident during the attempt, and the restore

Investigating the supposed holder, `pkill -9 -f "adb"` was used. The pattern matched far more than
intended: it matched this session's own shell (the tool call exited 137) and would have matched the
Waydroid container's `adbd` and `adb_root` too (they survived). The phone's adb link was lost — the
device stayed on the bus as `18d1:2d01` but stopped appearing in `adb devices`.

Restore, done on the host and reversible: `authorized=0` then `authorized=1` on
`/sys/bus/usb/devices/1-2` (via sudo), which re-enumerated the phone into its normal mode
(`0fce:320d`), after which `adb devices` listed it again. Two traps worth keeping: **never match
`-f adb`** (it matches `adbd`, `adb_root`, and the command line that contains the pattern), and an
AOA switch drops the phone from adb until it re-enumerates — poll for the phone's own serial, not
for "a device".

## What is still open

The frame-owner question, which needs a USB state where host→phone writes complete — the state the
2026-10-24 runs were in and this one is not. Until then, the honest summary of the single-reader
handover is: the release is measured (10/10), the driver's refusal of a second reader is measured,
the double challenge is measured from the host, and **who consumes a host frame is not**.
