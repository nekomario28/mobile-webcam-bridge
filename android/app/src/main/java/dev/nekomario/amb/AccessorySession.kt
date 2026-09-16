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
        readerThread = Thread({ runLoop() }, "amb-accessory-reader").also { it.start() }
    }

    override fun attachIdrRequester(requester: (() -> Unit)?) {
        idrRequester.set(requester)
    }

    override fun onCodecConfig(config: H264Encoder.CodecConfig) {
        outbound.setCodecConfig(config)
    }

    override fun onCameraStarted(config: CameraEncoderSession.ActiveConfig) {
        outbound.sendVideoConfig(config)
    }

    override fun onAccessUnit(unit: H264Encoder.AccessUnit) {
        outbound.sendAccessUnit(unit)
    }

    override fun droppedVideoFrames(): Long = outbound.droppedVideoFrames.get()

    private fun runLoop() {
        var verified = 0
        try {
            while (running.get()) {
                val (header, payload) = Wire.readFrame(input)
                when (header.type) {
                    Wire.TYPE_PING -> {
                        require(payload.size == 8) { "G0.5 PING payload must be 8 bytes" }
                        outbound.sendPong(header, payload)
                        verified += 1
                        if (verified == 1 || verified % 100 == 0) onProgress(verified)
                    }
                    Wire.TYPE_VIDEO_IDR_REQUEST -> {
                        require(payload.isEmpty()) { "VIDEO_IDR_REQUEST payload must be empty" }
                        outbound.forceRecovery()
                    }
                    else -> error("unexpected host message type 0x%02x".format(header.type))
                }
            }
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
