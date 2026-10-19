# Per-PC authorization implementation

Requested scope: real identity/list persistence, phone approval/revocation UI,
and mandatory camera/control authorization over both USB and pinned TLS Wi-Fi.
All three are committed on the integration branch. Local mechanical gates pass;
API 35 production-Activity conformance passed at `36ab5b4`. Later lifecycle
repairs still require their exact-head CI; neither conformance nor screenshots
are independent security/visual acceptance.
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
that case passed in the later `36ab5b4` emulator run, not by inference from
previous storage CI.

### 2. Approval and revocation — API 35 conformance, visual acceptance pending

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
Both injected mutations were restored and the positive local matrix was rerun.
These checks do not establish native Android UI behavior or native Windows
consumer use.

### First integrated CI attempt

At `153a73a`, Android CI built the APKs but failed one of 93 JVM cases:
`Size.getWidth()` is not implemented by Gradle's mockable Android jar, whereas
the local android-all probe executed it. Keep Android dimensions at the Camera2
producer edge; the shared media/authorization boundary accepts plain numbers.
The retained socket case also checks the emitted dimensions, rate and bitrate.
The cache-only probe now warns about direct Android references rather than
presenting itself as Gradle-equivalent.

Emulator run `38781385547` passed the independent OpenSSL TLS/v2/quality exchange
and reached real EN reject, pending, stale-action, approve and reconnect effects.
It stopped at the renewal button lookup: Android exposes transformed uppercase
button text. Six PNG/metadata pairs were collected before shutdown. The pointer
driver now matches that transformation; renew/revoke, JA/scaled and storage-failed
startup still required a passing rerun. That partial attempt was not conformance
success or independent visual acceptance.

### API 35 rerun and subsequent lifecycle guard

At `36ab5b4`, emulator run `38783327991` passed the actual semantic marker and
all 15 PNG/metadata pairs at 840×1400 / 320dp, with EN font1.0 and JA font1.3.
The production-Activity pointer actions checked durable approval, unchanged
known-key expiry, confirmed renewal, live revoke/socket closure, stale actions,
rejection and corrupt-storage startup. Source/APK receipts were collected before
shutdown. This is functional emulator evidence, not independent visual approval,
physical USB/Camera2 or native consumer acceptance.

A later shutdown review found an unstarted `LanSession.close()` skipped physical
socket closure because `running` was still false. Its real-socket regression
failed with `SocketTimeoutException` (1/94). Close now owns physical I/O regardless
of start state, and a closed socket cannot restart its session. All 94 local JVM
cases pass after the repair; its exact-head native workflows remain required.

### Independent authorization review — merge blocked

A separately authorized, read-only independent model review of this committed
scope reviewed `01cfcc2` and returned **BLOCK**: [review record and evidence
boundary](../../docs/reviews/per-pc-authorization-review.md). Findings:

- **AUTHREV-001 (Medium, blocker):** `AuthorizedMediaSession.closeMedia()` did
  not stop the Camera2/MediaCodec producer; only the main-thread `stopCamera()`
  path did, so expiry/revocation left capture running until the UI thread ran.
  Output remained gated and the transport closed — continued capture, not
  post-revocation frame disclosure.
- **AUTHREV-002:** persistence held the shared authorization monitor, so a
  stalled Android Keystore/`commit()` call delayed expiry and owner close.

Both are now repaired with one guard each: the session takes a required
`onCaptureCancelled` sink (a transport cannot silently omit it) and stops the
producer off the UI thread; the repository publishes wait-free snapshots and
`PhoneAuthorization` writes outside its monitor. Each guard was run against the
pre-repair behaviour and failed there before passing on the repaired tree; see
the review record for the exact failures and the restore discipline.

A second, separately authorized independent review then examined that repair
and found a regression the repair itself introduced
([repair review record](../../docs/reviews/per-pc-authorization-repair-review.md)):
revocation stopped taking effect until the durable write returned, because the
frame gate no longer blocked behind the write. Revocation now takes effect in
memory before the write and a regression pins the frame gate; the same review
found and this branch fixed a cancellation window during camera-session
construction. Its remaining items are accepted as named limitations with
discriminators (unbounded platform calls on the teardown thread, MediaCodec call
sites, an Allow that persists while its request expires). The UI-wiring guard gap
that review named is closed: the producer stop is a production class owning its
own thread, and both its contract and the Activity's use of it have guards that
were observed failing on the pre-repair wiring.

Boundaries: neither review is human security certification, and the
Camera2/MediaCodec teardown has no device execution yet. Confirm it with the API
35 conformance run and physical capture before claiming that authorization loss
stops the camera on a device. Do not treat the guards as independent sign-off.

## Remaining acceptance and follow-through

- Run the new exact-revision Android/Windows workflows; require semantic outcomes
  and source/APK provenance, not green icons alone. Publish exact evidence on
  Draft PR #11 and tracking issue #10.
- Inspect EN/JA narrow/scaled captures and the action/storage/socket assertions.
  Do not call screenshots independent visual acceptance without that review.
- Independent authorization-design review happened once (above) and returned a
  blocker; physical USB/Camera2/native consumer acceptance remain separate,
  unpassed merge/release gates.
- Explicit identity recovery is implemented on the host (`--replace-identity`,
  and the GUI action behind a two-step confirmation): it quarantines the
  unusable file under a `.corrupt-<unix-seconds>` name, publishes a new key, and
  states that every phone approval is invalidated; a key that still reads is left
  alone unless `--force`. Failed records are still preserved and unavailable
  storage is still reported rather than reset automatically. The phone-side
  explicit repair remains open.

See [choices](choices.md) for design trade-offs and
[the user/security contract](../../docs/per-pc-authorization.md) for behavior.
