package dev.nekomario.amb

import android.app.Instrumentation
import android.hardware.usb.UsbAccessory
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.ParcelFileDescriptor
import java.io.FileInputStream
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * Measures, on a real endpoint, the mechanism the single-reader guard rests on.
 *
 * `AccessorySession.closeAndAwait` closes the descriptor and waits, because a blocked read on
 * the accessory stream is not interruptible. Everything the repository said about that sentence
 * was *read* — the security-coverage row says so, and the guard it justifies was therefore
 * insurance nobody had priced. This harness prices it, and it asserts the **precondition** on
 * every iteration rather than assuming it: a read that was not blocked when the descriptor
 * closed proves nothing about unblocking one, so those iterations are reported and excluded
 * from the rate instead of counted as successes.
 *
 * Three measurements, in the order they depend on each other:
 *
 * 1. **A raw blocked read** on one descriptor: open, read, confirm it is still blocked, close,
 *    and see whether the read returns and how fast. No app class is involved, so this is the
 *    platform's behaviour rather than any build's.
 * 2. **A real session**, `AccessorySession.start()` then `closeAndAwait`, with the reader
 *    thread found by name and its stack recorded, so "the reader was blocked in the read when
 *    the close happened" is evidence rather than assumption.
 * 3. **Two descriptors on one accessory**: what a second reader gets, and what closing one
 *    descriptor does to the other's blocked read. If the platform refused the second open, the
 *    app-level guard would be redundant.
 *
 * What it does not measure, and cannot from this side: a *host* driving frames while two
 * readers are open — the mode the intermittent `BAD_SIGNATURE` and "expected authorization
 * outcome before capabilities/media" failures were seen in. A clean release rate is not
 * evidence that the guard is unnecessary; it is evidence about which half of the failure is
 * cheap to reproduce and which needs a host.
 *
 * No app state is touched: the authorization owner is an in-memory repository.
 */
class UsbReaderReleaseMeasurement(private val instrumentation: Instrumentation) {
    private val rawIterations = 10
    private val sessionIterations = 10
    private val settleMs = 300L
    private val waitMs = 1_500L

    fun run(): String {
        val manager = instrumentation.targetContext.getSystemService(UsbManager::class.java)
            ?: return "FAIL usb-reader: no UsbManager\n"
        val accessory = manager.accessoryList?.firstOrNull()
            ?: return "FAIL usb-reader: no accessory is attached (AOA not active)\n"
        val report = StringBuilder()
        report.append("usb-reader measurement on ").append(Build.MODEL)
            .append(" (API ").append(Build.VERSION.SDK_INT).append("), accessory ")
            .append(accessory.manufacturer).append(' ').append(accessory.model).append('\n')
        if (!manager.hasPermission(accessory)) {
            report.append("no USB permission for this accessory: asking the device now\n")
            val started = System.currentTimeMillis()
            if (!awaitPermission(manager, accessory, 180_000L)) {
                report.append("FAIL usb-reader: permission was not granted in 180 s")
                    .append(" — answer the dialog on the phone and run it again\n")
                return report.toString()
            }
            report.append("permission granted after ")
                .append(System.currentTimeMillis() - started).append(" ms\n")
        }
        rawRelease(manager, accessory, report)
        sessionRelease(manager, accessory, report)
        twoDescriptors(manager, accessory, report)
        return report.toString()
    }

    private fun rawRelease(manager: UsbManager, accessory: UsbAccessory, report: StringBuilder) {
        var blockedBeforeClose = 0
        var released = 0
        var slowest = 0L
        for (iteration in 1..rawIterations) {
            val pfd = manager.openAccessory(accessory)
            if (pfd == null) {
                report.append("FAIL usb-reader: openAccessory returned null (no USB permission)\n")
                break
            }
            val reader = BlockingReader(pfd)
            reader.start()
            Thread.sleep(settleMs)
            // Both halves: the read was entered, and it had not returned when the descriptor closed.
            val blocked = reader.awaitEntered(1_000) && reader.outcome == "blocked"
            val started = System.nanoTime()
            runCatching { pfd.close() }
            val returned = reader.await(waitMs)
            val elapsed = (System.nanoTime() - started) / 1_000_000
            if (blocked) {
                blockedBeforeClose++
                if (returned) {
                    released++
                    slowest = maxOf(slowest, elapsed)
                }
            }
            report.append("  raw ").append(iteration).append(": blockedBeforeClose=").append(blocked)
                .append(" afterClose=").append(reader.outcome)
                .append(" returned=").append(returned).append(" in ").append(elapsed).append(" ms\n")
        }
        report.append("raw release: ").append(released).append('/').append(blockedBeforeClose)
            .append(" blocked reads returned once the descriptor closed inside ").append(waitMs)
            .append(" ms\n")
        if (released > 0) report.append("raw slowest: ").append(slowest).append(" ms\n")
    }

    private fun sessionRelease(manager: UsbManager, accessory: UsbAccessory, report: StringBuilder) {
        var blockedBeforeClose = 0
        var released = 0
        var slowest = 0L
        for (iteration in 1..sessionIterations) {
            val pfd = manager.openAccessory(accessory)
            if (pfd == null) {
                report.append("FAIL usb-reader: openAccessory returned null (no USB permission)\n")
                break
            }
            val errors = Collections.synchronizedList(mutableListOf<String>())
            val owner = PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = {})
            val session = AccessorySession(
                pfd, {}, { reason -> errors.add(reason) }, owner, {}, {},
            )
            try {
                session.start()
                Thread.sleep(settleMs)
                val reader = threadNamed("amb-accessory-reader")
                val blocked = reader != null && reader.isAlive
                val top = reader?.stackTrace?.take(2)
                    ?.joinToString(" <- ") { "${it.className}.${it.methodName}" } ?: "no thread"
                val errorsBefore = synchronized(errors) { errors.toList() }
                val started = System.nanoTime()
                val stopped = session.closeAndAwait(waitMs)
                val elapsed = (System.nanoTime() - started) / 1_000_000
                if (blocked && errorsBefore.isEmpty()) {
                    blockedBeforeClose++
                    if (stopped) {
                        released++
                        slowest = maxOf(slowest, elapsed)
                    }
                }
                val after = threadNamed("amb-accessory-reader")?.isAlive == true
                report.append("  session ").append(iteration)
                    .append(": readerAliveBefore=").append(blocked)
                    .append(" errorsBefore=").append(errorsBefore.size)
                    .append(" released=").append(stopped).append(" in ").append(elapsed)
                    .append(" ms readerStillAliveAfter=").append(after).append('\n')
                report.append("      reader stack: ").append(top).append('\n')
            } finally {
                runCatching { owner.close() }
            }
        }
        report.append("session release: ").append(released).append('/').append(blockedBeforeClose)
            .append(" blocked readers came back once the close ran inside ").append(waitMs)
            .append(" ms\n")
        if (released > 0) report.append("session slowest: ").append(slowest).append(" ms\n")
    }

    private fun twoDescriptors(manager: UsbManager, accessory: UsbAccessory, report: StringBuilder) {
        val first = manager.openAccessory(accessory)
        val second = manager.openAccessory(accessory)
        report.append("two descriptors on one accessory: first=").append(first != null)
            .append(" second=").append(second != null).append('\n')
        if (first == null || second == null) {
            runCatching { first?.close() }
            runCatching { second?.close() }
            return
        }
        val reader1 = BlockingReader(first)
        val reader2 = BlockingReader(second)
        reader1.start()
        reader2.start()
        reader1.awaitEntered(1_000)
        Thread.sleep(400)
        report.append("  both open, no host: reader1=").append(reader1.outcome)
            .append(" reader2=").append(reader2.outcome).append('\n')
        runCatching { first.close() }
        Thread.sleep(400)
        report.append("  after closing descriptor 1: reader1=").append(reader1.outcome)
            .append(" reader2=").append(reader2.outcome).append('\n')
        runCatching { second.close() }
        Thread.sleep(400)
        report.append("  after closing descriptor 2: reader1=").append(reader1.outcome)
            .append(" reader2=").append(reader2.outcome).append('\n')
    }

    /**
     * Ask the device for the accessory and wait for the owner to answer the dialog.
     *
     * The request goes through the app's own [MainActivity.createUsbPermissionRequest], so the
     * instrument exercises the same path the product does. Polling `hasPermission` rather than
     * reading the broadcast's extra keeps this independent of what the system fills in.
     *
     * **Not exercised by the runs recorded in `docs/evidence/2026-10-25-usb-reader-release.md`:**
     * the USB grant survived `adb install -r` at the same versionCode, so `hasPermission` was
     * already true and this path never ran. It is here for a device where the grant is absent,
     * and it is untested until a run reports answering that dialog.
     */
    private fun awaitPermission(manager: UsbManager, accessory: UsbAccessory, timeoutMs: Long): Boolean {
        val context = instrumentation.targetContext
        val started = System.currentTimeMillis()
        runCatching {
            manager.requestPermission(
                accessory,
                MainActivity.createUsbPermissionRequest(context, "usb-reader-measurement"),
            )
        }
        while (System.currentTimeMillis() - started < timeoutMs) {
            if (manager.hasPermission(accessory)) return true
            Thread.sleep(500)
        }
        return manager.hasPermission(accessory)
    }

    private fun threadNamed(name: String): Thread? =
        Thread.getAllStackTraces().keys.firstOrNull { it.name == name && it.isAlive }

    /** A raw blocking read on one descriptor, so the outcome is not a session's opinion. */
    private class BlockingReader(pfd: ParcelFileDescriptor) {
        private val stream = runCatching { FileInputStream(pfd.fileDescriptor) }.getOrNull()
        private val finished = CountDownLatch(1)

        /**
         * Set immediately before the read, so "blocked" means "inside the read" rather than
         * "constructed and probably scheduled by now". The first version of this instrument
         * initialised the outcome to "blocked" and counted a 300 ms sleep as proof, which is a
         * verdict that does not prove it measured anything (independent review, 2026-10-25, ID-003).
         */
        private val entered = CountDownLatch(1)

        @Volatile
        var outcome: String = if (stream != null) "blocked" else "no stream"

        private val thread = Thread({
            val buffer = ByteArray(64)
            try {
                entered.countDown()
                val read = stream!!.read(buffer)
                outcome = if (read < 0) "eof" else "read $read bytes"
            } catch (error: Exception) {
                outcome = "exception " + error.javaClass.simpleName + ": " + error.message
            } finally {
                finished.countDown()
            }
        }, "raw-accessory-reader")

        fun start() {
            if (stream != null) thread.start()
        }

        fun await(timeoutMs: Long): Boolean = finished.await(timeoutMs, TimeUnit.MILLISECONDS)

        /** True once the reader is inside the read: the precondition, checked rather than assumed. */
        fun awaitEntered(timeoutMs: Long): Boolean = entered.await(timeoutMs, TimeUnit.MILLISECONDS)
    }
}
