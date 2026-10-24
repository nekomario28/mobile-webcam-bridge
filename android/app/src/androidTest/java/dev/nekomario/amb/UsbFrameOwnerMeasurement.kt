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
class UsbFrameOwnerMeasurement(private val instrumentation: Instrumentation) {
    private val controlMs = 3_000L
    private val waitForHostMs = 25_000L
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

        val newErrors = Collections.synchronizedList(mutableListOf<String>())
        var newPfd: android.os.ParcelFileDescriptor? = null
        var newOwner: PhoneAuthorization? = null
        val held = mutableListOf<android.os.ParcelFileDescriptor>()
        val heldOwners = mutableListOf<PhoneAuthorization>()
        val heldSessions = mutableListOf<AccessorySession>()
        try {
            // The "new" session first: its reader is refused by the driver (measured separately
            // as EIO), so the endpoint stays with whatever blocked reader came before it.
            newPfd = manager.openAccessory(accessory)
            newOwner = PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = {})
            if (newPfd != null) {
                AccessorySession(newPfd, {}, { reason -> newErrors.add(reason) }, newOwner, {}, {}).start()
                Thread.sleep(400)
                report.append("new session started on an earlier descriptor, its errors=")
                    .append(snapshot(newErrors)).append('\n')
                line("B-STARTED", "errors=" + snapshot(newErrors))
            }

            // A proof deadline is absolute and 5 s long, so a blocked reader cannot be held for
            // ever in PROOF. Rather than race it, arm a fresh blocked reader repeatedly: an
            // iteration that ends by deadline is itself the control (nothing arrives on an idle
            // endpoint), and the frame shows up in whichever iteration it lands in.
            val deadline = System.currentTimeMillis() + waitForHostMs
            var iteration = 0
            var answered: String? = null
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
                val session = AccessorySession(pfd, {}, { reason -> errors.add(reason) }, owner, {}, {})
                held.add(pfd); heldOwners.add(owner); heldSessions.add(session)
                session.start()
                Thread.sleep(200)
                val reader = threadNamed()
                report.append("iteration ").append(iteration)
                    .append(": readerAlive=").append(reader?.isAlive == true)
                    .append(" stack=").append(topFrames(reader)).append('\n')
                line("A-ARMED", "iteration=" + iteration + " alive=" + (reader?.isAlive == true))
                if (reader?.isAlive != true) {
                    report.append("  the reader was not blocked, so this iteration says nothing\n")
                    continue
                }
                line("READY-FOR-HOST-FRAME", "iteration=" + iteration)
                report.append("READY-FOR-HOST-FRAME (iteration ").append(iteration).append(")\n")
                val iterationDeadline = System.currentTimeMillis() + 4_500
                while (System.currentTimeMillis() < iterationDeadline) {
                    if (reader.isAlive.not() || snapshot(errors).isNotEmpty()) {
                        answered = snapshot(errors).joinToString("; ")
                        break
                    }
                    Thread.sleep(50)
                }
                val reason = if (answered != null) answered else "still blocked after 4.5 s"
                report.append("  iteration ").append(iteration).append(" ended: ").append(reason)
                    .append("\n")
                line("ITERATION", "iteration=" + iteration + " ended=" + reason)
                // Only a reason the *session* produces for a frame counts as consumption. A
                // deadline is the idle-endpoint control, and EIO is the driver refusing this
                // reader outright - counting either as "consumed" made the first version of this
                // instrument report a false positive.
                answered = answered?.takeIf { it.contains("proof rejected") }
            }
            if (answered != null) {
                report.append("outcome: the blocked reader consumed the host's frame — ").append(answered)
                    .append(" (after ").append(iteration).append(" iteration(s))\n")
                line("OUTCOME", "consumed=true reason=" + answered + " iterations=" + iteration)
            } else {
                report.append("FALSIFIED: no host frame reached the blocked reader in ")
                    .append(iteration).append(" iteration(s); every one ended by its own proof")
                    .append(" deadline, which is the control\n")
                line("OUTCOME", "consumed=false iterations=" + iteration)
            }
            report.append("  new session errors at the end: ").append(snapshot(newErrors)).append('\n')
        } finally {
            heldOwners.forEach { runCatching { it.close() } }
            heldSessions.forEach { runCatching { it.close() } }
            held.forEach { runCatching { it.close() } }
            runCatching { newOwner?.close() }
            runCatching { newPfd?.close() }
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
