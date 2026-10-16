package dev.nekomario.amb

import android.content.Context
import java.util.concurrent.atomic.AtomicBoolean

/** Connects the Camera2/MediaCodec producer to the selected transport session. */
class CameraBridgeController(
    private val context: Context,
    private val session: MediaSession,
    private val onStatus: (String) -> Unit,
    private val onError: (String) -> Unit,
    private val onStarted: (CameraEncoderSession.ActiveConfig) -> Unit = {},
) : AutoCloseable {
    private var cameraSession: CameraEncoderSession? = null
    private val closed = AtomicBoolean(false)
    // Serialises start() against close(): a cancellation that lands while the
    // camera session is being constructed must not be missed (REV-003).
    private val lifecycle = Any()

    fun start(request: CameraEncoderSession.Request) = synchronized(lifecycle) {
        check(cameraSession == null) { "camera bridge already running" }

        check(!closed.get()) { "camera bridge is closed" }
        val generation = session.beginCapture()
        val listener = object : CameraEncoderSession.Listener {
            override fun onCodecConfig(config: H264Encoder.CodecConfig) {
                if (!closed.get()) session.onCodecConfig(config, generation)
            }

            override fun onAccessUnit(unit: H264Encoder.AccessUnit) {
                if (!closed.get()) session.onAccessUnit(unit, generation)
            }

            override fun onEncoderError(message: String, cause: Throwable?) {
                if (!closed.get()) onError(buildError(message, cause))
            }

            override fun onCameraStarted(config: CameraEncoderSession.ActiveConfig) {
                if (closed.get()) return
                // Android dimensions stop at the camera boundary; transport
                // authorization/framing does not depend on framework objects.
                session.onVideoConfig(width = config.size.width, height = config.size.height,
                    fps = config.fps, bitrate = config.bitrate, generation = generation)
                onStarted(config)
                onStatus(
                    "Camera ${config.cameraId}: ${config.size.width}x${config.size.height}" +
                        " @${config.fps} fps, ${config.bitrate / 1_000_000.0} Mbps",
                )
            }

            override fun onCameraError(message: String, cause: Throwable?) {
                if (!closed.get()) onError(buildError(message, cause))
            }
        }

        val created = CameraEncoderSession(context, request, listener,
            authorized = { this.session.clientAuthorization?.isAuthorized() == true })
        cameraSession = created
        try {
            // close() may have run while the session was being constructed; in
            // that case it saw no session to close, so close it here instead.
            check(!closed.get()) { "camera bridge was closed while starting" }
            this.session.attachIdrRequester { created.requestIdr() }
            created.start()
        } catch (t: Throwable) {
            cameraSession = null
            this.session.attachIdrRequester(null)
            runCatching { created.close() }
            session.stopVideo()
            throw t
        }
    }

    fun requestIdr() {
        cameraSession?.requestIdr()
    }

    override fun close() {
        val current = synchronized(lifecycle) {
            if (!closed.compareAndSet(false, true)) return
            cameraSession.also { cameraSession = null }
        }
        session.stopVideo()
        session.attachIdrRequester(null)
        runCatching { current?.close() }
    }

    private fun buildError(message: String, cause: Throwable?): String =
        if (cause?.message.isNullOrBlank()) message else "$message: ${cause?.message}"
}
