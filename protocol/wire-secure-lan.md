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
5. Android sends empty HELLO_ACK with sequence=1 on success. The integration
   branch then requires [per-PC authorization v2](client-authorization-v2.md),
   including binding the signature to this pinned certificate. HELLO_ACK alone
   permits neither camera output nor controls. All subsequent AMB1 frames and
   H.264 access units run *inside TLS*.
6. **Rotate Wi-Fi access key** changes the random bootstrap key, closes the
   server/session and restarts a secure listener. Individual grants remain;
   revoke them separately in **Authorized PCs**.

The protocol intentionally does not permit the pre-TLS plaintext AMB1 greeting.
The old C++/Qt host cannot connect until a separately reviewed secure update
is available. AMB1 frame layout is unchanged, but the integration branch also
requires v2 authorization over USB: older USB clients are not compatible.

**Known gaps:** native device acceptance, independent review of the new per-PC
protocol/store/UI, explicit identity recovery, and safer pairing UX for the
full out-of-band pin. The handshake wall-clock cap and the concurrent-handshake
bound are implemented (see `LanServer`). Never treat a manually
typed access key as protected if an attacker can view the phone screen or
compromise a desktop with the saved secret.
