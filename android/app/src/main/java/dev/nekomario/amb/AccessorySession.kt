package dev.nekomario.amb

import android.os.ParcelFileDescriptor
import java.io.FileInputStream
import java.io.FileOutputStream
import java.util.concurrent.atomic.AtomicBoolean

class AccessorySession(
    private val pfd: ParcelFileDescriptor,
    private val onProgress: (Int) -> Unit,
    private val onError: (String) -> Unit,
    authorization: PhoneAuthorization,
    onAuthorization: (MediaSession) -> Unit,
    onCaptureCancelled: (MediaSession) -> Unit,
    capabilities: (() -> Wire.Capabilities?)? = null,
    onQuality: (MediaSession, Wire.HostCommand) -> Unit = { _, _ -> },
) : AuthorizedMediaSession(FileOutputStream(pfd.fileDescriptor), authorization, Wire.AUTH_USB, ByteArray(Wire.PHONE_BINDING_BYTES),
    capabilities, onQuality, onAuthorization, onCaptureCancelled, "mwb-usb-writer") {
    private val running = AtomicBoolean(false)
    private var readerThread: Thread? = null
    private val input = FileInputStream(pfd.fileDescriptor)
    private var verified = 0

    fun start() {
        if (!running.compareAndSet(false, true)) return
        try {
            startMedia()
            readerThread = Thread({
                try {
                    while (running.get()) readMedia(input)
                } catch (e: Exception) {
                    if (running.get()) failMedia(e.message ?: e.javaClass.simpleName)
                } finally { closeMedia() }
            }, "amb-accessory-reader").also { it.start() }
        } catch (e: Exception) { failMedia(e.message ?: "USB authorization startup failed") }
    }

    override fun onControlPing() {
        verified++
        if (verified == 1 || verified % 100 == 0) onProgress(verified)
    }

    override fun failMedia(reason: String) {
        if (!running.compareAndSet(true, false)) return
        // Closing the physical I/O first wakes any blocked reader/writer.
        runCatching { pfd.close() }
        closeMedia()
        onError(reason)
    }

    override fun close() {
        closeAndAwait(500)
    }

    /**
     * Close, and say whether the reader is really gone.
     *
     * A blocked read on the accessory stream is not interruptible, so closing the
     * descriptor is what unblocks it — and if that does not take effect inside the wait,
     * the old reader is still alive on the *same* endpoint. Starting a new session then
     * gives one accessory two readers: the older one takes the new connection's proof,
     * answers it against its own (already replaced) challenge, and its writer can put
     * capability frames in front of the authorization result. Measured on a real phone
     * 2026-10-24 as `BAD_SIGNATURE` and `expected authorization outcome before
     * capabilities/media`, both intermittent. Callers that are about to open a new session
     * must check this and refuse to run two.
     */
    fun closeAndAwait(timeoutMs: Long): Boolean {
        running.set(false)
        runCatching { pfd.close() }
        closeMedia()
        val reader = readerThread
        readerThread = null
        if (reader != null && Thread.currentThread() !== reader) {
            reader.interrupt()
            runCatching { reader.join(timeoutMs) }
            return !reader.isAlive
        }
        return true
    }
}
