# Per-PC authorization

**Implemented on the integration branch; unreviewed and not release-approved.**
Both USB and Wi-Fi require a fresh proof from a saved PC identity. A new identity
waits for explicit approval on the phone, and approval must be stored before the
phone permits capture/control. Approval does **not** start the camera: the phone
user still presses Start camera. USB needs no QR and never treats a physical
plug or accessory descriptor as cryptographic identity.

The [v2 wire contract](../protocol/client-authorization-v2.md) is an intentional
hard cutover of previously unshipped auth scaffolding. Older peers fail closed;
there is no bearer-only or empty-list fallback. Wi-Fi still requires its TLS 1.3
pin and encrypted HELLO access key before the per-PC exchange. Revoking a PC does
not rotate that shared bootstrap key; rotate it separately if it was exposed.

## Authority and identity

An identity is the app's persistent P-256 key in an OS user profile, not proof of
physical PC hardware, process identity or a trustworthy PC name. Same-user
software can generally use that user's files/DPAPI protection. A copied Unix
key can represent the same identity on another machine. An authorized PC can
proxy its own connection; this scheme does not attest its software.

LAN proofs also bind the exact phone certificate pinned by that PC connection,
so a different pinned phone cannot borrow its proof. USB carries an explicit
zero phone binding: it authenticates the PC to the phone, not the phone to the
PC, and does not claim to prevent an active physical relay between devices.

The phone derives the displayed 128-bit ID from the transferred public point.
The approval surface marks the name as PC-proposed and shows the full ID. Match
it with the ID on the PC before allowing an unexpected request. There is no
invented short-code/SAS protocol, and this is not a claim of resistance to every
human-verification error or relay attack.

[PhoneAuthorization](../android/app/src/main/java/dev/nekomario/amb/PhoneAuthorization.kt)
owns one live cross-transport slot, one random challenge per session, and one
bounded pending decision. A valid proof for a known, unexpired key reconnects
without adding/renewing a grant. Unknown keys can request approval; full or
expired entries are rejected. Renewing an expired entry is a separate explicit
phone action, followed by reconnecting the PC.

Revocation removes the grant rather than adding a permanent ban. Its next valid
proof can request approval again. Rejecting, timing out or abandoning a pending
request starts a short global unknown-request cooldown within this Activity owner's
lifetime; it is prompt-flood mitigation, not a persistent block list. No timeout
or reconnect silently approves or renews a key.

## Persistence is part of the grant

[The host store](../host-rs/src/identity/store.rs) loads a user-only private file
on Unix and a user-scope DPAPI-encrypted file under LocalAppData on Windows. It
publishes complete first-use keys atomically without replacing a concurrent
winner. Missing creates a key; corrupt/inaccessible existing data fails without
silently rotating identity. Unix permissions are access control, not encryption.
Failure is surfaced, never repaired behind the user's back: startup fails closed
and the stored file is left exactly as it was. Recovery is explicit —
`mobile-webcam --replace-identity`, and the same action in the GUI (offered only
when it sees that failure, behind a two-step confirmation). A replacement
**renames** the unusable file to `<name>.corrupt-<unix-seconds>` — never deleted,
never overwritten, so its bytes and permissions survive for inspection — and then
publishes a new key. Because the phone stores this PC's public key, every
approval for this PC is invalidated, which is why the command refuses to touch a
key that still reads unless `--force` is given, and why the GUI states it before
asking. The phone-side explicit repair (authorization data that cannot be read)
is still open and remains fail-closed.

[The phone codec/repository](../android/app/src/main/java/dev/nekomario/amb/ClientRepository.kt)
stores public points, labels and approval/expiry times. IDs are derived instead
of persisted redundantly. Decoding validates bounded counts/lengths, point and
ID binding, duplicates, strict UTF-8 and approval periods. Key bytes are copied
on input/output so a UI caller cannot mutate a stored grant through an alias.
[ClientStore](../android/app/src/main/java/dev/nekomario/amb/ClientStore.kt) uses a
separate Android Keystore AES-GCM alias and AEAD context from the Wi-Fi secret.
Invalid ciphertext or a missing decryption key never becomes an empty list.
A surviving namespace key with a missing record also requires recovery rather
than being treated as never-used storage.

The repository publishes memory state only after successful commit and latches
closed after write failure. The live owner then closes access and reports the
error. In particular, a failed revoke stops this connection but **does not prove
that the on-disk grant was removed**. It must not be shown as successful; the
user needs to resolve the storage problem. There is no automatic data reset.

## Phone UI and live enforcement

[The approval surface](../android/app/src/main/java/dev/nekomario/amb/ClientApprovalUi.kt)
creates controls owned by the displayed session, instead of changing the target
read by an old listener. An old queued action cannot approve its replacement;
an ended request is not misreported as a storage failure. Authorized PCs shows expiration and explicit renewal
or revocation with confirmation. All these operations go through the same owner
as the transport gate; the UI does not edit a second copy of the list.

[AuthorizedMediaSession](../android/app/src/main/java/dev/nekomario/amb/AuthorizedMediaSession.kt)
is the common USB/LAN capture and control boundary. It withholds capabilities
until authorized, rejects pre-auth controls, checks permission before accepting
capture/output callbacks, and closes the physical I/O on revocation/expiry. A
separate timer closes stalled proof/pending sessions even if their reader or UI
is idle. UI callbacks are posted, not run synchronously while holding that
owner's lock. Already-completed I/O cannot be recalled.

Expiration uses the phone's system Unix clock. Deadline budgets use a monotonic
clock. Moving wall time before approval invalidates a grant; changing the trusted
phone clock within its approval window can change its effective real duration.
There is no secure-clock or hardware-attestation claim.

The host performs authorization before opening the decoder/output pipeline.
While pending, it displays the local key's ID, polls responsively for Stop and
waits for a separate Authorized result. Repeated Pending does not renew the
human-decision budget. A rejection/failure closes rather than retrying. Every
new physical session also clears the phone's previous camera-start intent.

## Verification boundary

The [active implementation spec](../specs/per-pc-authorization/README.md) records
current checks and remaining acceptance. JVM tests exercise decision/storage
failures, one-use challenges, stale taps, expiry/revocation and real socket
control/output gating. Rust tests exercise actual persistence, TLS proof
exchange, stop/rejection ordering and the existing quality pipeline. Shared wire
and transcript fixtures pin the cross-language bytes; raw/DER signature fixtures
also run against Android's actual EC provider.

Emulator instrumentation includes production-Activity pointer input, persisted
approval/renewal/revocation, live socket closure, EN/JA narrow/scaled captures,
and an independent OpenSSL client against the phone gate. **Adding these tests
is not evidence that their current revision passed.** Inspect exact-head CI and
its semantic markers/artifacts before claiming native UI/runtime verification.
Screenshots are review material, not visual acceptance by their mere existence.

Physical USB-AOA attachment/permission reuse, Camera2 capture, real Linux/Windows
consumer acceptance, sustained performance and independent review of this auth
design remain separate. The earlier static TLS review does not cover this
protocol, identity store, approval UI or later transport changes. Keep the
integration PR Draft; passing tests alone does not authorize merge/release.

## Pairing follow-up

Wi-Fi still exposes its pin/access key through the existing explicit private
screen. A future QR flow may reduce transcription, but opaque QR bytes are still
extractable secrets and must not enter logs, screenshots or analytics. Do not
replace the bootstrap with an ad-hoc short hash or assert that a fresh nonce
alone prevents adaptive human-code grinding. Research and review a replacement
before changing this trust boundary.
