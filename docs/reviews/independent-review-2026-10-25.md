# Independent review (local Codex CLI, read-only), 2026-10-25

## Scope and evidence boundary

Asked for by the implementer, in `request-2026-10-25.md`, to review the sixteen commits merged since
`9c45226` - the change set that request describes. The reviewer was an independent agent on this
machine (the local Codex CLI), given the request, a patch file of exactly those sixteen commits, and
read-only access to the tree. It shares the machine with the implementer, so it is an outsider to the
code but not to this environment.

What it could do: read the whole tree, and run the **already-compiled** Android test bytecode (10
tests from `ReaderHandoverTest` and `StopActionTest` passed). What it could **not** do, in its own
words, and therefore did not: a fresh Android or Cargo build (the sandbox refused lock-file
creation), and `adb` (its daemon could not write its log). Its device-dependent conclusions are marked
as such, and the working tree stayed clean.

The review is reproduced below as it was written. The implementer's disposition follows it, in the
same document, as this repository does for every review.

---

## Findings

### ID-001 — the frame-owner instrument attributes an unrelated frame to the measured host write (Medium; gate effect measured, USB attribution static)

- **Where:** `android/app/src/androidTest/java/dev/nekomario/amb/UsbFrameOwnerMeasurement.kt:175`, `host-rs/tools/send-hello.py:120`.
- **Sequence:** an earlier PING reaches a fresh session in `PROOF`. `PhoneAuthorization.receive` rejects its type and reports `PC authorization proof rejected`. The instrument matches that substring and prints `consumed=true`, identifying the measured host frame despite never inspecting its type, sequence, payload, or relationship to the host write.
- **Observed versus inferred:** I ran JShell against the existing compiled `PhoneAuthorization` classes, started a fresh gate, and supplied only `Wire.Header(TYPE_PING, …)` with eight payload bytes:

  ```text
  PING exception=PC authorization required before controls
  No HOST_HELLO supplied: [PC authorization proof rejected]
  ```

  The catch at `PhoneAuthorization.kt:205-207` supplies the same reason for multiple failures. The host script’s “read before writing” reads `inp`—phone→host. It cannot establish that host→phone contains no earlier frame.
- **Impact:** the success predicate proves a session reported a rejection; it does not identify the experiment’s write as its cause.
- **Evidence boundary:** measured on existing JVM bytecode; attribution logic rechecked against source. Persistence and delivery of a queued PING on this device remain **needs-device**, not reproduced here.

### ID-002 — idle iterations overlap, and the final verdict invents deadline observations (Medium; static)

- **Where:** `android/app/src/androidTest/java/dev/nekomario/amb/UsbFrameOwnerMeasurement.kt:159`, particularly lines 175–184.
- **Sequence:** run the default instrument with an idle endpoint. After starting a session, it sleeps 200 ms and observes it for 4,500 ms. With ordinary scheduling, it therefore advances after approximately 4.7 seconds, before the five-second authorization deadline. It neither closes nor joins that session before opening the next descriptor; cleanup occurs only in the outer `finally`.
- **Observed versus inferred:** the source explicitly allows an iteration to end as `still blocked after 4.5 s`. Nevertheless, the final negative verdict says **“every one ended by its own proof deadline”**. It prints that verdict after `openAccessory` returns null, after skipped iterations, and after non-proof errors too.

  A further consequence is an inference grounded in the loop: the next iteration can open while the previous reader still lives. `threadNamed()` selects any live `amb-accessory-reader`, rather than the new session’s reader, while `errors` belongs only to the new session. Liveness and errors can therefore describe different sessions.
- **Impact:** an idle run can manufacture the two-reader condition it purports to control, and its final text reports a control it did not actually observe.
- **Evidence boundary:** **read only**. Exact timing and the resulting second-reader EIO require a device; the unconditional false description does not.

### ID-003 — “blocked before close” is still an assumed state in the raw release measurement (Low; static)

- **Where:** `android/app/src/androidTest/java/dev/nekomario/amb/UsbReaderReleaseMeasurement.kt:225`, consumed at lines 89–97.
- **Sequence:** construct a `BlockingReader`, start its thread, but delay that thread before it enters `stream.read`. `outcome` already equals `"blocked"` from construction. After 300 ms, the measurement counts the precondition as established and closes the descriptor. The reader subsequently attempts the read, throws, and counts down `finished`; `await` succeeds and the instrument counts a released blocked read.
- **Observed versus inferred:** the initialization and success condition are directly visible in the source. There is no reader-entry acknowledgement or stack check in the raw path. Delayed scheduling is the inferred reproduction condition; the 300 ms sleep reduces its likelihood but does not exclude it.
- **Impact:** the raw result can still report successful release without a read having been blocked when close occurred. This does **not** demonstrate that the recorded device runs were false.
- **Evidence boundary:** **read only**; the scheduling counterexample was not reproduced on Android.

### ID-004 — the new Q1 disposition’s header check does not exclude a silently substituted frame (Low; constructed input, static decoder verification)

- **Where:** `docs/reviews/independent-review-2026-10-24.md:359`, relying on `usb.rs:354-358` and `wire.rs:478-520`.
- **Sequence:** construct an outer `VIDEO_AU` whose payload is this complete, valid zero-payload `HELLO_ACK` header:

  ```text
  414d4231010200004d000000000000000000000000000000
  ```

  If alignment starts on that payload transfer after the outer header was consumed, `align_stream` recognizes `AMB1` and retains it. `Header::decode` accepts version 1, kind 2, sequence 77, and payload length zero. The framing layer assembles a `HELLO_ACK`; the phone’s outer frame was `VIDEO_AU`.
- **Observed versus inferred:** a Python command constructed and unpacked those exact bytes. Acceptance follows directly from the real Rust decoder’s checks and its `HELLO_ACK => length == 0` rule. I did **not** execute the Rust decoder or establish that the shipping camera produces this payload.
- **Impact:** the new disposition’s unconditional “cannot produce a silently wrong frame” conclusion is unsupported. Magic and length validation check syntax, not whether the bytes originated at a frame boundary. This is a documentation finding, not a newly introduced parser regression.
- **Evidence boundary:** constructed bytes plus **read-only** verification; hardware frequency and downstream effects remain unmeasured.

## Hypotheses I could not break

- **B1:** no alternate shipping pairing/capability sender bypass found; both physical session implementations inherit the gated methods.
- **B2:** no token-free stop route found in the inspected intent handling and manifest; cross-process Android execution remains untested.
- **C2:** survived static inspection: the added alignment-result variable feeds only tracing; the result still does not gate connection success.
- **D1:** survived source inspection against the explicitly documented anchored-window contract; boundary bursts are already acknowledged.

**Unresolved:** A2’s kernel endpoint-release question, D2’s on-wire consequence, and E1 under active adb traffic require device access. They did not survive an experiment; those experiments were **NOT RUN**.

Cargo and Gradle validation attempts stopped on read-only lock-file creation. Existing compiled `ReaderHandoverTest` and `StopActionTest` bytecode passed **10 tests**; that is not a fresh HEAD build. adb could not start its daemon because `/tmp/adb.1000.log` was unwritable. The working tree remained clean.

---

## Disposition (implementer, 2026-10-25)

**All four findings are accepted; none is dismissed.** The reviewer was asked to attack the verdicts of
the new instruments before anything else, and it found a case where one of them could report success
without its stated cause, by running the code rather than by arguing - which is the most useful shape a
finding can arrive in here.

**ID-001 (Medium) - FIXED, and the counterexample now fails.** The verdict used to be a substring of a
reason string that several failures share, so it proved only that *some* frame was refused; the
reviewer reproduced that by feeding a `TYPE_PING` to a fresh session and getting the same sentence.
`AuthorizedMediaSession` now reports every host frame it reads through a seam in the shape of its
existing ones (`onFrameRead`, defaulted, no behaviour change; `AccessorySession` forwards it), and the
instrument requires the frame to be the one the host wrote: `type=0x0e` with the sequence the host
script now stamps (`0x1234`). Measured on the XQ-GE44 after the fix, and the distinction shows up in
the output rather than being asserted:

```
REPORT   iteration 1 ended: still blocked after 4.5 s
REPORT   frames this iteration read: []
REPORT   no frame matching the host's (type=0x0e sequence=4660) was read in this iteration
REPORT iteration 2 ended: PC authorization proof rejected
REPORT   frames this iteration read: [type=0x0e sequence=4660 len=0]
REPORT outcome: the blocked reader consumed the host's frame - PC authorization proof rejected
```

The first iteration is now *rejected* as unattributable instead of counted, which is exactly the
reviewer's complaint. Installing the fix needed the app APK as well as the test APK, because the seam
changes a constructor signature; the USB grant survived that update (same `versionCode`), so the
instrument's permission path still has not run.

**ID-002 (Medium) - FIXED.** Three things changed. Liveness is the session's own answer
(`session.readerAlive()`) rather than a thread found by name, because the name is shared by every
reader the instrument arms and the previous iteration's reader could be the one inspected. Each
iteration now takes its reader off the endpoint before the next one arms (`closeIteration`), instead of
leaving every session armed until the outer `finally` - which is what could manufacture the two-reader
state the instrument is supposed to control for. And the negative verdict no longer claims that every
iteration ended by its own deadline: it reports the tallies it actually observed (ended by deadline,
skipped, ended for another reason) and the frames read.

**ID-003 (Low) - FIXED.** `BlockingReader` sets an `entered` latch immediately before the read, and an
iteration counts as "blocked before close" only if the read was entered *and* had not returned. The
300 ms sleep is no longer treated as proof. Re-measured after the fix: **10/10 raw and 10/10 session**,
both with the stronger precondition, and the two-descriptor behaviour unchanged (`EIO` on the second
reader). The fix did not change the result; it changed what the result proves.

**ID-004 (Low, documentation) - FIXED, and the claim was wrong.** The disposition of the previous
review's Q1 said a wrong alignment verdict "cannot produce a *silently wrong* frame". The reviewer
constructed a counterexample - a complete, valid, zero-payload header
(`414d4231010200004d000000000000000000000000000000`) sitting inside an outer `VIDEO_AU` payload, which
`align_stream` would keep and the real decoder would accept as `HELLO_ACK` - and the claim is now
written as what the code supports: the magic and length checks reject a **malformed** frame, which is
the visible failure mode, but a correctly-shaped frame can still be misparsed when alignment was
assumed. Whether the shipping camera emits such a payload is unmeasured, as the reviewer says.

### What this pass did not reach

**A2 - ANSWERED, with a number.** The instrument now asks it directly rather than leaving it to be
inferred: after each close it opens a fresh descriptor and starts a fresh reader. Measured on the
XQ-GE44, **10/10 fresh descriptors blocked on the endpoint immediately afterwards** - blocked, not
`EIO` - while a reader that is *live* still refuses a second one. So the endpoint is released, and a
live reader is what refuses the newcomer; it is not that one release failed to take effect. See
`docs/evidence/2026-10-25-usb-reader-release.md`, "A2, answered".

**E1 - ANSWERED, with a caveat that matters.** With USB debugging on, the phone re-attaches to adb
within 2-4 s of the accessory switch, and a 20 MiB `adb push` ran **byte-exact** while an accessory
session held a blocked reader and the host wrote to it. Four arms with the product path (including one
where the app received and handled `USB_ACCESSORY_ATTACHED`) ran at ~40 MB/s, so no throughput cost is
demonstrated for the claim as written. **But** the three runs with the frame-owner *instrumentation*
active each pushed at 3.2 MB/s - reproduced, 12x slower - and which part of the instrument causes it is
**not attributed**. Recorded in `docs/evidence/2026-10-25-g0-adb-coexistence.md`, because the next
person to measure adb throughput under an instrumentation would otherwise blame the thing under test.

**D2 - ANSWERED by showing it.** The coverage row claimed the consequence of two readers sharing an
endpoint; with the repository's own host answering a real proof against the two-session state, the
consequence appears: a blocked reader consumes the host's `CLIENT_AUTH` (`type=0x0b len=134` in the
device report) and the host is either left waiting for an approval it did not choose or told `the phone
rejected this PC's signature` - the documented `BAD_SIGNATURE`, shown rather than claimed, including the
leftover-challenge path that produces it. Table and quotes:
`docs/evidence/2026-10-25-usb-frame-owner.md`, "D2, shown".

Every question this review left is now answered, except the one that needs the app's USB grant cleared
first - an instrument path, not a claim. The reviewer's four unbroken hypotheses - B1, B2, C2, D1 - are
recorded here as results, not as gaps.
