# Implementation choices

These are implementing-agent choices, not independent security approval.

## One identity and persistence boundary

- **Per-user application identity, not hardware attestation.** The PC owns a P-256
  private key. The phone derives an ID from its public point. A hostname, USB
  descriptor or full ID alone is never proof of possession.
- **No-replace publication.** Concurrent first launches must read one published
  key rather than overwrite each other. Existing corrupt/inaccessible identities
  fail without rotation. Windows reuses user-scope DPAPI in a file, not a second
  registry/FFI owner; native storage CI exercised this at the storage checkpoint.
- **Bounded binary phone records.** These are internal data, not an editable
  config or external API. Store the point/label/times and derive the ID; avoid
  redundant identity fields and an extra serialization dependency.
- **Separate Keystore alias/AAD.** Phone grants do not share the Wi-Fi bearer
  secret's storage/migration lifecycle. Ciphertext without its key fails without
  replacement; a surviving namespace key without its record is not pristine
  first use. The latter native case needs the new conformance run.
- **Publish only after write success.** A failed preferences commit can change
  Android's memory cache. Latch the repository/owner closed, stop active access
  and report failure. Do not claim unsuccessful revocation reached disk.
- **Immutable, single-line labels.** Copy key bytes on input/output; enforce
  strict UTF-8 byte limits without truncation. Check Unicode scalars for controls,
  format characters and line/paragraph separators. UTF-16 half checks missed
  supplementary tags; the new case failed before repair. Rust consumes the
  already-locked Unicode category crate rather than a hand-copied category table.

## One mandatory session owner

- **Unshipped v1 is removed, not adapted.** Both peers upgrade together. No
  timeout detection, empty-list grant, old-phone bypass or bearer-only fallback.
- **The proof names its actual phone context.** The domain-separated v2 message
  includes transport, phone binding, one-use nonce, point and label. LAN binding
  is the pinned TLS certificate's DER SHA-256 and is checked before signing.
  USB explicitly uses zero: there is no cryptographic phone attestation or
  active-physical-relay guarantee. Keep the existing full pin/bootstrap key;
  do not invent human short-code security.
- **One cross-transport slot and one proof.** The phone owns nonce, proof budget,
  pending lifetime and decision. No controls/capabilities/media precede a grant.
  Both transport implementations consume `AuthorizedMediaSession`; the Activity
  cannot bypass its capture/output checks.
- **Revocation removes, not bans.** A later valid proof can request approval
  again. Declined, expired or abandoned prompts incur an owner-lifetime cooldown;
  disconnecting must not evade it. Accepted grants do not require repeated
  onboarding. This is prompt mitigation, not a persistent block list or DoS proof.
- **Independent close/Stop.** The phone timer can close idle/revoked/expired
  sessions without reader/UI progress. Host proof polling remains stoppable;
  Pending never opens decoder/output. A real subprocess test also stops while
  its TLS peer deliberately withholds the grant.
- **No implicit camera consent.** Every new physical session clears old Start
  intent. Approval permits a later phone-owned Start; PC quality changes can
  stage while stopped, not start Camera2 by themselves.

## Approval views and evidence

- **Controls are owned by the displayed session.** Replace pending action views
  when the session changes; never mutate the target read by an old listener.
  Ending an old request is not a storage error and cannot grant or stop its
  replacement. Native conformance replays an old view's queued click and checks
  storage before explicitly approving the new peer with pointer input.
- **Callbacks post, not synchronously re-enter.** Owner callbacks enqueue UI
  work/frames or close I/O. They must not join a reader waiting for the same
  owner lock. The Activity fences callbacks by current session identity.
- **Primary UI actions use real pointer input.** The disposable API 35 harness
  uses production Activity, TLS and encrypted storage; it exercises both
  languages, exact IDs, cooldown, known reconnect without renewal, explicit
  renewal and live revoke. Source/APK receipts and captured geometry/locale/font
  metadata tie the resulting artifacts to their run.
- **Evidence is not interchangeable.** Local stub compilation does not link an
  APK; emulator interaction is not physical USB/Camera2/native-consumer use;
  implementer screenshot inspection is not independent visual acceptance.
  Keep PR #11 Draft and the independent authorization review outstanding.

Explicit recovery UX and safer pairing remain follow-up work. Failures preserve
state and stop access; they never silently choose replacement identities.
