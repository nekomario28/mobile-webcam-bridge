# Per-PC authorization and revocation

Design proposal. Not implemented. This closes the gap
[security.md](security.md) already records: *"a single bearer key is shared by any
PCs that the owner provisions, so one lost key forces global rotation rather than
per-PC revocation"*, and *"there is no client certificate or authenticated device
label"*.

## The problem in one sentence

Today the 128-bit pairing key is a **bearer token with no owner**: whoever holds
it connects forever, and the only remedy — rotating the key — revokes every PC at
once and forces re-pairing on each.

## What this must protect against

| Threat | Today | With per-PC authorization |
| --- | --- | --- |
| Pairing key leaks (backup, shoulder-surfing, clipboard) | Anyone with it connects indefinitely | It can only *register* a new PC, and only with an explicit tap on the phone |
| One PC is lost or sold | Rotate the key; every PC re-pairs | Revoke that one entry; the others keep working |
| An authorization outlives its purpose | Never expires | Expires; re-approval is required |
| "Which PCs have access?" | Not answerable | Listed on the phone, with approve and revoke |

## What it does not protect against

Stated plainly because a design that hides its limits gets trusted too far.

- **A compromised PC.** It holds its own key; a local attacker uses it. This is
  per-PC authorization, not endpoint attestation.
- **The user approving the wrong PC.** The approval decision is the human in the
  loop, and the human can be fooled by a hostile label.
- **An attacker watching the phone screen at pairing time.** They see the
  fingerprint and can approve their own client. Physical access to the phone at
  pairing time is out of scope, exactly as it is today.
- **Local compromise of the phone or the PC, traffic metadata, and network
  flooding.** Unchanged from the current guarantees.
- **Linux key storage.** A PC key in a 0600 file is access control, not
  encryption at rest — the same limitation the current pairing key has. Secret
  Service integration remains open.

## Chosen design

Application-layer client authentication inside the existing TLS session, not a
TLS client certificate.

The phone has to store a per-PC public key either way, so the storage cost is
identical. What differs is where the proof happens. Inside the encrypted channel
it is an ordinary AMB1 frame: it versions cleanly, an old PC simply ignores an
unknown type as it already does, the wire stays inspectable, and the failure
modes are the ones the existing reader already validates. A TLS client
certificate would also change the handshake itself, which is the part most worth
keeping boring.

### Pairing, which is the part worth redesigning

Today the user reads a 64-character certificate fingerprint and a 32-character
access key off the phone screen and types both into the desktop. That is the
worst part of the setup, and it is the trust anchor, so it cannot simply be
dropped — but how it is entered has room to improve, and two established
patterns apply to exactly this two-device situation.

**Numeric comparison, as Bluetooth Secure Simple Pairing does it.** Both sides
display the same short code and the user compares them, then approves on the
phone. No typing at all, no camera on either side, and the tap *is* the pairing
action — so the approval this design needs anyway is not an extra step.

```text
phone: a PC wants to connect · 418273 · [Approve] [Reject]
PC:    compare with your phone: 418273        (nothing to type)
```

The code is derived from the handshake, so a machine-in-the-middle cannot make
the two sides agree:

```
SHA256(serverNonce || clientPublicKey || certificateFingerprint)[0..4] mod 10^6
```

The nonce is the phone's own `CLIENT_CHALLENGE`, so it is fresh per attempt and
neither predictable nor choosable by the client.

**QR on the PC, scanned by the phone, as WhatsApp Web does it.** The desktop
shows a one-time token; the phone scans it and pushes its identity bound to that
token. Zero typing, one scan, and the anchor is physical — the user pointed
their own phone at their own screen. The costs are a camera permission, a QR
decoder on the phone, and the scan UI.

**QR on the phone, read by the PC's camera.** The phone displays everything the
PC needs — address, certificate fingerprint, and a one-time token — so a single
scan finishes setup. The cost is a camera capture stack on the desktop, which
this project does not have: it decodes PNGs and nothing else. Linux could read
V4L2 through `libc`, but Windows needs Media Foundation, which is a new
dependency in a project whose standing rule is to avoid them.

**Keeping the hexadecimal entry.** No change, worst experience, least work.

#### What may go in a QR, which is a leak question

The payload decides the damage, so it is worth separating.

| Content | If it leaks |
| --- | --- |
| Address | Not a secret |
| Certificate fingerprint | **Safe.** A pin, not a secret: without the private key it cannot be used to impersonate the phone. The existing docs already say fingerprints are pins, not secrets. |
| The long-lived access key | **Must never be in a QR.** It is a bearer token; whoever reads it connects indefinitely. |
| A one-time token with an expiry | **This is what belongs there.** Sixty seconds, single use, one registration. |

So a QR carries a *short-lived bootstrap credential*, not a long-lived secret.
That is the whole trick behind "must not leak": the thing on screen is not the
thing that grants standing access.

```text
Opaque binary, decoded only by this app — never a URI scheme, because a generic
QR reader would display the token as text and may log it.

  u8  version = 1
  u8  addressKind                    4 or 6
  16  address
  u16 port
  32  certificate SHA-256, raw
  16  one-time token, raw
  u32 expiresAtUnixSeconds
  u8  labelLength, then the label    32 bytes maximum
```

Roughly 75 bytes, a QR version 4 or 5, readable on a phone screen.

Leak paths, and what bounds them:

| Path | Consequence | Bound |
| --- | --- | --- |
| Screenshot or screen recording of the phone | Token captured | `FLAG_SECURE` is already set while the pairing display is open, and cleared in `onPause` |
| Another phone photographing the screen | Token captured | Sixty seconds, single use, one registration — and the legitimate PC normally uses it first |
| A QR appearing in a bug report | Token captured | Recording is blocked, and the token has expired |
| Someone reading over the user's shoulder | Token captured | The same window |

The residual risk is stated rather than hidden: **within that window a third
party's PC can connect with the captured token, and the user cannot tell which
PC read it.** That is why the QR transports the pin and the tap grants the
camera — a QR connection still requires approval before video.

#### The trade-off, stated plainly

| | 64-digit fingerprint | 6-8 digit code |
| --- | --- | --- |
| Theoretical strength | 2^256 | 10^6 to 10^8 |
| Effective strength | "did the user read 64 hex digits correctly" — in practice nobody does | "did the user actually compare them" — short enough that they do |

A readable code the user genuinely compares is stronger in practice than a
fingerprint nobody checks. Signal's 60-digit safety number, which is almost
never verified, is the counter-example that makes this argument.

Two things must come with it, because they are what the pairing key provides
today:

- **At most one pending request, and a cooldown after a rejection.** Without
  them, anything on the LAN can spam approval prompts.
- **The full fingerprint stays available on demand**, one click away in the
  details, for a user who wants the complete check.

#### Consequence: the pairing key is no longer required

TLS pinning, the code comparison and the phone's approval together are the
anchor; the 128-bit bearer key has no remaining job. The user is the weak link
in both models. The difference is that the new model asks them to do something
comprehensible instead of handling an opaque secret — and it removes 96
characters of typing from every setup.

A user who is not physically at the phone cannot pair, which is also true today
and is not a regression.

### Identity

On first run the PC generates a P-256 keypair and stores it with the same
protection the pairing key already has: DPAPI-protected registry value on
Windows, a 0600 file on Linux. The client ID is the first 16 bytes of the
SHA-256 of the public key — short enough to display, long enough to collide
nowhere near realistically.

### The exchange

```text
PC                                phone
 | --- HELLO (+ pairing key) --->  |   unchanged bootstrap credential
 | <-- HELLO_ACK (empty) --------- |   still requires sequence 1, empty payload
 | <-- CLIENT_CHALLENGE ---------- |   u8[32] server nonce
 | --- CLIENT_AUTH --------------> |   u8 version, u8[16] clientId,
 |                                 |   u16 labelLen, label, u8[64] signature
 | <-- CLIENT_AUTH_RESULT -------  |   u8 status, u8 reason,
 |                                 |   u32 expiresAtUnixSeconds
```

The signature covers `clientId || nonce || label`, so a registration cannot be
replayed into another session, moved to another PC, or given a label the client
did not sign.

`CLIENT_AUTH_RESULT` statuses: `0` authorized, `1` pending approval, `2`
rejected. Reasons: `0` none, `1` unknown client, `2` revoked, `3` expired,
`4` too many clients, `5` bad signature.

The label is a convenience string the PC proposes — a hostname, say. **It is
attacker-chosen and must be displayed as untrusted**, never used to identify a
client in a security decision. The client ID is the identity.

### Authorization rules

1. **No camera session before an authorized client.** A pending or rejected
   client gets no video. This is the point of the feature, and it costs the user
   one tap on the first connection after upgrading.
2. **The pairing key registers; it does not authorize.** Once any client is
   registered, a HELLO carrying only the pairing key is answered with
   `pending approval` rather than a session. On a fresh install with no clients
   registered, the pairing key alone still reaches the approval prompt — so a
   single-PC user pairs exactly as today, plus one tap.
3. **Expiry.** Each registration records `approvedAt` and `expiresAt`.
   Recommended default **90 days**, long enough that a desktop user is not
   nagged and short enough that a forgotten PC stops working. After expiry the
   PC shows *"authorization expired"* and the pairing key plus a phone tap
   restore it. The PC displays the expiry date so the deadline is not a surprise.
4. **Revocation** removes the entry and, if it is the live session, ends that
   session immediately — a revoked PC must not keep streaming.
5. **At most 8 clients**, bounding both the phone's storage and the user's
   review burden.

### Phone storage and UI

The list lives beside the existing `encrypted_secret_v1`, Keystore-encrypted,
as `authorized_clients_v1`: a JSON array of
`{clientId, label, approvedAt, expiresAt}`.

The phone shows the list with an approve action for a pending client and a
revoke action per entry. Both are explicit user actions; nothing auto-approves,
ever. This is the same discipline as the existing *Show pairing details* /
*Revoke access key* pair.

### PC-side storage and UI

The keypair persists next to the pairing key. The desktop shows the
authorization state — authorized until a date, pending, expired, revoked — in
the same place it already reports a wrong key, so a failed authorization is
never silent.

## Migration

Existing single-PC installations keep their pairing key. On the first connection
after the phone side understands this extension, the client is `pending
approval`; the user taps once and the PC is registered. No re-pairing, no key
rotation, no lost stream beyond that one tap.

The extension is versioned through the existing `extensionVersion` in the
capability advertisement. An old PC sending only a pairing key is answered
`pending approval` on a phone that has clients registered, and gets a session on
a phone that has none — which is exactly today's behaviour.

## Explicit non-goals

- Endpoint attestation or hardware-bound PC keys. The PC key is software
  protected; a compromised PC is out of scope.
- Secret Service integration on Linux, and any change to the 0600 file
  protection the pairing key already uses.
- Replacing or weakening TLS 1.3 or the certificate pin. This sits inside the
  channel TLS already authenticates.
- Automatic re-approval, grace periods, or "remember this PC" shortcuts.
- Multi-user phones, shared PCs, or per-session approval.

## The USB path needs the authorization, not the pairing

USB is the better transport for this: the physical plug *is* the bootstrap, so
none of the pairing machinery applies — no fingerprint to compare, no key to
type, no QR, no short code. What it still needs is the authorization list, and
today it has neither that nor a way to tell one PC from another.

Four findings, from reading the accessory path.

### H1 — the start consent survives a different accessory

```kotlin
openAccessory() {
    closeSession(clearStartRequest = false)
    ...
    if (cameraStartRequested) startCamera()
}
```

Once the user presses *Start camera*, that flag is **carried across accessory
changes**. The camera does not auto-start on a fresh install, which is the part
that matters most and is correct. But combined with H2 it means a second PC can
inherit the first PC's consent.

### H2 — the displayed identity is a string the PC chooses

```kotlin
"USB connected · ${accessory.manufacturer} ${accessory.model}"
```

The AOA accessory descriptor's manufacturer and model are set by the PC
application. A hostile PC can call itself anything, and Android's accessory
permission is keyed on that descriptor, so **presenting the same strings reuses
an already-granted permission**. H1 plus H2 is the real hole: press start once,
unplug, plug into a PC claiming the same accessory identity, and the camera
starts by itself.

### H3 — no per-PC authorization over USB

The LAN path has no per-PC identity either, but it at least has a key. USB has
nothing: the connected PC is not identified and cannot be revoked individually.

### H4 — host commands are unauthenticated

`SessionControl` validates every Prepare and Commit, but nothing identifies the
sending PC. A hostile PC can request quality switches — annoying, recoverable —
and change the display transform, which the user sees. Low severity, and it is
still an unauthenticated control channel.

### The fix

| Finding | Fix |
| --- | --- |
| H1 | Stop carrying `cameraStartRequested` across accessories. Require a press per newly opened accessory, or only carry it when the accessory identity is byte-identical to the one that just closed. |
| H2 | Display the strings as *claimed*, or use the accessory serial as a stable identifier. Never as a trust anchor. |
| H3 | Apply the same authorization list as the LAN path. The plug is the bootstrap; the first connection needs one tap on the phone; after that the PC authenticates itself and revocation comes from the list. |
| H4 | Accept host commands only from an authorized PC. An unauthorized one gets no video. |

H3 is also the correct fix for H2: stop trusting the string, and let the phone
hold the list, so the identity is one the phone issued rather than one the PC
announced.

The pairing design above exists because the LAN bootstrap happens across a
screen. USB's bootstrap is physical, so the authorization model is identical and
only the bootstrap differs.

## Open decisions

These are recommendations, not settled. Each one changes the user-visible cost,
so they are worth a decision before implementation.

| Decision | Recommendation | Alternative |
| --- | --- | --- |
| Expiry | 90 days | Per session (annoying), or never (defeats the point) |
| Client cap | 8 | 4, or unbounded with a scrolling list |
| Fresh install with no clients | Pairing key reaches the approval prompt | Pairing key alone grants a session (weaker, no extra tap) |
| Pairing key after registration | Registration only | Keep it as a full fallback (weaker) |
| Where the proof lives | Application frame inside TLS | TLS client certificate |
| Pairing flow | Numeric comparison + phone approval, no typing | QR in either direction, or keep the hex entry |
| QR direction, if QR is chosen | PC shows a token, phone scans it | Phone shows address + pin + one-time token, PC's camera reads it |
| One-time token lifetime | 60 seconds | Longer is more convenient and leaks further |

## Implementation slices

1. Wire types and canonical fixtures for the new frames — the four client-auth
   frames plus the pairing code shown to both sides — in both languages,
   with the payload lengths validated before allocation as the existing reader
   does.
2. PC keypair generation and storage, with the existing 0600 / DPAPI rules.
3. Phone-side list, storage, and the authorize/revoke/pending paths.
4. The handshake wiring on both sides, plus rejection reasons reaching the UI.
5. Regressions: unknown client, revoked client, expired client, replayed
   signature, wrong label, too many clients, and revocation of the live session.
6. The migration path from a pairing-key-only installation.
7. The USB path: stop carrying the start consent across accessories, and apply
   the same authorization list with the physical plug as its bootstrap.

Steps 1 and 2 are independently testable and do not change the current
handshake. Nothing here should ship without the release gate in
[security.md](security.md) being met, and the one-pass independent static review
already spent on the transport does not cover this design.
