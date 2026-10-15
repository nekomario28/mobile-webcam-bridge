# Migrating Wi-Fi from plaintext to TLS 1.3

Scope: the transition from published releases that used unauthenticated
plaintext Wi-Fi (Android APK 0.1.4 and desktop 0.2.0) to the secure Wi-Fi
implementation. USB/AOA framing is unchanged by this migration. The
protocol detail lives in [wire-secure-lan.md](../protocol/wire-secure-lan.md);
the security posture and its open gates live in [security.md](security.md).

## What changes on the wire

Wi-Fi connections negotiate TLS 1.3 against a per-installation phone
certificate pinned by its SHA-256 fingerprint, then authenticate with a
128-bit access key inside the encrypted channel. The AMB1 version byte
stays 1: plaintext and TLS sessions are distinguished by the HELLO
payload (empty on plaintext, 16 credential bytes under TLS), not by a new
wire version. There is no plaintext fallback on the Rust production
Wi-Fi path.

## Compatibility

| Android app | Desktop host | Wi-Fi | USB |
| --- | --- | --- | --- |
| 0.1.4 and earlier (plaintext listener) | published 0.2.0 Rust host (plaintext) | works, unauthenticated | works |
| 0.1.4 and earlier | secure Rust host | **no** — the host requires TLS | works |
| secure build | any plaintext host (0.2.0 Rust, classic C++/Qt) | **no** — the phone is TLS-only | works |
| secure build | secure Rust host | works, pairing required | works |

The classic C++/Qt host cannot connect to the TLS-only Android listener
until a separately reviewed secure update exists.

## Upgrade order

There is no interop window between the plaintext and TLS Wi-Fi protocols:

1. Update the Android app and the desktop host together.
2. If one side ships first, keep the other on its last working version
   and use USB until both are updated.
3. The first Wi-Fi connection after both sides are updated requires
   pairing (below).

## First connection after upgrade

The desktop remembers the phone address, never credentials from a
plaintext session: a fresh fingerprint and access key entry is always
required. Read both from the phone's **Show pairing key** screen, verify
the fingerprint directly on the phone display, and enter them on the
desktop. A rejected pairing (wrong fingerprint, rejected key) is named
specifically in the desktop UI with the matching next step.

## Credential rotation

**Revoke all paired PCs** on the phone rotates the access key and
disconnects every desktop; all desktops must re-pair. Per-PC revocation
is future work, so one lost key means global rotation.

## Rollback

Downgrading either side alone breaks Wi-Fi. To roll back, downgrade both
the Android app and the desktop host to the same plaintext generation.
USB keeps working in every combination.

## Wire protocol versioning rules

- The AMB1 version byte (1) covers the frame format only. Bump it for an
  incompatible frame change, never per transport.
- A frame kind a receiver does not implement is an error, not a silent
  skip: both sides reject unexpected kinds before any session exists.
- A handshake that needs its own authentication (the TLS credential
  exchange) lives inside the transport, not in the version byte.

## Release evidence for this migration

- CI on the feature branch: Android instrumentation, the emulator TLS
  gate (wrong-key rejection and authorized pairing against the real
  LanServer), Linux/Windows Rust CI including the TLS positive/negative
  tests, and the dependency security audit.
- Not established by build-only CI: physical Android 10+ device
  acceptance, native sustained streaming, and the MITM/phishing review.
  No migration step should be declared complete on those gates alone.
