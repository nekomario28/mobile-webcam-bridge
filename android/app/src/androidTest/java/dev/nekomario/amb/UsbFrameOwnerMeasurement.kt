package dev.nekomario.amb

import android.app.Instrumentation
import android.hardware.usb.UsbAccessory
import android.hardware.usb.UsbManager
import android.os.Build
import android.util.Log
import java.util.Collections

/**
 * Asks who consumes a frame the host sends while an *old* reader holds the endpoint.
 *
 * The single-reader guard exists because of a state the repository has only ever seen from the
 * host side: an old accessory reader stays blocked on the endpoint, a new session opens on top
 * of it, and the host's bytes go somewhere other than the session it thinks it is talking to.
 * `docs/evidence/2026-10-25-usb-reader-release.md` measured the *price* of the guard (closing
 * the descriptor releases the reader, and two descriptors both open) but not this, because it
 * needs a host that writes.
 *
 * So this leaves the endpoint idle for a control window, announces itself, and then waits for
 * the host to write exactly one frame (see `host-rs/tools/send-hello.py`). The two outcomes are
 * distinguishable without trusting either side's narrative:
 *
 * - **the old session consumes it** — its reader leaves the read and the session ends, with the
 *   reason `PhoneAuthorization` gives for a frame that is not a proof ("PC authorization proof
 *   rejected"). That string is the ID-001 path: a control frame arriving at a session still
 *   waiting for its proof is refused *there*, and ends the session.
 * - **nothing consumes it** — the old reader is still blocked when the wait expires, which is
 *   the falsification of the premise, and the report says so.
 *
 * The control window is inside the run rather than assumed: the old reader must still be blocked
 * after the second session opens and after three seconds of silence, or the measurement is void
 * and says so.
 *
 * No app state is touched: the authorization owner is an in-memory repository.
 *
 * **Never run.** On 2026-10-25 the host could not claim the accessory interface while adb was
 * attached, and the device side needs adb to be driven and to report — so the pair has no
 * configuration in which both halves work yet. The evidence, and what would unblock it, is in
 * `docs/evidence/2026-10-25-usb-frame-owner-blocked.md`; nothing here is a measured claim.
 */
class UsbFrameOwnerMeasurement(private val instrumentation: Instrumentation) {
    private val oldSessionIndex = "A"
    private val newSessionIndex = "B"
    private val controlMs = 3_000L
    private val waitForHostMs = 25_000L

    private val tag = "UsbFrameOwner"

    fun run(): String {
        val manager = instrumentation.targetContext.getSystemService(UsbManager::class.java)
            ?: return "FAIL usb-frame-owner: no UsbManager\n"
        val accessory = manager.accessoryList?.firstOrNull()
            ?: return "FAIL usb-frame-owner: no accessory is attached (AOA not active)\n"
        val report = StringBuilder()
        report.append("usb frame-owner measurement on ").append(Build.MODEL)
            .append(" (API ").append(Build.VERSION.SDK_INT).append("), accessory ")
            .append(accessory.manufacturer).append(' ').append(accessory.model).append('\n')
        if (!manager.hasPermission(accessory)) {
            report.append("FAIL usb-frame-owner: this app holds no USB permission for the accessory\n")
            return report.toString()
        }

        val oldErrors = Collections.synchronizedList(mutableListOf<String>())
        val newErrors = Collections.synchronizedList(mutableListOf<String>())
        val oldPfd = manager.openAccessory(accessory)
        if (oldPfd == null) {
            report.append("FAIL usb-frame-owner: openAccessory returned null for the old session\n")
            return report.toString()
        }
        val oldOwner = PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = {})
        val oldSession = AccessorySession(
            oldPfd, {}, { reason -> oldErrors.add(reason) }, oldOwner, {}, {},
        )
        var newPfd: android.os.ParcelFileDescriptor? = null
        var newOwner: PhoneAuthorization? = null
        try {
            oldSession.start()
            Thread.sleep(300)
            // Resolved while only the old session exists, so this reference is the old reader and
            // not whichever thread happens to share its name later.
            val oldReader = threadNamed("amb-accessory-reader")
            report.append("old session started, reader alive=").append(oldReader?.isAlive == true)
                .append(" stack=").append(topFrames(oldReader)).append('\n')
            line("A-BLOCKED", "alive=" + (oldReader?.isAlive == true) + " " + topFrames(oldReader))

            newPfd = manager.openAccessory(accessory)
            newOwner = PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = {})
            if (newPfd != null) {
                val newSession = AccessorySession(
                    newPfd, {}, { reason -> newErrors.add(reason) }, newOwner, {}, {},
                )
                newSession.start()
                Thread.sleep(400)
                report.append("new session started on a second descriptor, its errors=")
                    .append(synchronized(newErrors) { newErrors.toList() }).append('\n')
                line("B-STARTED", "errors=" + synchronized(newErrors) { newErrors.toList() })
            } else {
                report.append("second openAccessory returned null, so no second session was started\n")
                line("B-NOT-OPENED", "")
            }

            // Control window: silence, and the old reader must still be blocked at the end of it.
            Thread.sleep(controlMs)
            val controlAlive = oldReader?.isAlive == true
            val controlErrors = synchronized(oldErrors) { oldErrors.toList() }
            report.append("control window (").append(controlMs).append(" ms, no host traffic): oldReaderAlive=")
                .append(controlAlive).append(" oldErrors=").append(controlErrors).append('\n')
            line("CONTROL", "oldReaderAlive=" + controlAlive + " errors=" + controlErrors)
            if (!controlAlive || controlErrors.isNotEmpty()) {
                report.append("VOID: the old session was already ending without any host traffic,")
                    .append(" so this run cannot attribute anything to the host\n")
                line("VOID", "the old session did not survive the control window")
                return report.toString()
            }

            line("READY-FOR-HOST-FRAME", "waiting up to " + waitForHostMs + " ms")
            report.append("READY-FOR-HOST-FRAME: waiting up to ").append(waitForHostMs).append(" ms\n")
            val started = System.currentTimeMillis()
            var consumed = false
            while (System.currentTimeMillis() - started < waitForHostMs) {
                if (oldReader?.isAlive != true || synchronized(oldErrors) { oldErrors.isNotEmpty() }) {
                    consumed = true
                    break
                }
                Thread.sleep(100)
            }
            val elapsed = System.currentTimeMillis() - started
            val reasons = synchronized(oldErrors) { oldErrors.toList() }
            report.append("outcome: oldReaderConsumedTheFrame=").append(consumed)
                .append(" after ").append(elapsed).append(" ms")
                .append(", oldReaderAlive=").append(oldReader?.isAlive == true)
                .append(", oldErrors=").append(reasons).append('\n')
            report.append("  new session errors at the end: ")
                .append(synchronized(newErrors) { newErrors.toList() }).append('\n')
            line("OUTCOME", "consumed=" + consumed + " ms=" + elapsed + " errors=" + reasons)
            if (!consumed) {
                report.append("FALSIFIED: nothing consumed the frame the host wrote — the old reader")
                    .append(" is still blocked, so the premise is not established here\n")
            }
        } finally {
            runCatching { oldOwner.close() }
            runCatching { oldSession.close() }
            runCatching { newOwner?.close() }
            runCatching { newPfd?.close() }
        }
        return report.toString()
    }

    private fun line(marker: String, detail: String) {
        Log.i(tag, marker + " " + detail)
    }

    private fun topFrames(thread: Thread?): String =
        thread?.stackTrace?.take(2)?.joinToString(" <- ") { it.className + "." + it.methodName }
            ?: "no thread"

    private fun threadNamed(name: String): Thread? =
        Thread.getAllStackTraces().keys.firstOrNull { it.name == name && it.isAlive }
}
