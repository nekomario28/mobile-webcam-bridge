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

## Implementation slices

1. Wire types and canonical fixtures for the four new frames, both languages,
   with the payload lengths validated before allocation as the existing reader
   does.
2. PC keypair generation and storage, with the existing 0600 / DPAPI rules.
3. Phone-side list, storage, and the authorize/revoke/pending paths.
4. The handshake wiring on both sides, plus rejection reasons reaching the UI.
5. Regressions: unknown client, revoked client, expired client, replayed
   signature, wrong label, too many clients, and revocation of the live session.
6. The migration path from a pairing-key-only installation.

Steps 1 and 2 are independently testable and do not change the current
handshake. Nothing here should ship without the release gate in
[security.md](security.md) being met, and the one-pass independent static review
already spent on the transport does not cover this design.
