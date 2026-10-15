# Per-PC authorization implementation

Requested scope: real identity/list persistence, phone approval/revocation UI,
and mandatory camera/control authorization over both USB and pinned TLS Wi-Fi.
All three are implemented in the integration worktree. Local mechanical gates
pass; the new production-Activity/emulator gate still needs runtime evidence.
No merge or release is authorized by tests alone. The earlier independent static
transport review does not cover this protocol, stores or UI. Do not launch another
independent model review under the exhausted one-review authorization.

## Preserved contracts

- Phone owns approval and first camera start; PC owns quality selection.
- No auto-approval or bearer-key fallback, including an empty phone list.
- USB needs no QR. Descriptors are claimed metadata, not identity or camera
  consent; USB has no cryptographic phone attestation or active-relay guarantee.
- Retain pinned TLS and its access key for Wi-Fi bootstrap. LAN proofs also bind
  that pinned phone certificate. Do not invent short-code security or weaken pins.
- One approval owner across both transports. A bounded pending request belongs
  to its exact live session; stale UI actions cannot approve a replacement.
- Persist before granting. Corrupt/read/write-failed storage must not silently
  rotate identity or publish an empty successful list. Failures stop access.
- Preserve quality deadlines, responsive Stop and output-based Applied. A grant
  never starts the camera; no automatic proof retry, renewal or recovery.

## Implemented slices and their gates

### 1. Persistence — platform checkpoint verified

The host stores a stable per-user P-256 identity beside settings, with Unix
private-file checks and atomic no-replace publication. Windows reuses the
existing user-scope DPAPI owner. Concurrent first-use processes return the same
published key. Phone records are bounded, immutable and validated, protected by
separate Keystore AES-GCM storage. Failed writes latch the repository closed;
failed revocation is not represented as durable success.

At `99a8e8a5a9c885143c7aa5b5546e73de99573448`, all six workflows succeeded.
Rust run `38764092374` executed native Windows identity reload, concurrency,
corruption and invalid-destination tests. Emulator run `38764092451` reported the
semantic storage/TLS/signature PASS marker. Those results belong to that storage
checkpoint, not to the later integrated gate/UI revision.

The newer adapter also refuses a lost record when its Keystore alias survives;
that native conformance case requires the new emulator run, not inference from
previous storage CI.

### 2. Approval and revocation — implemented, runtime/visual gate pending

The real Activity displays explicitly unverified names and full PC IDs, allows
or rejects the exact pending session, and lists saved IDs/expiry with confirmed
renew/revoke actions. A storage failure stops connections and is not shown as
successful revocation. The desktop displays its full ID while pending and keeps
Stop independent of phone interaction.

The API 35 conformance harness uses independent Java keys, the real Activity /
LanServer / ClientStore, injected pointer events, persisted-state assertions,
live socket effects and screenshots. It covers EN/JA, narrow/scaled layouts,
approve/reconnect/renew/revoke/reject, camera remaining stopped, and storage-failed
startup. It requires an explicitly disposable fresh emulator profile, restores
locale/font/window/mode settings and removes its own temporary grants.

`gxkit` is installed and its tool inventory was inspected. It checks declared
artifacts; it is not an Android interaction/capture driver. This gate instead
uses Android instrumentation and UiAutomation. Executing the harness and
inspecting its screenshots are separate from compiling it. Any unavailable
unbiased visual review remains named, not silently replaced by implementer
inspection or fixture snapshots.

### 3. Wire proof and shared gate — implemented, local checks pass

[Authorization v2](../../protocol/client-authorization-v2.md) is the wire owner:
public-point enrollment and a domain-separated proof bound to transport, phone
binding, one-use nonce, point and label. The unshipped v1 prototype is removed;
older peers fail rather than downgrade. Shared binary frames and transcript
fixtures are consumed by Rust, Kotlin and native instrumentation.

`PhoneAuthorization` owns challenge/proof/pending/authorized state and deadlines;
`AuthorizedMediaSession` enforces inbound and outbound access for both transports.
The Activity cannot grant access by starting Camera2. Revoke/expiry/clock reversal
and repository failure close active access independently of UI/reader ticks.
Host startup loads its identity and finishes proof before opening decoder/output.

Local checks for this integration include the Kotlin suite (with Activity and
instrumentation artifact checks), Rust default/no-default/capture suites,
TLS/socket/subprocess peers, strict clippy in all three configurations, formatting,
diff checks and Windows no-default all-target cross-compilation. A label-policy
regression went red on both implementations before scalar-based repair: Kotlin
accepted supplementary invisible tags and Rust accepted format controls. An
abandoned-prompt cooldown regression also failed before repair. Omitting the
phone binding from the Rust transcript failed its wrong-phone signature check;
bypassing the common Android permission check failed three socket/capture cases.
Both injected mutations were restored; rerun positive gates on the restored tree. These
checks do not establish native Android UI behavior or native Windows consumer use.

## Remaining acceptance and follow-through

- Run the new exact-revision Android/Windows workflows; require semantic outcomes
  and source/APK provenance, not green icons alone. Publish exact evidence on
  Draft PR #11 and tracking issue #10.
- Inspect EN/JA narrow/scaled captures and the action/storage/socket assertions.
  Do not call screenshots independent visual acceptance without that review.
- Keep independent authorization-design review and physical USB/Camera2/native
  consumer acceptance as separate, unpassed merge/release gates.
- Explicit identity recovery UX is follow-up work. Preserve failed records and
  report unavailable storage rather than automatically reset or rotate keys.

See [choices](choices.md) for design trade-offs and
[the user/security contract](../../docs/per-pc-authorization.md) for behavior.
