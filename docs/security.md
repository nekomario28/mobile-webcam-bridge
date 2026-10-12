# Transport security and resource limits

Mobile Webcam is currently intended for trusted local networks or direct USB.
Wi-Fi AMB1/TCP traffic is **not authenticated or encrypted**. The initial
`HELLO`/`HELLO_ACK` exchange is framing negotiation, **not identity verification**.
A client with network access to the phone's TCP port 48527 can connect to the
stream while the server is enabled. Do not expose this port to the public
Internet, guest Wi-Fi, or untrusted LAN clients.

## Implemented protections

- The Android reader validates control frame payload length **before**
  allocation: HELLO and IDR_REQUEST carry zero bytes; PING carries eight bytes.
- The Android outbound writer caps the control queue at 32 frames (the video
  queue already has an independent two-frame bound). On control overflow, the
  active LAN session reports an error and closes.
- Disconnect notifications identify their originating session, preventing a
  stale close event from discarding a newer connection.
- The Android activity stops camera capture when leaving the foreground.

## Outstanding security requirements

Real protection on untrusted networks requires cryptographic authentication
and encryption, with corresponding changes to Android, the Rust desktop client,
and the supported classic C++ client. Resource limits and lifecycle changes are
not a substitute for confidentiality. Sustained real-device tests remain required.
