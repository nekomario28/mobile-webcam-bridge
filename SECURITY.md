# Security policy

## Supported versions

The current published line is **v0.3.x** (latest **v0.3.2**). Those releases
use TLS 1.3 with a pinned phone certificate and per-PC authorization on USB
and Wi-Fi. Independent review of the pairing and authorization protocol
remains open; see `docs/security.md` and issues #4 and #9. That write-up is
not a security-support commitment or a release sign-off.

Historical tags through **v0.2.0**, including Android APK **0.1.4** and
classic Qt **v0.1.2**, used unauthenticated plaintext Wi-Fi. Do not put those
builds on a network you do not trust. USB-only is the recommended way to use
them, if at all. The change from those builds to TLS is described in
`docs/migration-tls-wifi.md`.

| Release | Security support |
| --- | --- |
| v0.3.x (latest v0.3.2) | Current published line. See `docs/security.md` for the review boundary. |
| v0.2.0 and earlier, including APK 0.1.4 and classic Qt v0.1.2 | Historical. Wi-Fi is unauthenticated plaintext. Use USB only, if at all. |
| Other tags, `main`, and development builds | No security-support commitment. |

## Reporting a vulnerability

Report privately with GitHub private vulnerability reporting. That opens a
draft security advisory rather than a public issue:

<https://github.com/nekomario28/mobile-webcam-bridge/security/advisories/new>

The form accepts reports only when private vulnerability reporting is enabled
for this repository (Settings → Advanced Security → Private vulnerability
reporting).

Include the release or commit, whether the session was USB or Wi-Fi, and
enough detail to reproduce the issue. Do not open a public GitHub issue,
pull request, or discussion for an undisclosed vulnerability. This repository
does not publish a separate security email address.

## Scope

In scope:

- The Android app under `android/`
- The Rust host under `host-rs/` and the classic host under `host/`
- The wire protocol under `protocol/`
- USB and Wi-Fi session handling, pairing, authorization, and credential storage
- Release signing and how the release workflow handles the signing key

Out of scope:

- The plaintext Wi-Fi design of historical desktop v0.2.0 and Android APK 0.1.4. That behavior is a documented property of those releases. Use USB.
- Vulnerabilities that exist only in a third-party project and are not reachable through this repository's use of it. Report those upstream as well.
- Social engineering, and attacks that start from physical possession of an unlocked device beyond the limits already stated in `docs/security.md`.

## Review boundary

Security-critical code in this repository has had AI-assisted review only.
The write-ups in `docs/reviews/` are those reviews. They are not an
independent human security audit, a penetration test, or a release sign-off.
