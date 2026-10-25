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
    /**
     * The host frame's fresh `ptsUs`; -1 when the caller did not supply one.
     *
     * Type and sequence alone are not attribution: they are constant across runs, so an earlier
     * identical write satisfies the same comparison, and a reviewer showed that a frame with a
     * different `ptsUs` passed it too (independent review, 2026-10-25, second pass, R2-001). The host
     * script stamps `ptsUs` with the current time and prints it; `-e hostPts <value>` carries it here
     * and the verdict requires it. Without it the verdict is weaker, and the report says so.
     */
    private val expectedPts: Long = -1L,
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

        // Each armed iteration owns its session, its owner, its frames and its descriptors, and takes
        // its own reader off the endpoint. The first version threaded all of that through the loop and
        // a five-parameter helper, which is the shape in which an iteration outlives its own cleanup
        // (independent review, 2026-10-25, ID-002).
        val armed = mutableListOf<Iteration>()
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
            var attempt = 0
            var answered: String? = null
            var deadlineEnded = 0
            var skipped = 0
            var otherEnded = 0
            while (System.currentTimeMillis() < deadline && answered == null) {
                attempt++
                val pfd = manager.openAccessory(accessory)
                if (pfd == null) {
                    report.append("FAIL usb-frame-owner: openAccessory returned null on iteration ")
                        .append(attempt).append('\n')
                    break
                }
                val iteration = Iteration(manager, accessory, pfd, extraDescriptors, secondSession, ::line)
                armed.add(iteration)
                iteration.session.start()
                Thread.sleep(200)
                val alive = iteration.alive
                val reader = threadNamed()
                report.append("iteration ").append(attempt)
                    .append(": readerAlive=").append(alive)
                    .append(" stack=").append(topFrames(reader)).append('\n')
                line("A-ARMED", "iteration=" + attempt + " alive=" + alive)
                if (!alive) {
                    report.append("  the reader was not blocked, so this iteration says nothing\n")
                    skipped++
                    iteration.close()
                    continue
                }
                report.append(iteration.armExtras())
                line("READY-FOR-HOST-FRAME", "iteration=" + attempt)
                report.append("READY-FOR-HOST-FRAME (iteration ").append(attempt).append(")\n")
                val iterationDeadline = System.currentTimeMillis() + 4_500
                while (System.currentTimeMillis() < iterationDeadline) {
                    // The session's own answer, not a thread found by name: the name is shared by
                    // every reader this instrument arms, so the previous iteration's reader could
                    // have been the one inspected (independent review, 2026-10-25, ID-002).
                    if (!iteration.alive || snapshot(iteration.errors).isNotEmpty()) {
                        answered = snapshot(iteration.errors).joinToString("; ")
                        break
                    }
                    Thread.sleep(50)
                }
                val reason = if (answered != null) answered else "still blocked after 4.5 s"
                report.append("  iteration ").append(attempt).append(" ended: ").append(reason)
                    .append("\n")
                line("ITERATION", "iteration=" + attempt + " ended=" + reason)
                val readNow = snapshot(iteration.frames)
                report.append("  frames this iteration read: ")
                    .append(readNow.map { describeFrame(it) }).append('\n')
                // Only a reason the *session* produces for a frame counts as consumption. A
                // deadline is the idle-endpoint control, and EIO is the driver refusing this
                // reader outright - counting either as "consumed" made the first version of this
                // instrument report a false positive.
                // Both halves are required now: the session refused a frame *and* the frame it
                // read is the one the host wrote.
                // Compared as fields, not as the text a formatter produces: the verdict is about the
                // frame, and a message-shaped comparison would rot silently if the format changed.
                val fromHost = readNow.any { header ->
                    header.type == Wire.TYPE_HOST_HELLO && header.sequence == hostFrameSequence &&
                        (expectedPts < 0 || header.ptsUs == expectedPts)
                }
                if (!fromHost) {
                    report.append("  no frame matching the host's (type=0x0e sequence=")
                        .append(hostFrameSequence)
                    if (expectedPts >= 0) {
                        report.append(" ptsUs=").append(expectedPts)
                    } else {
                        report.append(" and any pts, because -e hostPts was not supplied: the frame's")
                            .append(" own pts is printed above for cross-checking with the host's log")
                    }
                    report.append(") was read in this iteration\n")
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
                    if (!iteration.close()) {
                        report.append("VOID: this iteration's reader survived its close, so a later one")
                            .append(" would share the endpoint with it; the run cannot speak about a")
                            .append(" single reader\n")
                        line("VOID", "reader survived close")
                        return done(report)
                    }
                }
            }
            if (answered != null) {
                report.append("outcome: the blocked reader consumed the host's frame — ").append(answered)
                    .append(" (after ").append(attempt).append(" iteration(s))\n")
                line("OUTCOME", "consumed=true reason=" + answered + " iterations=" + attempt)
            } else {
                report.append("FALSIFIED: no host frame reached the blocked reader in ")
                    .append(attempt).append(" iteration(s)")
                    .append(" (ended by deadline: ").append(deadlineEnded)
                    .append(", skipped: ").append(skipped)
                    .append(", ended for another reason: ").append(otherEnded).append(")")
                    .append("; frames read: ").append(
                        armed.flatMapIndexed { index, one ->
                            snapshot(one.frames).map { "iteration " + (index + 1) + " " + describeFrame(it) }
                        },
                    ).append('\n')
                line("OUTCOME", "consumed=false iterations=" + attempt
                    + " deadline=" + deadlineEnded + " skipped=" + skipped + " other=" + otherEnded)
            }
        } finally {
            armed.forEach { runCatching { it.close() } }
        }
        return done(report)
    }

    /**
     * The host half does the AOA switch, so the accessory may not exist yet.
     *
     * One loop, not two: the first version returned into a second waiting function after its first log
     * line, a split with no second responsibility in it.
     */
    private fun awaitAccessory(manager: UsbManager, report: StringBuilder): UsbAccessory? {
        val started = System.currentTimeMillis()
        var lastLog = started
        while (System.currentTimeMillis() - started < waitForAccessoryMs) {
            manager.accessoryList?.firstOrNull()?.let { return it }
            val waited = System.currentTimeMillis() - started
            if (waited > 5_000 && System.currentTimeMillis() - lastLog > 10_000) {
                lastLog = System.currentTimeMillis()
                report.append("waiting for the accessory (the host switches the phone): ")
                    .append(waited).append(" ms\n")
                line("WAITING-FOR-ACCESSORY", "ms=" + waited)
            }
            Thread.sleep(500)
        }
        report.append("FAIL usb-frame-owner: no accessory appeared within ")
            .append(waitForAccessoryMs).append(" ms\n")
        return null
    }

    /**
     * What one armed iteration owns: its session, that session's owner, the frames it read, and the
     * descriptors it holds - plus the ability to take its own reader off the endpoint.
     *
     * The loop used to thread all of that through the caller and a five-parameter helper, and that
     * shape is what let an iteration outlive its own cleanup (independent review, 2026-10-25, ID-002).
     */
    private class Iteration(
        private val manager: UsbManager,
        private val accessory: UsbAccessory,
        private val pfd: android.os.ParcelFileDescriptor,
        private val extraDescriptors: Int,
        private val secondSession: Boolean,
        private val log: (String, String) -> Unit,
    ) {
        val errors = Collections.synchronizedList(mutableListOf<String>())
        val frames = Collections.synchronizedList(mutableListOf<Wire.Header>())
        private val descriptors = mutableListOf(pfd)
        private val owners = mutableListOf<PhoneAuthorization>()
        private val sessions = mutableListOf<AccessorySession>()

        /** The seam: every host frame this session reads is reported, so the verdict can name it. */
        val session = AccessorySession(
            pfd, {}, { reason -> errors.add(reason) },
            PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = {}).also { owners.add(it) },
            {}, {},
            onFrameRead = { header -> frames.add(header) },
        )

        init {
            sessions.add(session)
        }

        val alive: Boolean get() = session.readerAlive()

        /** Open the extra descriptors, and optionally a second session on one; returns the report lines. */
        fun armExtras(): String {
            val text = StringBuilder()
            var opened = 0
            repeat(extraDescriptors) { index ->
                val extra = manager.openAccessory(accessory)
                if (extra == null) {
                    text.append("  extra descriptor ").append(index + 1).append(" could not be opened\n")
                } else {
                    descriptors.add(extra)
                    opened++
                }
            }
            if (extraDescriptors > 0) {
                text.append("  holding ").append(opened).append(" extra descriptor(s)")
                    .append(" alongside the blocked reader\n")
                log("EXTRAS", "held=" + opened)
            }
            if (secondSession && descriptors.size > 1) {
                val extraOwner = PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = {})
                val extraErrors = Collections.synchronizedList(mutableListOf<String>())
                val extraSession = AccessorySession(
                    descriptors.last(), {}, { reason -> extraErrors.add(reason) }, extraOwner, {}, {},
                )
                extraSession.start()
                owners.add(extraOwner)
                sessions.add(extraSession)
                Thread.sleep(400)
                val reported = synchronized(extraErrors) { extraErrors.toList() }
                text.append("  second session on the extra descriptor, its errors=")
                    .append(reported).append('\n')
                log("SECOND-SESSION", "errors=" + reported)
            }
            return text.toString()
        }

        /**
         * Take this iteration's reader off the endpoint, and say whether it really is off.
         *
         * `AccessorySession.close()` discards `closeAndAwait`'s answer, and `closeAndAwait` keeps the
         * handle when the reader survived precisely so a caller can ask. Discarding it would let the
         * next iteration arm beside a live reader - the two-reader state this instrument exists to rule
         * out (independent review, 2026-10-25, second pass, R2-002).
         */
        fun close(): Boolean {
            val stopped = runCatching { session.closeAndAwait(500) }.getOrDefault(false)
            owners.forEach { runCatching { it.close() } }
            descriptors.forEach { runCatching { it.close() } }
            descriptors.clear()
            return stopped
        }
    }

    private fun describeFrame(header: Wire.Header): String =
        "type=0x%02x sequence=%d len=%d pts=%d".format(
            header.type, header.sequence, header.payloadLength, header.ptsUs,
        )

    private fun report(text: String) = StringBuilder(text)

    private fun <T> snapshot(list: MutableList<T>): List<T> = synchronized(list) { list.toList() }

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
