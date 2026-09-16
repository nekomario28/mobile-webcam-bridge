package dev.nekomario.amb

import android.annotation.SuppressLint
import android.content.Context
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.os.Handler
import android.os.HandlerThread
import android.util.Range
import android.util.Size
import java.util.concurrent.atomic.AtomicBoolean

/** Owns Camera2 -> MediaCodec Surface for one video session. */
class CameraEncoderSession(
    private val context: Context,
    private val requested: Request,
    private val listener: Listener,
) : AutoCloseable {
    data class Request(
        val cameraId: String,
        val width: Int = 1280,
        val height: Int = 720,
        val fps: Int = 30,
        val bitrate: Int = 6_000_000,
    )

    data class ActiveConfig(
        val cameraId: String,
        val size: Size,
        val fps: Int,
        val aeFpsRange: Range<Int>?,
        val bitrate: Int,
    )

    interface Listener : H264Encoder.Listener {
        fun onCameraStarted(config: ActiveConfig)
        fun onCameraError(message: String, cause: Throwable? = null)
    }

    private val started = AtomicBoolean(false)
    private val closed = AtomicBoolean(false)
    private val cameraThread = HandlerThread("amb-camera2")
    private lateinit var cameraHandler: Handler
    private var camera: CameraDevice? = null
    private var captureSession: CameraCaptureSession? = null
    private var encoder: H264Encoder? = null
    private var activeConfig: ActiveConfig? = null

    @SuppressLint("MissingPermission")
    fun start() {
        check(started.compareAndSet(false, true)) { "camera session already started" }
        check(!closed.get()) { "camera session already closed" }

        val info = CameraCatalog.enumerate(context)
            .firstOrNull { it.id == requested.cameraId }
            ?: error("unknown Camera2 id: ${requested.cameraId}")
        val size = CameraCatalog.chooseSize(info, requested.width, requested.height)
            ?: error("camera ${requested.cameraId} exposes no MediaCodec output size")
        val fpsRange = CameraCatalog.chooseFpsRange(info, requested.fps)
        activeConfig = ActiveConfig(
            cameraId = requested.cameraId,
            size = size,
            fps = requested.fps,
            aeFpsRange = fpsRange,
            bitrate = requested.bitrate,
        )

        cameraThread.start()
        cameraHandler = Handler(cameraThread.looper)

        val newEncoder = H264Encoder(
            H264Encoder.Config(
                width = size.width,
                height = size.height,
                fps = requested.fps,
                bitrate = requested.bitrate,
            ),
            listener,
        )
        val encoderSurface = try {
            newEncoder.start()
        } catch (t: Throwable) {
            close()
            throw t
        }
        encoder = newEncoder

        val manager = context.getSystemService(CameraManager::class.java)
        manager.openCamera(
            requested.cameraId,
            object : CameraDevice.StateCallback() {
                override fun onOpened(device: CameraDevice) {
                    if (closed.get()) {
                        device.close()
                        return
                    }
                    camera = device
                    configureCapture(device, encoderSurface, info.supportsContinuousVideoAf)
                }

                override fun onDisconnected(device: CameraDevice) {
                    listener.onCameraError("camera disconnected: ${requested.cameraId}")
                    device.close()
                    close()
                }

                override fun onError(device: CameraDevice, error: Int) {
                    listener.onCameraError("Camera2 error=$error on ${requested.cameraId}")
                    device.close()
                    close()
                }
            },
            cameraHandler,
        )
    }

    private fun configureCapture(
        device: CameraDevice,
        surface: android.view.Surface,
        continuousVideoAf: Boolean,
    ) {
        device.createCaptureSession(
            listOf(surface),
            object : CameraCaptureSession.StateCallback() {
                override fun onConfigured(session: CameraCaptureSession) {
                    if (closed.get()) {
                        session.close()
                        return
                    }
                    captureSession = session
                    try {
                        val request = device.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
                            addTarget(surface)
                            set(CaptureRequest.CONTROL_MODE, CaptureRequest.CONTROL_MODE_AUTO)
                            set(CaptureRequest.CONTROL_AE_MODE, CaptureRequest.CONTROL_AE_MODE_ON)
                            activeConfig?.aeFpsRange?.let {
                                set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, it)
                            }
                            if (continuousVideoAf) {
                                set(
                                    CaptureRequest.CONTROL_AF_MODE,
                                    CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_VIDEO,
                                )
                            }
                        }.build()
                        session.setRepeatingRequest(request, null, cameraHandler)
                        activeConfig?.let(listener::onCameraStarted)
                    } catch (t: Throwable) {
                        listener.onCameraError("unable to start repeating Camera2 request", t)
                        close()
                    }
                }

                override fun onConfigureFailed(session: CameraCaptureSession) {
                    listener.onCameraError("Camera2 capture-session configuration failed")
                    session.close()
                    close()
                }
            },
            cameraHandler,
        )
    }

    fun requestIdr() {
        encoder?.requestIdr()
    }

    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        runCatching { captureSession?.stopRepeating() }
        runCatching { captureSession?.abortCaptures() }
        runCatching { captureSession?.close() }
        captureSession = null
        runCatching { camera?.close() }
        camera = null
        runCatching { encoder?.close() }
        encoder = null
        if (::cameraHandler.isInitialized) cameraHandler.removeCallbacksAndMessages(null)
        cameraThread.quitSafely()
        if (Thread.currentThread() !== cameraThread) {
            runCatching { cameraThread.join(1000) }
        }
    }
}
