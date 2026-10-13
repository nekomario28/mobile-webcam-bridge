# Experimental secure Wi-Fi session (TLS/AMB1)

Status: Android and Rust client implementation, physical device acceptance pending.

1. Android 10+ creates a per-installation Android Keystore P-256 TLS server
   certificate. The user reads the full SHA-256 DER fingerprint on the phone.
2. The user manually enters that fingerprint and the independently generated
   128-bit access key (32 hex characters) into the Rust desktop client.
   **Verify the fingerprint from the real phone display**, not from a network
   response or an unverified QR/link.
3. Rust opens TCP port 48527, negotiates *TLS 1.3 only* and validates the
   server certificate bytes against the pinned SHA-256 and validates the
   handshake signature. On any failure it disconnects; no plaintext retry.
4. Under TLS, Rust sends an AMB1 HELLO frame (24-byte header; sequence=1;
   payload length=16) with the decoded random key. Phone performs a
   constant-time comparison before establishing the single-client session.
5. Android sends empty HELLO_ACK with sequence=1 on success. It never sends
   camera frames or accepts IDR control for an unauthorized client.
   All subsequent AMB1 frames and H.264 access units run *inside TLS*.
6. **Revoke Wi-Fi access key** on the phone rotates the random key, closes
   the server and established session and restarts a secure-only listener.

The protocol intentionally does not permit the pre-TLS plaintext AMB1 greeting.
The old C++/Qt host cannot connect until a separately reviewed secure update
is available. USB/AOA framing is unchanged.

**Known gaps:** native device acceptance, separate per-PC credentials and
revocation, desktop credential-store hardening, and anti-phishing UI for the
full out-of-band pin. The handshake wall-clock cap and the concurrent-handshake
bound are implemented (see `LanServer`). Never treat a manually
typed access key as protected if an attacker can view the phone screen or
compromise a desktop with the saved secret.
