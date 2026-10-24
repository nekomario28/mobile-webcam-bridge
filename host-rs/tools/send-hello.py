"""Write one AMB HOST_HELLO frame to an attached AOA accessory, and report what was read.

NEVER RUN. This is the host half of `UsbFrameOwnerMeasurement`; the pair could not be run on
2026-10-25 because the host cannot claim the accessory interface while adb is attached — see
docs/evidence/2026-10-25-usb-frame-owner-blocked.md, which contains the evidence and what
would unblock it. Requires pyusb, which is not a repository dependency.

Local measurement tool for docs/evidence/2026-10-25-usb-frame-owner.md: the phone side
(UsbFrameOwnerMeasurement) announces READY-FOR-HOST-FRAME and this writes exactly one frame.
The accessory interface is identified by its subclass (0x42), not guessed.
"""
import struct, sys, time
import usb.core, usb.util

MAGIC = 0x31424D41          # "AMB1" little-endian, Wire.MAGIC
VERSION = 1
HOST_HELLO = 0x0E
OUT, IN = 0x02, 0x82        # accessory interface 1: host->phone OUT, phone->host IN


def main():
    device = usb.core.find(idVendor=0x18D1, idProduct=0x2D01)
    if device is None:
        print("FAIL: the accessory is not attached (18d1:2d01)")
        return 2
    interface = None
    for config in device:
        for candidate in config:
            if candidate.bInterfaceSubClass == 0x42:
                interface = candidate
    if interface is None:
        print("FAIL: no AOA accessory interface (subclass 0x42)")
        return 2
    usb.util.claim_interface(device, interface.bInterfaceNumber)
    try:
        # What the phone has already sent — with two sessions open this is the evidence that the
        # endpoint carries more than the one challenge a host expects.
        drained = bytearray()
        while True:
            try:
                drained += bytes(device.read(IN, 4096, timeout=200))
            except usb.core.USBTimeoutError:
                break
        print("drained from the phone before writing:", len(drained), "bytes")
        offset = 0
        while offset + 24 <= len(drained):
            magic, version, ftype, flags, sequence, length, pts = struct.unpack_from("<iBBhiiq", drained, offset)
            print("  frame type=0x%02x sequence=%d payload=%d magic=%s" % (ftype, sequence, length, hex(magic)))
            offset += 24 + length
        header = struct.pack("<iBBhiiq", MAGIC, VERSION, HOST_HELLO, 0, 1, 0, 0)
        assert len(header) == 24, len(header)
        wrote = device.write(OUT, header, timeout=2000)
        print("wrote one HOST_HELLO frame:", wrote, "bytes")
        after = bytearray()
        deadline = time.time() + 3
        while time.time() < deadline:
            try:
                after += bytes(device.read(IN, 4096, timeout=300))
            except usb.core.USBTimeoutError:
                pass
        print("read after writing:", len(after), "bytes")
        offset = 0
        while offset + 24 <= len(after):
            magic, version, ftype, flags, sequence, length, pts = struct.unpack_from("<iBBhiiq", after, offset)
            print("  frame type=0x%02x sequence=%d payload=%d" % (ftype, sequence, length))
            offset += 24 + length
        return 0
    finally:
        usb.util.release_interface(device, interface.bInterfaceNumber)


if __name__ == "__main__":
    sys.exit(main())
