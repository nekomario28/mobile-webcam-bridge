package dev.nekomario.amb

import android.app.Instrumentation
import android.hardware.usb.UsbAccessory
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log
import java.util.Collections

/**
 * Asks who consumes a frame the host sends while an *old* reader holds the endpoint.
 *
 * The single-reader guard exists because of a state the repository has only ever seen from the
 * host side: an old accessory reader stays blocked on the endpoint, a new session opens on top of
 * it, and the host's bytes go somewhere other than the session it thinks it is talking to.
 * `docs/evidence/2026-10-25-usb-reader-release.md` measured the *price* of the guard (closing the
 * descriptor releases the reader, and two descriptors both open) but not this, because it needs a
 * host that writes. The host half is `host-rs/tools/send-hello.py`.
 *
 * The instrument **waits for the accessory to appear** rather than requiring it, because the host
 * half does the AOA switch itself and that re-enumerates the phone: the run is started while the
 * phone is still in its normal mode. It then arms a fresh blocked reader repeatedly, because a
 * proof deadline is absolute and 5 s long: an iteration that ends by deadline is itself the
 * control - proof that an idle endpoint delivers nothing - and the host's frame lands in whichever
 * iteration it arrives in. The outcomes are distinguishable without trusting either side's
 * narrative:
 *
 * - **the old session consumes it** — its reader leaves the read and the session ends with the
 *   reason `PhoneAuthorization` gives for a frame that is not a proof ("PC authorization proof
 *   rejected"). That string is the ID-001 path: a control frame reaching a session still waiting
 *   for its proof is refused *there*, and ends the session.
 * - **nothing consumes it** — the old reader is still blocked when the wait expires, which
 *   falsifies the premise and the report says so.
 *
 * The control is inside the run rather than assumed: an iteration ending with
 * `PC authorization deadline expired` is the proof that nothing arrives on an idle endpoint, which
 * is what makes a later `PC authorization proof rejected` attributable to the host.
 *
 * Every report line is also written to logcat under the [tag], because the AOA switch can take
 * adb away for the duration and `am instrument`'s own stream may not be readable afterwards.
 *
 * No app state is touched: the authorization owner is an in-memory repository.
 */
class UsbFrameOwnerMeasurement(
    private val instrumentation: Instrumentation,
    /**
     * Extra descriptors to hold open once the measured reader is blocked.
     *
     * The point is to move one variable at a time. Measured 2026-10-25: with one extra descriptor
     * held - no reader on it - the host's write **completes** and the measured reader consumes the
     * frame. So the descriptor count is not what stopped the earlier attempt, which had timed out in
     * a state that also had a second descriptor open; and with [secondSession] the write completes
     * too, so a second *reader* is not it either. The earlier timeout belongs to the port state (the
     * cable was re-seated between the observations). Evidence:
     * `docs/evidence/2026-10-25-usb-frame-owner.md`.
     */
    private val extraDescriptors: Int = 0,
    /**
     * Start a second **session** on an extra descriptor: the state the guard prevents, and the one
     * the earlier attempt timed out in. Its reader is the one the driver refuses with EIO, so this
     * is the difference between "a second descriptor" and "a second reader" - and measured, the
     * host's write completes in this state as well.
     */
    private val secondSession: Boolean = false,
) {
    private val controlMs = 3_000L
    private val waitForHostMs = 25_000L

    /**
     * The sequence this instrument requires the host's frame to carry.
     *
     * The verdict used to be a substring of a reason string that several failures share, so it
     * proved only that a session refused *some* frame (independent review, 2026-10-25, ID-001),
     * which a reviewer reproduced by feeding a PING to a fresh session and getting the same
     * sentence. The frame now has to be the one the host wrote: this type, this sequence.
     */
    private val hostFrameSequence = 0x1234
    private val waitForAccessoryMs = 120_000L
    private val tag = "UsbFrameOwner"

    fun run(): String {
        val manager = instrumentation.targetContext.getSystemService(UsbManager::class.java)
        if (manager == null) {
            return done(report("FAIL usb-frame-owner: no UsbManager\n"))
        }
        val report = report(
            "usb frame-owner measurement on " + Build.MODEL + " (API " + Build.VERSION.SDK_INT + ")\n",
        )
        val accessory = awaitAccessory(manager, report) ?: return done(report)
        report.append("accessory ").append(accessory.manufacturer).append(' ')
            .append(accessory.model).append('\n')
        if (!manager.hasPermission(accessory)) {
            report.append("FAIL usb-frame-owner: this app holds no USB permission for the accessory\n")
            return done(report)
        }

        val held = mutableListOf<android.os.ParcelFileDescriptor>()
        val heldExtras = mutableListOf<android.os.ParcelFileDescriptor>()
        val heldOwners = mutableListOf<PhoneAuthorization>()
        val heldSessions = mutableListOf<AccessorySession>()
        try {
            // One blocked reader only: that is the state the shipping app is in, and the state
            // in which the host can write (measured, after the cable was re-seated: 24 bytes
            // completed). The two-descriptor case belongs to UsbReaderReleaseMeasurement, and its
            // second reader is refused with EIO anyway, so it could consume nothing.
            // A proof deadline is absolute and 5 s long, so a blocked reader cannot be held for
            // ever in PROOF. Rather than race it, arm a fresh blocked reader repeatedly: an
            // iteration that ends by deadline is itself the control (nothing arrives on an idle
            // endpoint), and the frame shows up in whichever iteration it lands in.
            val deadline = System.currentTimeMillis() + waitForHostMs
            var iteration = 0
            var answered: String? = null
            val framesRead = Collections.synchronizedList(mutableListOf<String>())
            var deadlineEnded = 0
            var skipped = 0
            var otherEnded = 0
            while (System.currentTimeMillis() < deadline && answered == null) {
                iteration++
                val pfd = manager.openAccessory(accessory)
                if (pfd == null) {
                    report.append("FAIL usb-frame-owner: openAccessory returned null on iteration ")
                        .append(iteration).append('\n')
                    break
                }
                val owner = PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = {})
                val errors = Collections.synchronizedList(mutableListOf<String>())
                // The seam: every host frame this session reads is reported, so the verdict can
                // name the frame instead of a reason string shared by several failures.
                val framesThisIteration = Collections.synchronizedList(mutableListOf<String>())
                val session = AccessorySession(
                    pfd, {}, { reason -> errors.add(reason) }, owner, {}, {},
                    onFrameRead = { header ->
                        framesThisIteration.add(describeFrame(header))
                        framesRead.add("iteration " + iteration + " " + describeFrame(header))
                    },
                )
                held.add(pfd); heldOwners.add(owner); heldSessions.add(session)
                session.start()
                Thread.sleep(200)
                val alive = session.readerAlive()
                val reader = threadNamed()
                report.append("iteration ").append(iteration)
                    .append(": readerAlive=").append(alive)
                    .append(" stack=").append(topFrames(reader)).append('\n')
                line("A-ARMED", "iteration=" + iteration + " alive=" + alive)
                if (!alive) {
                    report.append("  the reader was not blocked, so this iteration says nothing\n")
                    skipped++
                    closeIteration(session, owner, heldExtras, heldSessions, heldOwners)
                    continue
                }
                var openedExtras = 0
                repeat(extraDescriptors) { index ->
                    val extra = manager.openAccessory(accessory)
                    if (extra == null) {
                        report.append("  extra descriptor ").append(index + 1)
                            .append(" could not be opened\n")
                    } else {
                        heldExtras.add(extra)
                        openedExtras++
                    }
                }
                if (extraDescriptors > 0) {
                    report.append("  holding ").append(openedExtras).append(" extra descriptor(s)")
                        .append(" alongside the blocked reader\n")
                    line("EXTRAS", "held=" + openedExtras)
                }
                if (secondSession && heldExtras.isNotEmpty()) {
                    val extraOwner = PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = {})
                    val extraErrors = Collections.synchronizedList(mutableListOf<String>())
                    val extraSession = AccessorySession(
                        heldExtras.last(), {}, { reason -> extraErrors.add(reason) }, extraOwner, {}, {},
                    )
                    extraSession.start()
                    heldOwners.add(extraOwner)
                    heldSessions.add(extraSession)
                    Thread.sleep(400)
                    report.append("  second session on the extra descriptor, its errors=")
                        .append(snapshot(extraErrors)).append('\n')
                    line("SECOND-SESSION", "errors=" + snapshot(extraErrors))
                }
                line("READY-FOR-HOST-FRAME", "iteration=" + iteration)
                report.append("READY-FOR-HOST-FRAME (iteration ").append(iteration).append(")\n")
                val iterationDeadline = System.currentTimeMillis() + 4_500
                while (System.currentTimeMillis() < iterationDeadline) {
                    // The session's own answer, not a thread found by name: the name is shared by
                    // every reader this instrument arms, so the previous iteration's reader could
                    // have been the one inspected (independent review, 2026-10-25, ID-002).
                    if (!session.readerAlive() || snapshot(errors).isNotEmpty()) {
                        answered = snapshot(errors).joinToString("; ")
                        break
                    }
                    Thread.sleep(50)
                }
                val reason = if (answered != null) answered else "still blocked after 4.5 s"
                report.append("  iteration ").append(iteration).append(" ended: ").append(reason)
                    .append("\n")
                line("ITERATION", "iteration=" + iteration + " ended=" + reason)
                report.append("  frames this iteration read: ").append(snapshot(framesThisIteration))
                    .append('\n')
                // Only a reason the *session* produces for a frame counts as consumption. A
                // deadline is the idle-endpoint control, and EIO is the driver refusing this
                // reader outright - counting either as "consumed" made the first version of this
                // instrument report a false positive.
                // Both halves are required now: the session refused a frame *and* the frame it
                // read is the one the host wrote.
                val fromHost = snapshot(framesThisIteration).any { it == describeFrameOf(hostFrameSequence) }
                if (!fromHost) {
                    report.append("  no frame matching the host's (type=0x0e sequence=")
                        .append(hostFrameSequence).append(") was read in this iteration\n")
                }
                if (answered != null && answered.contains("deadline expired")) {
                    deadlineEnded++
                    answered = null
                } else if (answered != null && !fromHost) {
                    otherEnded++
                    answered = null
                }
                // A clean reader for the next iteration: leaving this one armed is what could
                // manufacture the two-reader state this instrument is supposed to control.
                if (answered == null) {
                    closeIteration(session, owner, heldExtras, heldSessions, heldOwners)
                }
            }
            if (answered != null) {
                report.append("outcome: the blocked reader consumed the host's frame — ").append(answered)
                    .append(" (after ").append(iteration).append(" iteration(s))\n")
                line("OUTCOME", "consumed=true reason=" + answered + " iterations=" + iteration)
            } else {
                report.append("FALSIFIED: no host frame reached the blocked reader in ")
                    .append(iteration).append(" iteration(s)")
                    .append(" (ended by deadline: ").append(deadlineEnded)
                    .append(", skipped: ").append(skipped)
                    .append(", ended for another reason: ").append(otherEnded).append(")")
                    .append("; frames read: ").append(snapshot(framesRead)).append('\n')
                line("OUTCOME", "consumed=false iterations=" + iteration
                    + " deadline=" + deadlineEnded + " skipped=" + skipped + " other=" + otherEnded)
            }
        } finally {
            heldOwners.forEach { runCatching { it.close() } }
            heldSessions.forEach { runCatching { it.close() } }
            held.forEach { runCatching { it.close() } }
            heldExtras.forEach { runCatching { it.close() } }
        }
        return done(report)
    }

    /** The host half does the AOA switch, so the accessory may not exist yet. */
    private fun awaitAccessory(manager: UsbManager, report: StringBuilder): UsbAccessory? {
        val started = System.currentTimeMillis()
        while (System.currentTimeMillis() - started < waitForAccessoryMs) {
            manager.accessoryList?.firstOrNull()?.let { return it }
            if (System.currentTimeMillis() - started > 5_000) {
                report.append("waiting for the accessory (the host switches the phone): ")
                    .append(System.currentTimeMillis() - started).append(" ms\n")
                line("WAITING-FOR-ACCESSORY", "ms=" + (System.currentTimeMillis() - started))
                return waitForAccessory(manager, started, report)
            }
            Thread.sleep(500)
        }
        report.append("FAIL usb-frame-owner: no accessory appeared within ")
            .append(waitForAccessoryMs).append(" ms\n")
        return null
    }

    private fun waitForAccessory(manager: UsbManager, started: Long, report: StringBuilder): UsbAccessory? {
        var lastLog = System.currentTimeMillis()
        while (System.currentTimeMillis() - started < waitForAccessoryMs) {
            manager.accessoryList?.firstOrNull()?.let { return it }
            if (System.currentTimeMillis() - lastLog > 10_000) {
                lastLog = System.currentTimeMillis()
                line("WAITING-FOR-ACCESSORY", "ms=" + (System.currentTimeMillis() - started))
            }
            Thread.sleep(500)
        }
        report.append("FAIL usb-frame-owner: no accessory appeared within ")
            .append(waitForAccessoryMs).append(" ms\n")
        return null
    }

    private fun describeFrame(header: Wire.Header): String =
        "type=0x%02x sequence=%d len=%d".format(header.type, header.sequence, header.payloadLength)

    private fun describeFrameOf(sequence: Int): String =
        "type=0x%02x sequence=%d len=0".format(Wire.TYPE_HOST_HELLO, sequence)

    /**
     * Take one iteration's reader off the endpoint before the next one arms.
     *
     * The first version left each session armed until the outer `finally`, so an iteration could
     * open while the previous reader still lived - manufacturing exactly the two-reader state this
     * instrument is meant to control for (independent review, 2026-10-25, ID-002).
     */
    private fun closeIteration(
        session: AccessorySession,
        owner: PhoneAuthorization,
        heldExtras: MutableList<android.os.ParcelFileDescriptor>,
        heldSessions: MutableList<AccessorySession>,
        heldOwners: MutableList<PhoneAuthorization>,
    ) {
        runCatching { session.close() }
        heldSessions.remove(session)
        heldOwners.remove(owner)
        runCatching { owner.close() }
        heldExtras.forEach { runCatching { it.close() } }
        heldExtras.clear()
    }

    private fun report(text: String) = StringBuilder(text)

    private fun snapshot(list: MutableList<String>): List<String> = synchronized(list) { list.toList() }

    private fun done(report: StringBuilder): String {
        logReport(report)
        return report.toString()
    }

    private fun logReport(report: StringBuilder) {
        report.toString().split('\n').filter { it.isNotBlank() }.forEach { Log.i(tag, "REPORT " + it) }
    }

    private fun line(marker: String, detail: String) {
        Log.i(tag, marker + " " + detail)
    }

    private fun topFrames(thread: Thread?): String =
        thread?.stackTrace?.take(2)?.joinToString(" <- ") { it.className + "." + it.methodName }
            ?: "no thread"

    private fun threadNamed(): Thread? =
        Thread.getAllStackTraces().keys.firstOrNull { it.name == "amb-accessory-reader" && it.isAlive }
}
