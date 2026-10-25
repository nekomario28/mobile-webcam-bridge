# The single-reader release, measured on a real endpoint (2026-10-25)

The repository had one sentence it had only ever *read*: "a blocked read on the accessory
stream is not interruptible, so closing the descriptor is what unblocks it"
(`AccessorySession.closeAndAwait`). The guard that sentence justifies — refuse to open a
second accessory session while the previous reader is still alive — was therefore insurance
nobody had priced. `security-coverage.md` said so in as many words: *treat the device half as
unreviewed*.

This is that measurement. It answers three questions and refuses to answer a fourth.

## The instrument

`android/app/src/androidTest/java/dev/nekomario/amb/UsbReaderReleaseMeasurement.kt`, reached
through the instrumentation the repository already has (no new dependency, no new
`testInstrumentationRunner`):

```
adb shell am instrument -w -e physicalDevice true -e usbReaderMeasurement true \
  dev.nekomario.amb.test/dev.nekomario.amb.UsbPermissionInstrumentation
```

It asserts the **precondition on every iteration**: a read that was not blocked when the
descriptor closed proves nothing about unblocking one, so those iterations are reported and
excluded from the rate rather than counted as successes. The first version of this instrument
did not do that, and returned `released=true in 0 ms` twenty times — the exact shape of a
measurement that has not established that it measured anything.

It touches no app state: the authorization owner is an in-memory repository, so the device's
own approvals are neither read nor written.

## What was measured

Two runs, both identical. XQ-GE44, `Build.VERSION.SDK_INT` 37, accessory
"Android Media Bridge AMB Host", no host process attached (an idle endpoint — the stated case).

**A raw blocked read on one descriptor, ten times:**

```
raw 1: blockedBeforeClose=true afterClose=exception InterruptedIOException: read interrupted by close() on another thread returned=true in 0 ms
...
raw release: 10/10 blocked reads returned once the descriptor closed inside 1500 ms
```

The precondition was true on every iteration, the return was immediate, and the platform names
the mechanism in its own message.

**A real session (`AccessorySession.start()` then `closeAndAwait(1500)`), ten times:**

```
session 1: readerAliveBefore=true errorsBefore=0 released=true in 0 ms readerStillAliveAfter=false
    reader stack: libcore.io.Linux.readBytes <- libcore.io.Linux.read
...
session release: 10/10 blocked readers came back once the close ran inside 1500 ms
```

The reader thread was found by name (`amb-accessory-reader`), was alive, and its stack proves
it was inside the read syscall when the close happened. No error had been reported before the
close. So "closing the descriptor unblocks the blocked reader" is now measured on a real
endpoint — 10 of 10, twice — rather than read.

**Two descriptors on one accessory:**

```
two descriptors on one accessory: first=true second=true
  both open, no host: reader1=blocked reader2=exception IOException: read failed: EIO (I/O error)
  after closing descriptor 1: reader1=exception InterruptedIOException... reader2=exception IOException: read failed: EIO (I/O error)
```

Both `openAccessory` calls succeed. **The platform does not prevent two readers** — which is
what makes the app-level guard load-bearing instead of redundant, and it is now an observation
rather than an assumption.

It also sharpens the mechanism the code comment describes. The *old* reader stays blocked on
the endpoint and is the one that receives what the new host sends; the *new* reader's first read
fails immediately with `EIO`. So the two do not silently interleave frames: the endpoint stays
held by the old reader and the new session cannot read at all. The consequence for the host is
the documented one — a proof answered against a replaced challenge (`BAD_SIGNATURE`) or a
capability frame ahead of the authorization result — but the reason is refusal, not theft. The
guard is what keeps the phone out of that state.

## Which build ran

The runs did not all run the same APK, and the difference is recorded rather than smoothed over:

- The first two runs measured the app that happened to be installed — `versionCode 2000 /
  versionName 0.2.0` — whose `AccessorySession` has no `readerAlive()`
  (`NoSuchMethodError`), so it predates the ID-002 fix. Its `closeAndAwait` is:

  ```kotlin
  fun closeAndAwait(timeoutMs: Long): Boolean {
      running.set(false); runCatching { pfd.close() }; closeMedia()
      val reader = readerThread
      readerThread = null
      if (reader != null && Thread.currentThread() !== reader) {
          reader.interrupt(); runCatching { reader.join(timeoutMs) }
          return !reader.isAlive
      }
      return true
  }
  ```

  The release semantics are the fixed ones already (`return !reader.isAlive`); what the fix adds
  is keeping the handle when the reader survived. So the rate applies to both, and the
  *durability* half is not what this document measured — `ReaderHandoverTest` covers the object
  it feeds.
- The final run reinstalled the **branch build itself** — `adb install -r` of the debug APK
  `assembleDebug` produced from the branch — and measured again: identical, 10/10 and 10/10,
  both descriptors open, `EIO` on the second reader. The build is verified rather than assumed:
  `ReaderHandover` and `StopAction` exist only on the branch, and both are in the installed
  APK's `classes3.dex`.

**No permission dialog appeared and no tap was issued.** The USB grant survived the update —
`dumpsys usb` still lists the app's uid under `accessory_permissions`, and the harness's own
"no USB permission for this accessory: asking the device now" line never appears in the log.
That narrows the repository's earlier note that the grant is "cleared by reinstalling or
updating the app": what was measured here is an update at the *same* `versionCode`, which kept
it. A version bump was not tried, so the note is not contradicted, only narrowed.

The attempt to answer a dialog automatically is recorded for what it cost. The helper dumped
whatever window was focused and read the phone's unrelated UI, because it never checked which
window it was looking at. It issued no tap — the helper prints a line for every tap and the log
has none — and the measurement did not need one. An instrument that answers a security dialog
must confirm the focused window is that dialog first.

## Verification boundary

- **Idle endpoint.** No host process was attached, so this is "a read with nothing to read".
  The mode the intermittent symptoms were actually seen in — a *host* driving frames while an
  old reader still holds the endpoint — is **not** measured here and is not claimed. A clean
  rate here is not evidence that the guard is unnecessary; it is evidence that the cheap half
  of the claim is reproducible without a host, and which half needs one.
- One device, one host, three runs of the same instrument — two against the previously
  installed build, one against the branch build reinstalled. The 0 ms figures are the same clock on
  the phone and are not a statement about USB latency.
- `UsbReaderReleaseMeasurement` runs as part of the instrumentation APK, so nothing in CI
  exercises it: the emulator has no accessory. It is a device instrument, run by hand like
  `--usb-probe`.
