# Independent review of the authorization repair (2026-10-16)

Second independent review, run after the first review's findings were repaired.
It reviewed the **repair commit** rather than the original work, and it found one
real regression the repair had introduced. **Not** human security certification,
device/visual acceptance or release approval.

- Reviewer: `pi --print`, model `opencode-go/deepseek-v4.1-flash`, `--thinking
  high`. Read-only, static only.
- Reviewed revision: `bd41cd9c5b51e7604c603dc6e3e3017d62a83230`, in an isolated
  clone (`git clone --local`) so the authoring worktree could not be touched.
  Diff under review: `c90b9af..bd41cd9`.
- Reviewer evidence: static source/Git inspection. No build, test, device,
  emulator, service or other agent. Every behavioural claim it made is `NOT RUN`.
- It read the changed files in full plus their callers, and enumerated the
  threads/monitors involved (auth timer, transport readers, outbound writer,
  Camera2 handler, MediaCodec callback, the new teardown executor, main looper,
  `PhoneAuthorization.lock`, `FramedOutbound.lock`, `MainActivity.cameraLock`,
  the repository monitor).

## Findings and disposition

### REV-001 — revocation stopped taking effect until the durable write returned (Medium; real regression, FIXED)

Moving the persistence write outside the owner monitor cured AUTHREV-002 but
changed revocation semantics: the frame gate no longer blocked behind the write,
so while a stalled Keystore/`commit()` call was still running, `permitted()` read
the **pre-revocation** snapshot and kept authorizing frames. Before the repair
the gate blocked on the same monitor the write held, so a stalled write
suppressed output. The reviewer rated the contract change real and unguarded.

Owner verification: **STATIC, confirmed** by reading the paths it named
(`PhoneAuthorization.revoke` → `ClientRepository.publish` → `permitted()`).

Repair: revocation now records the PC ID in memory **before** the durable write
and `checkAuthorized()` consults that set, so the live session loses access
immediately; the write still happens outside the monitor, so expiry and teardown
stay unblocked. A fresh approval of the same PC clears the in-memory entry (it
only ever applied to the live session).

Guard: `revocation_stops_the_live_frame_gate_before_the_durable_write_returns`
reads the gate from another thread while the write is latched, so a blocked gate
or a still-authorized gate fails the assertion instead of hanging. It was run
against the pre-repair behaviour and failed (`revocation must stop the frame gate
before the write returns`); it passes on the repaired tree (97 cases). The
mutated file was restored byte-for-byte from a copy taken before the run.

### REV-003 — cancellation could be missed during camera-session construction (Low; FIXED)

`MainActivity` installed the bridge and then called `start()`, which created and
started the `CameraEncoderSession`. A cancellation landing in that window closed
a bridge that had no session yet; the single-shot cancellation was then already
latched, so nothing closed the session the main thread went on to start. The
reviewer could not build a path where repeating capture survived (`onOpened`
re-checks authorization), so it stayed Low, with a bounded transient open as the
residual.

Repair: `CameraBridgeController.start()` and `close()` are now serialised on one
lock, and the cancellation latches before the session is created, so a bridge is
either closed after its session exists or refuses to start.

Guard boundary: this is a construction-order fix reasoned from the code; there
is no JVM test (it needs Camera2), so it is confirmed by inspection only.

### REV-002 — the first repair's guard does not pin the UI-thread wiring (Low/Medium; ACCEPTED, named)

The AUTHREV-001 regression test asserts that `AuthorizedMediaSession` invokes the
cancellation sink it was given. Reverting `MainActivity.onCaptureCancelled` to
`handler.post { stopCamera() }` — the original defect — would leave that test
green, because the sink in the test is the test's own counter.

Disposition: accepted as a named guard gap rather than papered over. The
off-UI property is enforced by construction in `MainActivity` (the cancellation
body only enqueues to a dedicated single-thread executor; there is no `handler`
gate on that path) and can only be pinned by an on-device test, which the current
conformance fixture cannot provide — its harness has no camera producer. The
concrete check is listed for #8: with a camera attached, expire/revoke a grant
while the main looper is deliberately busy and assert repeating capture stops.

### REV-004 — the single teardown thread can block unboundedly in framework calls (Low/Medium; ACCEPTED, needs device)

`stopRepeating`/`abortCaptures`/`camera.close()`/`codec.stop()` have no timeout,
so a wedged camera HAL would queue every later cancellation behind the first (and
the UI path could wedge the same way). The reviewer confirmed there is **no
monitor cycle** — only unbounded platform calls.

Disposition: accepted. The project cannot bound a platform call it does not own,
and inventing a timeout would risk aborting teardown half-way. Needs device
confirmation under #8 (drive two sequential sessions with the HAL held).

## Needs-verification items

- **NV-1 (monitor cycles).** The reviewer traced every join reachable under a
  lock: `outbound.close()` joins the writer for ≤500 ms while the auth monitor is
  held, and `cameraThread`/encoder `callbackThread` joins are taken without it.
  **No cycle found.** Device/StrictMode confirmation remains.
- **NV-2 (approval persisted but no live grant).** If the pending deadline
  expires while the Keystore write stalls, the record is durable and `grant()`
  never runs. Accepted behaviour, now documented: an Allow the user confirmed is
  durable even when that particular request then times out; the PC must still
  reconnect and prove possession, and the next connection is then authorized
  without a second prompt.
- **NV-3 (MediaCodec call sites).** `configure/start` run on the main thread
  while `stop/release` can now run on the teardown or camera thread. Cross-thread
  `MediaCodec` use already existed here (`requestIdr` from the writer thread);
  the stop/release move needs on-device confirmation under #8.
- **NV-4 (test coverage).** The persistence guard did not assert the frame gate.
  Superseded by the REV-001 guard added above, which does.

## Outcome

`HOLD` on the reviewed commit, with REV-001 as the named blocker and REV-002 as a
named guard gap. REV-001 and REV-003 are repaired with evidence above; REV-002,
REV-004, NV-1, NV-2 and NV-3 are recorded as accepted limitations with their
discriminators, and none of them is a claim of device verification.

The reviewer explicitly did **not** find a path where post-authorization frames
are disclosed, a monitor cycle, a lost or overwritten grant, or a wrong-session
cancellation.
