# Transport security and resource limits

The published v0.2.0 releases used unauthenticated plaintext Wi-Fi. The
secure-Wi-Fi source branch implements TLS 1.3 with explicit certificate pinning
and a 128-bit access key. The integration branch additionally requires per-PC
authorization on both USB and Wi-Fi. It is unreviewed and not release-approved;
end-to-end device/Windows/Linux acceptance still has separate gates.
The two transports are not interoperable; see
[migrating to secure Wi-Fi](migration-tls-wifi.md) for the upgrade path.

The phone generates a self-signed P-256 TLS certificate in Android Keystore.
The user directly compares the SHA-256 DER certificate fingerprint shown on
the phone with the desktop entry (out-of-band verification); the desktop
never learns or trusts a new phone key automatically from the network.
After TLS 1.3 successfully verifies this pinned certificate and validates
the handshake signature, the desktop sends an independent random 16-byte
shared access key inside the TLS-encrypted AMB1 HELLO. The server rejects
wrong keys before creating a camera session or returning HELLO_ACK.

Use **Show pairing key** on the phone to obtain the fingerprint and secret.
**Rotate Wi-Fi access key** changes that shared bootstrap credential and
disconnects Wi-Fi clients; it does not remove per-PC grants. **Authorized PCs**
provides individual approval expiry, renewal and revocation. Both transports
require a fresh signed proof; see [per-PC authorization](per-pc-authorization.md)
and its explicit verification/review boundary.
No plaintext fallback is provided on the Rust production Wi-Fi path.
Android 10 (API 29) or newer is required for secure Wi-Fi; USB still works
on earlier supported Android releases.

## Implemented protections

- The Android reader validates control frame payload length **before**
  allocation, including bounded public-key proofs. USB has no credential HELLO;
  its per-PC gate precedes any camera/quality control. The exact layout and
  direction/order rules live in [authorization v2](../protocol/client-authorization-v2.md).
- The Android outbound writer caps the control queue at 32 frames (the video
  queue already has an independent two-frame bound). On control overflow, the
  active LAN session reports an error and closes.
- Disconnect notifications identify their originating session, preventing a
  stale close event from discarding a newer connection.
- The Android activity stops camera capture when leaving the foreground.
- The Android Wi-Fi listener only serves connections that arrive on a local
  network address of the phone (RFC 1918 / link-local IPv4, IPv6 link-local or
  unique-local, loopback for `adb forward`) on a non-mobile-data interface;
  others are closed before TLS. The pairing addresses sent to the PC use the
  same rule.
- The AMB1 HELLO deadline is absolute, and active LAN sessions reject excessive
  PING rates. TLS negotiation has both a per-operation socket timeout and an
  absolute 5-second whole-handshake deadline that closes the unauthenticated
  socket, with at most six concurrent untrusted handshakes in flight and at
  most two from any one remote address.
- The Rust client propagates one monotonic deadline beneath TLS record
  buffering. Socket connection attempts, TLS negotiation and AMB1 pairing
  share a budget; frame header, payload and flush do not renew it. A timeout
  is terminal, since an AMB1 frame may already be partially consumed.
  Synchronous DNS resolution is not interruptible by this I/O deadline; use
  a literal phone address when that distinction matters.

## Outstanding security requirements

The per-PC protocol and its new persistence/UI/gate have not received an
independent security review. Test coverage and emulator interaction do not
replace that review or physical-device/native-consumer acceptance. The shared
Wi-Fi bootstrap key still needs rotation if disclosed even when individual PC
grants are revoked. Explicit identity recovery UX and a reviewed, safer pairing
bootstrap remain follow-up work, not implicit guarantees of these stores.

Desktop settings enforce mode 0600 for Linux files containing a Wi-Fi key
and store the Windows credential as per-user DPAPI ciphertext in the
registry. An earlier plain-text development-build registry value is read for
migration and removed once the protected value is successfully saved.

On Linux, when a desktop Secret Service (GNOME Keyring, KWallet, KeePassXC, ...)
answers on the session bus, the PC identity file and the settings file's
`lanKey` are sealed with ChaCha20-Poly1305 under a random 256-bit storage key
kept in that keyring (item label "Mobile Webcam storage key"); the files hold
`mwss1:` ciphertext and keep mode 0600. Plain values written by earlier
versions are sealed on the next load. Without a Secret Service (headless
sessions, no D-Bus, a build without the default `keyring` feature, or
`MOBILE_WEBCAM_KEYRING=off`) the files keep the previous plain 0600 format,
and **mode 0600 there is access control, not encryption at rest**. A sealed
value never falls back: if the keyring is locked, missing or no longer holds
the storage key, loading fails closed and the file is preserved. Deleting the
keyring item is equivalent to losing the PC key: use explicit recovery and
re-pair. Downgrading to a release older than this change cannot read sealed
files; re-pair after a downgrade. Same-user processes that can query the
unlocked keyring are not separated from this app.

There is no TLS client certificate or authenticated hardware/process label.
PC identity is a per-user application key, not hardware attestation. LAN proofs
bind the pinned phone certificate; USB has no cryptographic phone attestation
and does not claim to defeat an active physical relay. The deprecated classic
C++ host does not implement v2 authorization, including on USB: upgrade Android
and the Rust host together. A leaked Wi-Fi pairing key can reach the proof gate
but alone cannot satisfy it. Remote network flooding, local compromise and
traffic metadata remain outside TLS guarantees.

**Release gate:** verify AndroidKeyStore TLS signing on physical Android 10+
devices, Linux and Windows end-to-end consumer frames, incorrect pin, incorrect
key, MITM substitution, revocation during streaming, stalled TLS handshake,
and USB regression. No security claim is established by build-only CI.
The [one-pass independent static review](reviews/secure-wifi-static-review.md)
records its narrower coverage and the reproduced client-deadline finding.


### External Rust security controls

The Rust Wi-Fi client uses **rustls** for the TLS 1.3 record layer and handshake.
GitHub Actions `rust-dependency-security` audits the committed
`host-rs/Cargo.lock` using a pinned release of **RustSec cargo-audit** on
lockfile changes and weekly. This detects published third-party crate
advisories; it does not prove the pairing protocol or native FFmpeg/Android
runtime is safe. Treat findings as actionable; don't add blanket ignores.

The Android Keystore TLS signing identity uses alias v2 and authorizes
`DIGEST_NONE` alongside `DIGEST_SHA256`: Android's TLS implementation
signs a pre-digested transcript. Clients paired with the earlier development
v1 certificate must compare and re-pin the new certificate fingerprint.

## Linux host integration setup

The Rust host runs `pkexec --disable-internal-agent /bin/sh -s --` and writes
the setup script on that shell's standard input. The script text and the udev
and modprobe files it installs are compiled into the binary. `pkexec` does not
execute `install-host-integration.sh` from a path next to the executable.

The Linux release artifact is an AppImage. Packaging has no root install step
that could place the script at a fixed `/usr/libexec` path, so that layout is
not what the host executes. A polkit action allowing `/bin/sh` would authorize
a shell, and polkit cannot pin standard input, so no `org.*.policy` file is
shipped.

Arguments after `--` are a packaged USB helper path (absolute, final component
`mobile-webcam-usb`, a regular file, not a symbolic link), `--switch-parent`
with a non-zero numeric PID, and `--device-json` built only from that device's
numeric bus, address, vendor, product and port fields. The script still refuses
to replace a modified modprobe or modules-load file. When a USB switch is
required, the helper is installed to `/usr/lib/mobile-webcam/mobile-webcam-usb`
and executed only at that path, with the device JSON on its standard input.
Manual `sudo` and the classic Qt host still use the on-disk script.
