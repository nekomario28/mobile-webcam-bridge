package dev.nekomario.amb

import android.os.ParcelFileDescriptor
import java.io.FileInputStream
import java.io.FileOutputStream
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

class AccessorySession(
    private val pfd: ParcelFileDescriptor,
    private val onProgress: (Int) -> Unit,
    private val onError: (String) -> Unit,
    private val capabilities: (() -> Wire.Capabilities?)? = null,
    private val onQuality: (MediaSession, Wire.HostCommand) -> Unit = { _, _ -> },
) : MediaSession {
    private val running = AtomicBoolean(false)
    private var readerThread: Thread? = null
    private val input = FileInputStream(pfd.fileDescriptor)
    private val output = FileOutputStream(pfd.fileDescriptor)
    private val idrRequester = AtomicReference<(() -> Unit)?>(null)
    private val outbound = FramedOutbound(
        output = output,
        onNeedIdr = { idrRequester.get()?.invoke() },
        onError = onError,
        threadName = "mwb-usb-writer",
    )

    fun start() {
        if (!running.compareAndSet(false, true)) return
        outbound.start()
        // The new APK advertises its capabilities right after the accessory
        // session starts; an old PC ignores the unknown type.
        capabilities?.invoke()?.let { outbound.sendCapabilities(it) }
        readerThread = Thread({ runLoop() }, "amb-accessory-reader").also { it.start() }
    }

    /** Re-advertise after the capability list changes (e.g. permission granted). */
    override fun updateCapabilities() {
        capabilities?.invoke()?.let { outbound.sendCapabilities(it) }
    }

    override fun attachIdrRequester(requester: (() -> Unit)?) {
        idrRequester.set(requester)
    }

    override fun beginCapture() = outbound.beginCapture()
    override fun stopVideo() = outbound.stopVideo()
    override fun onCodecConfig(config: H264Encoder.CodecConfig, generation: Long) = outbound.setCodecConfig(config, generation)
    override fun onCameraStarted(config: CameraEncoderSession.ActiveConfig, generation: Long) = outbound.sendVideoConfig(config.size.width, config.size.height, config.fps, config.bitrate, generation)
    override fun onAccessUnit(unit: H264Encoder.AccessUnit, generation: Long) = outbound.sendAccessUnit(unit, generation)
    override fun sendQualityResult(result: Wire.QualityResult) = outbound.sendQualityResult(result)
    override fun pauseQuality(epoch: Wire.Epoch) = outbound.pauseQuality(epoch)

    override fun droppedVideoFrames(): Long = outbound.droppedVideoFrames.get()

    private fun runLoop() {
        var verified = 0
        try {
            val control = SessionControl(outbound, onQuality = { onQuality(this, it) }, onPing = {
                verified += 1
                if (verified == 1 || verified % 100 == 0) onProgress(verified)
            })
            while (running.get()) control.readAndDispatch(input)
        } catch (e: Exception) {
            if (running.get()) onError(e.message ?: e.javaClass.simpleName)
        } finally {
            running.set(false)
            outbound.close()
        }
    }

    override fun close() {
        running.set(false)
        idrRequester.set(null)
        runCatching { pfd.close() }
        outbound.close()
        readerThread?.interrupt()
        if (Thread.currentThread() !== readerThread) {
            runCatching { readerThread?.join(500) }
        }
        readerThread = null
    }
}
