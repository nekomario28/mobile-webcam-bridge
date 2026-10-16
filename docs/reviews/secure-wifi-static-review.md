# Secure Wi-Fi: one-pass static review

## Evidence boundary

On 2026-10-14, a fresh pi session using `openai/gpt-6.1-sol` reviewed the
secure-Wi-Fi source at `bfd7ddb`. It received selected Android identity,
LAN server, wire and Activity sources, Rust pin verifier, socket transport
and wire sources, and the protocol/security documents. This is a summary
of that single review, not a verbatim transcript. No second model review
was run. It was a static source assessment, not a penetration test,
physical-device acceptance or independent human sign-off.

Desktop credential-storage implementations, connection-selection code,
Android manifest/backup rules, outbound queues and dependency lockfiles
were not supplied. No conclusion about those components follows from the
review. The documentation described their protections, but documentation
was not treated as implementation evidence.

## Finding and resolution

The reviewer identified a client-side availability issue: rustls can perform
multiple reads within `complete_io()` or a TLS stream read. A timeout set
only outside that call does not bound trickled TLS records. It does not
mean certificate verification or access-key authorization can be bypassed.
The reviewer rated it Medium; exploitability and impact were not measured
against a running desktop application.

The initial attempted fix (`66a6d1b`) checked the deadline outside
`complete_io()` and did **not** repair the mechanism. A local loopback peer
with a deliberately incomplete, trickled handshake took about 721 ms to
reject despite a 200 ms connect budget. The regression test failed on that
implementation.

The corrected implementation checks the remaining monotonic deadline at
every raw socket read/write beneath rustls. Connection attempts, TLS and
pairing use the same deadline; a frame's header and payload also share one
budget. Tests in `host-rs/tests/secure_lan.rs` exercise incomplete handshake
and encrypted-record trickling through the public transport. Removing the
read enforcement makes the encrypted-record test fail; renewing the
payload budget makes the header/payload test fail. Restoring enforcement
passes those tests. These tests establish the bounded loopback behavior,
not physical-device or native Windows performance. Synchronous DNS remains
outside the interruptible socket-I/O boundary.

## Other observations

No additional authentication-bypass finding was identified in the supplied
pin verification, handshake-signature verification or TLS-resumption
application-authentication paths. That is not proof of absence.

The access key is reusable until rotation and is not bound to a PC. A holder
who can reach the real phone server is authorized; individual revocation
requires a design change. `FLAG_SECURE` does not prevent clipboard copies or
external-camera capture. First-use trust still depends on comparing the
fingerprint on the actual phone, not a substituted display or network
message. There is no expiring, single-use pairing-approval feature to test.

Screen authenticity, hardware-backed Keystore guarantees, actual TLS
resumption behavior, storage/backup rules and end-to-end Android/Linux/
Windows behavior remain outside this review's evidence. Track the remaining
acceptance work in GitHub issues #4 and #9; neither is closed by this report.
