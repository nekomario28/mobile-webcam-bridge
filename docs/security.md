# Transport security and resource limits

The published v0.2.0 releases used unauthenticated plaintext Wi-Fi. The
secure-Wi-Fi source branch implements TLS 1.3 with explicit certificate pinning
and a 128-bit access key. USB/AOA remains unchanged. This feature must not be
declared fully validated until end-to-end device/Windows/Linux tests pass.
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

Use **Show Wi-Fi pairing details** on the phone to obtain the fingerprint and
secret. Use **Revoke Wi-Fi access key** to rotate the 128-bit access key and
disconnect an existing session. All desktops using the previous key are
revoked together. Per-PC identity/signature scaffolding exists, but expiry and
per-PC revocation are not enforced by either transport; see
[per-PC authorization](per-pc-authorization.md).
No plaintext fallback is provided on the Rust production Wi-Fi path.
Android 10 (API 29) or newer is required for secure Wi-Fi; USB still works
on earlier supported Android releases.

## Implemented protections

- The Android reader validates control frame payload length **before**
  allocation: secure Wi-Fi HELLO carries the 16-byte credential; USB HELLO
  and IDR_REQUEST carry zero bytes; PING carries eight bytes.
- The Android outbound writer caps the control queue at 32 frames (the video
  queue already has an independent two-frame bound). On control overflow, the
  active LAN session reports an error and closes.
- Disconnect notifications identify their originating session, preventing a
  stale close event from discarding a newer connection.
- The Android activity stops camera capture when leaving the foreground.
- The AMB1 HELLO deadline is absolute, and active LAN sessions reject excessive
  PING rates. TLS negotiation has both a per-operation socket timeout and an
  absolute 5-second whole-handshake deadline that closes the unauthenticated
  socket, with at most four concurrent untrusted handshakes in flight.
- The Rust client propagates one monotonic deadline beneath TLS record
  buffering. Socket connection attempts, TLS negotiation and AMB1 pairing
  share a budget; frame header, payload and flush do not renew it. A timeout
  is terminal, since an AMB1 frame may already be partially consumed.
  Synchronous DNS resolution is not interruptible by this I/O deadline; use
  a literal phone address when that distinction matters.

## Outstanding security requirements

This first implementation still has limitations: a single bearer key is
shared by any PCs that the owner provisions, so one lost key forces global
rotation rather than per-PC revocation. The design that closes this is in
[per-PC authorization](per-pc-authorization.md). Isolated identity, signature and
exchange tests exist; enforcement, persistent identity/list storage and approval
UI are not implemented, and the authorization design is unreviewed.

Desktop settings enforce mode 0600 for Linux files containing a Wi-Fi key
and store the Windows credential as per-user DPAPI ciphertext in the
registry. An earlier plain-text development-build registry value is read for
migration and removed once the protected value is successfully saved.
**Linux mode 0600 is access control, not encryption at rest**; application-
level encryption or desktop Secret Service integration remains open.

There is no
client certificate or authenticated device label. The deprecated classic C++
host cannot connect to the TLS-only Android Wi-Fi listener. A leaked pairing
key alone does not defeat phone-certificate pinning, but it does authorize an
attacker who can reach the real TLS server. Remote network flooding, local
compromise and traffic metadata remain out of scope of TLS guarantees.

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
