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
    /**
     * Every host frame this session read, before it is dispatched or refused.
     *
     * A test seam with no effect on behaviour, in the same shape as [onQuality] and
     * [onCaptureCancelled]. It exists because a verdict that says "the session ended with
     * `PC authorization proof rejected`" names a *reason* that several failures share, so it cannot
     * say which frame caused it (independent review, 2026-10-25, ID-001). With this, an instrument
     * can report the header it read and require the frame it wrote.
     */
    private val onFrameRead: (Wire.Header) -> Unit = {},
) : MediaSession {
    private val idrRequester = AtomicReference<(() -> Unit)?>(null)
    private val captureCancelled = java.util.concurrent.atomic.AtomicBoolean(false)
    private val outbound = FramedOutbound(output, { idrRequester.get()?.invoke() }, { failMedia(it) }, threadName = threadName)
    private lateinit var gate: PhoneAuthorization.Session
    override val clientAuthorization: PhoneAuthorization.Session? get() = if (::gate.isInitialized) gate else null
    private val control = SessionControl(
        outbound,
        { onQuality(this, it) },
        { onControlPing() },
        { onHostHello() },
    )

    protected abstract fun failMedia(reason: String)
    protected open fun onControlPing() {}

    /**
     * A host said hello: end a spent session so the phone issues a fresh challenge.
     *
     * USB has no session boundary and AOA reports no disconnect, so a host that reconnects
     * finds an accessory session that has already been used — quiet if it authorised
     * somebody, or answering with the older connection's frames. Without this the only way
     * to get a new exchange was to restart the app or replug the cable (issue #78).
     */
    protected open fun onHostHello() {
        if (::gate.isInitialized) gate.onHostHello()
    }

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
        onFrameRead(header)
        if (gate.isAuthorized()) control.dispatch(header, payload)
        else gate.receive(header, payload)
    }

    protected fun closeMedia() {
        idrRequester.set(null)
        // Clear queued output first: authorization loss must not be followed by a
        // drain of already-queued frames. Only a frame already inside the writer
        // can still reach the peer, which is why the device acceptance allows one.
        outbound.stopVideo()
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
    /**
     * The Wi-Fi pairing hand-off, gated like every other sender.
     *
     * It is for a PC that has been *approved*: an earlier version wrote the frame while the
     * session was still unauthorized (the UI offered the button exactly then), which lands a
     * non-outcome frame inside the host's authorization exchange and dies on its protocol check
     * (independent review, ID-003).
     */
    override fun sendPairingInfo(payload: ByteArray) {
        if (permitted()) outbound.sendPairingInfo(payload)
    }

    override fun updateCapabilities() {
        if (permitted()) capabilities?.invoke()?.let { outbound.sendCapabilities(it) }
    }
}
