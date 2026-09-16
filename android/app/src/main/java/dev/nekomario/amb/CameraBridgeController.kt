package dev.nekomario.amb

import android.content.Context

/** Connects the Camera2/MediaCodec producer to the selected transport session. */
class CameraBridgeController(
    private val context: Context,
    private val session: MediaSession,
    private val onStatus: (String) -> Unit,
    private val onError: (String) -> Unit,
) : AutoCloseable {
    private var cameraSession: CameraEncoderSession? = null

    fun start(request: CameraEncoderSession.Request) {
        check(cameraSession == null) { "camera bridge already running" }

        val listener = object : CameraEncoderSession.Listener {
            override fun onCodecConfig(config: H264Encoder.CodecConfig) {
                session.onCodecConfig(config)
            }

            override fun onAccessUnit(unit: H264Encoder.AccessUnit) {
                session.onAccessUnit(unit)
            }

            override fun onEncoderError(message: String, cause: Throwable?) {
                onError(buildError(message, cause))
            }

            override fun onCameraStarted(config: CameraEncoderSession.ActiveConfig) {
                session.onCameraStarted(config)
                onStatus(
                    "Camera ${config.cameraId}: ${config.size.width}x${config.size.height}" +
                        " @${config.fps} fps, ${config.bitrate / 1_000_000.0} Mbps",
                )
            }

            override fun onCameraError(message: String, cause: Throwable?) {
                onError(buildError(message, cause))
            }
        }

        val session = CameraEncoderSession(context, request, listener)
        cameraSession = session
        this.session.attachIdrRequester { session.requestIdr() }
        try {
            session.start()
        } catch (t: Throwable) {
            this.session.attachIdrRequester(null)
            cameraSession = null
            runCatching { session.close() }
            throw t
        }
    }

    fun requestIdr() {
        cameraSession?.requestIdr()
    }

    override fun close() {
        session.attachIdrRequester(null)
        runCatching { cameraSession?.close() }
        cameraSession = null
    }

    private fun buildError(message: String, cause: Throwable?): String =
        if (cause?.message.isNullOrBlank()) message else "$message: ${cause?.message}"
}
