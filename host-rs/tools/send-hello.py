"""Do the AOA switch if needed, then write one AMB HOST_HELLO frame, and report what was read.

The host half of `UsbFrameOwnerMeasurement` (under `-e usbFrameOwner true`), for
docs/evidence/2026-10-25-usb-frame-owner.md: an old accessory reader holds the endpoint, a second
session opens on top of it, and this writes exactly one frame so the device side can say which
session consumed it.

Endpoint choice is taken from the shipping host, not guessed: `host-rs/src/usb.rs` prefers the
interface whose class and subclass are both 0xFF and uses that interface's bulk pair. The
interface with subclass 0x42 and protocol 1 is **adb's**, and claiming that one fails with EBUSY
because adb holds it legitimately.

Requires pyusb, which is not a repository dependency.
"""
import struct, sys, time
import usb.core, usb.util

MAGIC = 0x31424D41          # "AMB1" little-endian (Wire.MAGIC)
VERSION = 1
HOST_HELLO = 0x0E
# A copy of `host-rs/src/usb.rs`'s identity strings, which own them (the Android side repeats them
# again in `res/xml/accessory_filter.xml`). A change there has to be mirrored here. This script exists
# because the repository's CLI has no switch-only mode, and the other half of the experiment - writing
# one raw AMB frame from outside the process - is something only pyusb can do.
AOA_STRINGS = [
    "Android Media Bridge",
    "AMB Host",
    "Low-latency Android to Linux media bridge",
    "0",
    "https://github.com/nekomario28/mobile-webcam-bridge",
    "amb-g0",
]
ACCESSORY_IDS = [(0x18D1, pid) for pid in (0x2D00, 0x2D01, 0x2D02, 0x2D03, 0x2D04, 0x2D05)]
NORMAL_IDS = [(0x0FCE, 0x320D)]


def find(ids):
    for vid, pid in ids:
        device = usb.core.find(idVendor=vid, idProduct=pid)
        if device is not None:
            return device
    return None


def switch_to_accessory(device):
    protocol = device.ctrl_transfer(0xC0, 51, 0, 0, 2, timeout=1200)
    if len(protocol) != 2 or protocol[0] == 0 and protocol[1] == 0:
        raise RuntimeError("this device does not support AOA")
    for index, value in enumerate(AOA_STRINGS):
        payload = value.encode() + b"\0"
        sent = device.ctrl_transfer(0x40, 52, 0, index, payload, timeout=1200)
        if sent != len(payload):
            raise RuntimeError("short AOA identification transfer")
    device.ctrl_transfer(0x40, 53, 0, 0, b"", timeout=1200)


def accessory_interface(device):
    for config in device:
        for candidate in config:
            if candidate.bInterfaceClass == 0xFF and candidate.bInterfaceSubClass == 0xFF:
                inp = out = None
                for endpoint in candidate:
                    if endpoint.bmAttributes & 0x03 != 0x02:
                        continue
                    if endpoint.bEndpointAddress & 0x80:
                        inp = endpoint.bEndpointAddress
                    else:
                        out = endpoint.bEndpointAddress
                if inp is not None and out is not None:
                    return candidate.bInterfaceNumber, inp, out
    return None


def frames(buffer):
    offset, found = 0, []
    while offset + 24 <= len(buffer):
        magic, version, ftype, flags, sequence, length, pts = struct.unpack_from("<iBBhiiq", buffer, offset)
        if magic != MAGIC:
            found.append("(not an AMB header at offset %d: %s)" % (offset, hex(magic)))
            break
        found.append("type=0x%02x sequence=%d payload=%d" % (ftype, sequence, length))
        offset += 24 + length
    return found


def main():
    # `--switch-only` performs the AOA switch and exits, for experiments that need the phone in
    # accessory mode while adb stays attached (G0's claim that USB debugging need not be off).
    if "--switch-only" in sys.argv:
        device = find(ACCESSORY_IDS) or find(NORMAL_IDS)
        if device is None:
            print("FAIL: no device")
            return 2
        if find(ACCESSORY_IDS) is not None:
            print("already an accessory")
            return 0
        switch_to_accessory(device)
        deadline = time.time() + 10
        while time.time() < deadline:
            time.sleep(0.3)
            if find(ACCESSORY_IDS) is not None:
                print("accessory mode entered")
                return 0
        print("FAIL: the phone did not re-enumerate as an accessory")
        return 2
    settle_seconds = float(sys.argv[1]) if len(sys.argv) > 1 else 2.0
    # An optional ptsUs, so the operator can hand the same fresh value to the instrument
    # (`-e hostPts <value>`) before the write happens, making the ownership verdict attributable
    # rather than merely plausible (independent review, 2026-10-25, second pass, R2-001).
    chosen_pts = int(sys.argv[2]) if len(sys.argv) > 2 else None
    device = find(ACCESSORY_IDS)
    if device is None:
        device = find(NORMAL_IDS)
        if device is None:
            print("FAIL: neither an accessory nor the phone in its normal mode is attached")
            return 2
        print("switching the phone to accessory mode")
        switch_to_accessory(device)
        deadline = time.time() + 10
        while time.time() < deadline:
            time.sleep(0.3)
            device = find(ACCESSORY_IDS)
            if device is not None:
                break
        if device is None:
            print("FAIL: the phone did not re-enumerate as an accessory")
            return 2
    print("accessory present")
    chosen = accessory_interface(device)
    if chosen is None:
        print("FAIL: no interface with class/subclass 0xFF (the accessory pair)")
        return 2
    interface, inp, out = chosen
    print("claiming interface %d: host->phone 0x%02x, phone->host 0x%02x" % (interface, out, inp))
    try:
        device.set_auto_detach_kernel_driver(True)
    except Exception:
        pass
    usb.util.claim_interface(device, interface)
    try:
        # Let the device-side harness arm (it needs ~4 s once the accessory exists), and read
        # what the two sessions send: two challenges on one endpoint is already the finding.
        noise = bytearray()
        end = time.time() + settle_seconds
        while time.time() < end:
            try:
                noise += bytes(device.read(inp, 4096, timeout=300))
            except usb.core.USBTimeoutError:
                pass
        print("read before writing:", len(noise), "bytes ->", frames(noise))
        # A distinctive sequence, because the device instrument now requires the frame it reads to
        # be this one rather than any frame that provokes the same refusal (review 2026-10-25, ID-001).
        # ptsUs carries a value fresh for this run, and it is printed: type and sequence are constant
        # across runs, so without a fresh discriminator an earlier identical write satisfies the same
        # comparison (independent review, 2026-10-25, second pass, R2-001).
        pts = chosen_pts if chosen_pts is not None else int(time.time() * 1000)
        header = struct.pack("<iBBhiiq", MAGIC, VERSION, HOST_HELLO, 0, 0x1234, 0, pts)
        assert len(header) == 24
        try:
            print("writing one HOST_HELLO frame:", device.write(out, header, timeout=4000), "bytes")
            print("host frame: type=0x0e sequence=%d ptsUs=%d  <- pass -e hostPts %d to the instrument"
                  % (0x1234, pts, pts))
        except usb.core.USBTimeoutError as error:
            print("write timed out (%s); clearing the halt and retrying once" % error)
            try:
                device.clear_halt(out)
            except Exception as halt:
                print("clear_halt failed:", halt)
            time.sleep(0.5)
            try:
                print("retry wrote:", device.write(out, header, timeout=8000), "bytes")
            except Exception as retry:
                print("retry failed too:", type(retry).__name__, retry)
        after = bytearray()
        end = time.time() + 3
        while time.time() < end:
            try:
                after += bytes(device.read(inp, 4096, timeout=300))
            except usb.core.USBTimeoutError:
                pass
        print("read after writing:", len(after), "bytes ->", frames(after))
        return 0
    finally:
        usb.util.release_interface(device, interface)


if __name__ == "__main__":
    sys.exit(main())
