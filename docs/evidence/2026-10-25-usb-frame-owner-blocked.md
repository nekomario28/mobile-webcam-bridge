# The frame-owner measurement, and the precondition that blocked it (2026-10-25)

The one mode the intermittent failures were actually seen in: an *old* accessory reader holding the
endpoint while a **host** drives frames. `docs/evidence/2026-10-25-usb-reader-release.md` measured
what that guard costs (closing the descriptor releases a blocked reader, 10/10, and two
`openAccessory` calls both succeed) but not this, because it needs a host that writes.

Two instruments were written for it, and **neither has ever run**:

- device side: `UsbFrameOwnerMeasurement` (behind `-e usbFrameOwner true`), which arms an old
  session, opens a second one on top of it, proves the old reader is still blocked after a control
  window of silence, then waits for the host's single frame and reports who consumed it;
- host side: `host-rs/tools/send-hello.py` (pyusb), which writes exactly one `HOST_HELLO` and
  prints the frames it read before and after.

This document is the reason they could not run, so the next attempt does not repeat the probing.
It measures nothing.

## The blocker, with its evidence

**A host process cannot claim the accessory interface while adb is attached.**

- `usb.util.claim_interface(device, 1)` → `USBError [Errno 16] Resource busy`, for interface 1 —
  the accessory (class `0xff`, subclass `0x42`, endpoints `0x02` out / `0x82` in). Interface 0
  claims fine in the same process, so this is not a permissions or driver-binding problem.
- `fuser` on the phone's usbfs node names `adb` (the server pid), and the server cannot be taken
  out of the way: a Waydroid session keeps an `adb connect 192.168.240.112:5555` loop running, so
  the server is back within a second of being killed, and it re-holds the node.
- With that server running, the repository's own host reached the AOA transition
  (`0fce:320d` → `18d1:2d01`) but then timed out without ever seeing a challenge — "retrying after
  a host hello: deadline expired", then "usb probe could not run: Operation timed out" — which is
  what an accessory endpoint that cannot be claimed looks like from the driver.

This is not a new discovery about the machine, it is the repository's own stated precondition:
`docs/gates.md` G0 lists **USB debugging OFF**. With debugging off the phone exposes
accessory-only and adb is not on the bus, which is the configuration every measured run of
2026-10-24 was in.

## Why both halves are needed at once, which is the actual conflict

The device side must be started and must report (that is adb), and the host side must write one
raw frame (that is the accessory endpoint exclusively). USB debugging on gives adb the endpoint;
USB debugging off takes adb away — and toggling it re-enumerates the USB, which destroys the armed
sessions the device side is holding. So the measurement needs a channel that survives the switch:

- adb over Wi-Fi (`adb tcpip 5555`, then `adb connect <phone-ip>`) with the USB function moved to
  the accessory, so no host process claims the USB interface; or
- an arming step followed by a physical toggle of USB debugging, with the result read afterwards
  from logcat, which survives because the device-side process keeps running.

Either one needs a physical action available as a fallback, which is why this stopped here.

## An incident during the attempt, and the restore

Investigating the holder, `pkill -9 -f "adb"` was used and the pattern matched far more than
intended: it matched this session's own shell (the tool call exited 137), and it would have matched
the Waydroid container's `adbd` and `adb_root` as well (they survived). The phone's adb link was
lost: the device stayed on the bus as `18d1:2d01` but stopped appearing in `adb devices`.

Restore, done on the host and reversible: `authorized=0` then `authorized=1` on
`/sys/bus/usb/devices/1-2` (via sudo), which re-enumerated the phone into its normal mode
(`0fce:320d`), after which `adb devices` listed it again. Recorded because the trap will recur:
**never match `-f adb`** — it matches `adbd`, `adb_root` and the command line that contains the
pattern itself. Use `pgrep -x adb` or an exact pid.

## What is still unmeasured

The requested mode: a host driving frames while an old reader holds the endpoint. Everything else
about the single-reader handover has a number now; this needs a configuration in which the host
owns the endpoint while the device side can still report.
