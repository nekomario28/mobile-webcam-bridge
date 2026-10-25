# Who consumes a host frame while a blocked reader holds the endpoint (2026-10-25)

The last unmeasured mode of the single-reader handover: an old accessory reader holds the
endpoint, and a **host** writes. `docs/evidence/2026-10-25-usb-reader-release.md` measured what
the guard costs (closing the descriptor releases a blocked reader, 10/10) but not this, because it
needs a host that writes. It is measured now.

## The result

One run, both halves instrumented, no inference from either side's narrative:

```
host:  claiming interface 0: host->phone 0x01, phone->host 0x81
host:  read before writing: 90 bytes -> ['type=0x0a sequence=1 payload=66']      # exactly one challenge
host:  writing one HOST_HELLO frame: 24 bytes                                   # the write completed
host:  read after writing: 0 bytes
device: iteration 1: readerAlive=true stack=libcore.io.Linux.readBytes <- libcore.io.Linux.read
device:   iteration 1 ended: PC authorization proof rejected
device: outcome: the blocked reader consumed the host's frame (after 1 iteration(s))
```

`PC authorization proof rejected` is the reason `PhoneAuthorization.receive` gives when a frame
that is not a `CLIENT_AUTH` proof reaches a session that is still waiting for its proof: the
`require(header.type == Wire.TYPE_CLIENT_AUTH)` fails, the session is ended there. So the frame the
host wrote **reached the reader that held the endpoint and was refused at that session's proof
gate** - the reader's stack was inside the read syscall when the close came, and the reason is not
the one a deadline produces.

The control is in the same instrument rather than assumed: with the endpoint idle, iterations end
with `PC authorization deadline expired` - the reader's own absolute 5 s proof deadline - and never
with `PC authorization proof rejected`. So the string is attributable to the host's frame.

## What each device state does to the host

| State of the phone | Host reads | Host writes |
|---|---|---|
| one descriptor, one reader blocked | one `CLIENT_CHALLENGE` (`0x0a`, sequence 1) | **24 bytes, completed** |
| one reader blocked, plus one extra descriptor held with no reader on it | one `CLIENT_CHALLENGE` | **24 bytes, completed** |
| one reader blocked, plus an extra descriptor carrying a second session whose reader the driver refused with `EIO` | **five** `CLIENT_CHALLENGE` frames, from the instrument's own re-arms | **24 bytes, completed** |
| phone idle in accessory mode, nothing open | nothing | **timeout** |

The last row is mechanistically expected - nothing is draining the phone's OUT endpoint, so the
host's transfer is NAKed until it times out - and it matters because it is the state the *first*
attempt kept measuring in.

The middle two rows resolve the attribution that attempt could not make. The shape of the device-side
session is **not** what stopped the host's write: it completes with an extra descriptor held, and
again with a second *session* on that descriptor whose reader the driver refuses with `EIO` - which
is the failing run's state reproduced. The earlier timeout therefore belongs to the port state, the
cable having been re-seated between the two observations, and not to the descriptor count, the second
reader, or anything the instrument controls. That is one observation of a port state which is not
reproducible on demand, and it should be read as exactly that.

The five challenges are a second, unplanned datum, and the review request's hypothesis A1 is about
precisely this: AOA reports no disconnect, so the endpoint keeps what earlier sessions wrote. Three
re-armed measured sessions plus their second sessions left five `0x0a` challenges for the host to
drain before it wrote. Nothing had been written *towards* the phone, the verdict came after the
host's single `0x0e`, which is what makes the verdict attributable here - but a host arriving later
finds a stream of other sessions' challenges, and that is the case A1 asks a reviewer to attack.

## The wrong diagnosis this corrected

The first version of this attempt blamed the environment for holding the accessory interface. It
tried `claim_interface(device, 1)` - subclass `0x42`, protocol `1` - got `EBUSY`, and concluded
that adb held the accessory. That descriptor is **adb's**; adb holds it legitimately, which is
exactly why the claim failed. The accessory is interface 0, class `0xff` / subclass `0xff`, and it
claims fine - as `host-rs/src/usb.rs` says in its endpoint selection, which prefers
`class == 0xff && sub_class == 0xff`. The corrected script claims interface 0 and both reads and
writes the phone's data. Identify a surface from the description the code uses, not from the first
descriptor that looks plausible.

## The instrument's own bug, found by its first run

It counted any session error as consumption, so its first run reported `consumed=true` on an `EIO`
that arrived before any host frame. Only a session-level refusal counts now (`proof rejected`); a
deadline is the control and `EIO` is the driver refusing that reader.

## Incident during the attempt, and the restore

Investigating the supposed holder, `pkill -9 -f "adb"` was used. The pattern matched far more than
intended: it matched this session's own shell (the tool call exited 137) and would have matched the
Waydroid container's `adbd` and `adb_root` too (they survived). The phone's adb link was lost - the
device stayed on the bus as `18d1:2d01` but stopped appearing in `adb devices`.

Restore, on the host and reversible: `authorized=0` then `authorized=1` on
`/sys/bus/usb/devices/1-2` (via sudo), which re-enumerated the phone into its normal mode
(`0fce:320d`), after which `adb devices` listed it again. Two traps worth keeping: **never match
`-f adb`** (it matches `adbd`, `adb_root`, and the command line that contains the pattern), and an
AOA switch drops the phone from adb until it re-enumerates - poll for the phone's own serial, not
for "a device".

## Verification boundary

- One device, one host, one run of the result above, with the control in the same run. The two
  other rows of the table are single observations, and the two-descriptor one predates the cable
  being re-seated.
- `UsbFrameOwnerMeasurement` needs the accessory attached and the app's USB grant, so it is a
  device instrument, run by hand like `--usb-probe`; nothing in CI exercises it (the emulator has
  no accessory).
- Not measured: what happens to *the app* in the two-descriptor state (the Activity's refusal path
  is pinned by `SessionServiceWiringTest`, and the release by the reader-release measurement), and
  whether two descriptors can coexist at all on another platform or Android version.
