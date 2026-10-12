# Transport security and resource limits

The published v0.2.0 releases used unauthenticated plaintext Wi-Fi. The
secure-Wi-Fi source branch implements TLS 1.3 with explicit certificate pinning
and a 128-bit access key. USB/AOA remains unchanged. This feature must not be
declared fully validated until end-to-end device/Windows/Linux tests pass.

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
revoked together; per-PC credentials and revocation remain future work.
No plaintext fallback is provided on the Rust production Wi-Fi path.
Android 10 (API 29) or newer is required for secure Wi-Fi; USB still works
on earlier supported Android releases.

## Implemented protections

- The Android reader validates control frame payload length **before**
  allocation: HELLO and IDR_REQUEST carry zero bytes; PING carries eight bytes.
- The Android outbound writer caps the control queue at 32 frames (the video
  queue already has an independent two-frame bound). On control overflow, the
  active LAN session reports an error and closes.
- Disconnect notifications identify their originating session, preventing a
  stale close event from discarding a newer connection.
- The Android activity stops camera capture when leaving the foreground.
- The AMB1 HELLO deadline is absolute, and active LAN sessions reject excessive
  PING rates. TLS negotiation also has a socket timeout. An absolute whole-TLS
  handshake deadline remains an explicit denial-of-service hardening task.

## Outstanding security requirements

This first implementation still has limitations: a single bearer key is
shared by any PCs that the owner provisions, so one lost key forces global
rotation rather than per-PC revocation. Desktop settings store it in a
user-private config file on Linux or per-user registry on Windows; a dedicated
Windows DPAPI-protected credential store is not yet implemented. There is no
client certificate or authenticated device label. The deprecated classic C++
host cannot connect to the TLS-only Android Wi-Fi listener. A leaked pairing
key alone does not defeat phone-certificate pinning, but it does authorize an
attacker who can reach the real TLS server. Remote network flooding, local
compromise and traffic metadata remain out of scope of TLS guarantees.

**Release gate:** verify AndroidKeyStore TLS signing on physical Android 10+
devices, Linux and Windows end-to-end consumer frames, incorrect pin, incorrect
key, MITM substitution, revocation during streaming, stalled TLS handshake,
and USB regression. No security claim is established by build-only CI.
