# Independent review of per-PC authorization (2026-10-16)

Independent model security review of the committed authorization scope, plus the
owner's own reproduction and static verification of each claim. **Not** human
security certification, visual acceptance, physical-device acceptance or release
approval.

- Reviewer: separate model run, read-only (`codex exec --sandbox read-only`),
  one run under a fresh, explicitly granted review authorization.
- Reviewed revision: `01cfcc2a7e5ee9df93baa94e224f6c75b60bdd57` (HEAD matched,
  clean tree at start and end; no files changed by the reviewer).
- Scope: Android authorization/persistence/UI/transport/framing/camera/quality,
  Rust identity/store/DPAPI/authorization/wire/transport/TLS/video and the
  contracts in `protocol/` and `docs/`.
- Reviewer evidence: static source/Git inspection only. It ran no builds, tests,
  exploits, browsers or services, and did not open earlier review reports.
- The base commit named in the review brief was unavailable in the reviewer's
  checkout, so it reviewed HEAD directly rather than a base diff.

## AUTHREV-001 — capture teardown depends on UI progress (Medium; merge blocker)

- `AuthorizedMediaSession.closeMedia()` (`AuthorizedMediaSession.kt:42-46`) only
  clears the IDR requester, closes the authorization session and closes the
  outbound writer. It does not stop the Camera2/MediaCodec producer.
- `MainActivity.onAuthorizationChanged` (`MainActivity.kt:383-392`) is the only
  path that calls `stopCamera()` on authorization loss, and it **posts** to the
  main handler. `CameraBridgeController.close()` runs only from `stopCamera()`,
  which is reachable only from the main thread.
- `CameraEncoderSession` callbacks gate on their own `closed` flag
  (`CameraEncoderSession.kt:89,122`), not on authorization, so a pending
  Camera2 callback can still install repeating capture after authorization ends.

Owner verification: **STATIC, confirmed** — the reviewer's claim matches the
call graph above (grepped callers, `MainActivity.kt:389` inside `handler.post`).
Consequence: the physical camera keeps capturing after expiry/revocation until
the main thread runs the posted shutdown. Output is still gated by
`permitted()`/`isAuthorized()` and the transport is closed, so no post-revocation
frame disclosure was established; the confirmed defect is continued/late
**capture** (camera-on privacy), with no authorization-owned bound on the delay.

Reviewer reproduction (NOT RUN by the reviewer; owner path below): hold the main
handler while the camera handler and authorization timer run, expire the grant,
and observe that repeating capture has not stopped.

This violates the stated requirement that revocation/expiry/storage failure stop
capture independently of UI progress. **Merge is blocked on this finding** until
the authorization/session lifetime owns a direct, non-UI producer-cancellation
path (including in-flight Camera2 startup callbacks), or the requirement is
explicitly re-scoped and documented.

## AUTHREV-002 — persistence can stall the shared authorization lock

- `PhoneAuthorization` performs approval/renew/revoke persistence while holding
  its owner monitor (`PhoneAuthorization.kt:29-36`; `ClientRepository.kt`), and
  `stored` runs the repository write on the caller thread.
- In production the blocking operation is Android Keystore encryption plus
  `SharedPreferences.commit()`.

Owner reproduction (**EXECUTED, failed closed as expected**): a diagnostic test
injected a repository writer that blocks on a latch, authorized a session,
started `revoke()` on a second thread, then advanced beyond expiry and called
`tick()` and `close()` on two more threads. Result:
`closure=false tickDone=false closeDone=false tickThread=BLOCKED
closeThread=BLOCKED` — expiry and owner close cannot tear down the transport
until the persistence call returns. The probe is a cache-only diagnostic, not
shipped tooling; production Keystore/commit stall duration was **not** measured.

Boundary: because every inbound control and output path re-checks authorization
through the same monitor, a stalled write also blocks readers and writers, so
this is a delayed-teardown liveness issue, not a demonstrated bypass. No
permanent stall or Java monitor deadlock was established.

## Property checklist (reviewer static assessment)

| Property | Assessment |
| --- | --- |
| Proof/transcript binding | Domain separation, transport, phone binding, fresh nonce, point and bounded UTF-8 label covered; Rust checks LAN binding against the pin TLS enforces; Android derives binding from its Keystore certificate (provider selection not executed). |
| USB/LAN gate coverage | Both production adapters use the shared `AuthorizedMediaSession`; no bearer-only, empty-store or old-peer bypass found. |
| Live-session/stale-action fencing | Approval views retain their exact session; stale approvals rejected; quality callbacks recheck origin. Camera lifetime is the exception — **AUTHREV-001**. |
| Persistence/fail-closed | Approval publishes after successful persistence; write exceptions latch failure; corrupt/missing/orphaned grant state fails closed. |
| Revoke/expiry/locking | Independent timer and transport closure exist; capture still depends on UI progress (**AUTHREV-001**) and persistence can delay teardown (**AUTHREV-002**). |
| Host pre-grant startup/Stop | Production exchange precedes decoder/output startup; Pending is not permission; Stop polls are bounded. Runtime responsiveness not measured. |

## Outcome

`BLOCK` on AUTHREV-001. Resolve AUTHREV-002 with a focused blocking-persistence
discriminator. The independent-review authorization granted for this run is
consumed; no further independent model review is authorized.
