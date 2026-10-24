# Security policy

## Supported versions

The published desktop release **v0.2.0** and Android APK **0.1.4** use
**unauthenticated plaintext Wi-Fi**. USB-only is the recommended way to use
those releases. Do not put them on a network you do not trust.

There is no supported security release of the later TLS and per-PC
authorization work. That code is on `main` and is unreleased. `docs/security.md`
describes the source tree. It is not a statement about the published v0.2.0
binaries. The plaintext-to-TLS break is described in
`docs/migration-tls-wifi.md`.

| Release | Security support |
| --- | --- |
| Desktop v0.2.0 and Android APK 0.1.4 | Published. Wi-Fi is unauthenticated plaintext. Use USB only. |
| Older tags, `main`, and other development builds | No security-support commitment. |

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

- The plaintext Wi-Fi design of published desktop v0.2.0 and Android APK 0.1.4. That behavior is a documented property of those releases. Use USB.
- Vulnerabilities that exist only in a third-party project and are not reachable through this repository's use of it. Report those upstream as well.
- Social engineering, and attacks that start from physical possession of an unlocked device beyond the limits already stated in `docs/security.md`.

## Review boundary

Security-critical code in this repository has had AI-assisted review only.
The write-ups in `docs/reviews/` are those reviews. They are not an
independent human security audit, a penetration test, or a release sign-off.
