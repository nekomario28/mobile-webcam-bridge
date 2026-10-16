package dev.nekomario.amb

import java.io.InputStream
import java.io.OutputStream
import java.util.concurrent.atomic.AtomicReference

/** One capture/control authorization boundary for both physical transports. */
abstract class AuthorizedMediaSession(
    output: OutputStream,
    private val authorization: PhoneAuthorization,
    private val transport: Int,
    private val phoneBinding: ByteArray,
    private val capabilities: (() -> Wire.Capabilities?)?,
    private val onQuality: (MediaSession, Wire.HostCommand) -> Unit,
    private val onAuthorization: (MediaSession) -> Unit,
    /**
     * Authorization loss must reach the capture producer here, not through a UI
     * callback a busy main thread can delay (AUTHREV-001). Required, so a new
     * transport cannot silently skip it. Called at most once per session on the
     * authorization/transport thread: hand off heavy teardown, never touch views.
     */
    private val onCaptureCancelled: (MediaSession) -> Unit,
    threadName: String,
) : MediaSession {
    private val idrRequester = AtomicReference<(() -> Unit)?>(null)
    private val captureCancelled = java.util.concurrent.atomic.AtomicBoolean(false)
    private val outbound = FramedOutbound(output, { idrRequester.get()?.invoke() }, { failMedia(it) }, threadName = threadName)
    private lateinit var gate: PhoneAuthorization.Session
    override val clientAuthorization: PhoneAuthorization.Session? get() = if (::gate.isInitialized) gate else null
    private val control = SessionControl(outbound, { onQuality(this, it) }, { onControlPing() })

    protected abstract fun failMedia(reason: String)
    protected open fun onControlPing() {}

    protected fun startMedia() {
        outbound.start()
        gate = authorization.openSession(transport, phoneBinding,
            send = { type, payload -> outbound.sendAuthorization(type, payload) },
            onChanged = { updateCapabilities(); onAuthorization(this) },
            onClosed = { failMedia(it) })
        gate.start()
    }

    protected fun readMedia(input: InputStream) {
        val (header, payload) = Wire.readHostFrame(input)
        if (gate.isAuthorized()) control.dispatch(header, payload)
        else gate.receive(header, payload)
    }

    protected fun closeMedia() {
        idrRequester.set(null)
        if (captureCancelled.compareAndSet(false, true)) onCaptureCancelled(this)
        clientAuthorization?.close()
        outbound.close()
    }

    private fun permitted() = clientAuthorization?.isAuthorized() == true
    override fun attachIdrRequester(requester: (() -> Unit)?) { idrRequester.set(requester) }
    override fun beginCapture(): Long {
        check(permitted()) { "PC authorization required before capture" }
        return outbound.beginCapture()
    }
    override fun stopVideo() = outbound.stopVideo()
    override fun onCodecConfig(config: H264Encoder.CodecConfig, generation: Long) {
        if (permitted()) outbound.setCodecConfig(config, generation)
    }
    override fun onVideoConfig(width: Int, height: Int, fps: Int, bitrate: Int, generation: Long) {
        if (permitted()) outbound.sendVideoConfig(width, height, fps, bitrate, generation)
    }
    override fun onAccessUnit(unit: H264Encoder.AccessUnit, generation: Long) {
        if (permitted()) outbound.sendAccessUnit(unit, generation)
    }
    override fun sendQualityResult(result: Wire.QualityResult) {
        if (permitted()) outbound.sendQualityResult(result)
    }
    override fun pauseQuality(epoch: Wire.Epoch) {
        if (permitted()) outbound.pauseQuality(epoch)
    }
    override fun droppedVideoFrames() = outbound.droppedVideoFrames.get()
    override fun updateCapabilities() {
        if (permitted()) capabilities?.invoke()?.let { outbound.sendCapabilities(it) }
    }
}
