package dev.nekomario.amb

import android.app.Instrumentation
import android.content.Context
import android.content.pm.PackageManager
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketTimeoutException
import java.security.KeyPairGenerator
import java.security.Signature
import java.security.spec.ECGenParameterSpec
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/**
 * Emulator/device acceptance for "authorization loss stops the producer".
 *
 * This drives the real `LanSession` -> `AuthorizedMediaSession.closeMedia()` ->
 * `CameraBridgeController` -> `CameraEncoderSession` -> `H264Encoder` path
 * against a real Camera2 device and requires:
 *
 *  - real AMB1 `VIDEO_CONFIG` and access units arrive before the revocation,
 *  - no access unit is accepted after it,
 *  - the transport becomes terminal,
 *  - the session-owned off-UI cancellation sink runs, and
 *  - the camera can be opened again afterwards (so "stopped sending" is
 *    distinguished from "stopped capturing").
 *
 * Boundaries, stated so a green run cannot be over-read: this is **not** physical
 * hardware, it is not the Activity's own dispatcher wiring, and it skips loudly
 * (with a `SKIP:` marker the workflow must surface) when the runtime has no
 * camera, no CAMERA permission or no usable capture mode. The cancellation sink
 * below mirrors `MainActivity.onCaptureCancelled` (dedicated executor, detach
 * then close); the production Activity path itself is still covered only by
 * inspection.
 */
internal class CameraTeardownInstrumentation(private val test: Instrumentation) {
    private val context: Context get() = test.targetContext

    /** Production controller for the mid-stream switch; set before commands arrive. */
    @Volatile private var qualityController: PhoneQualityController? = null

    /** Epoch the peer must commit to; advanced when a switch applies. */
    @Volatile private var admittedEpoch = 0

    @Volatile private var switchRecord: SwitchRecord? = null

    private class SwitchRecord(
        val attempted: Boolean,
        val pausedCount: Int,
        val applied: Boolean,
        val to: String,
        val configs: Int,
        val keyframes: Int,
        val continued: Int,
    ) {
        val report: String
            get() = if (!attempted) "unavailable (one supported mode)"
            else "paused=$pausedCount applied=$applied to=$to configs=$configs keyframes=$keyframes continued=$continued"
    }

    fun run(): String {
        var cameraId = ""
        var resolvedMode: CaptureModes.CaptureMode? = null
        if (context.checkSelfPermission(android.Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            return "SKIP camera acceptance: CAMERA permission is not granted on this runtime"
        }
        val cameras = CameraCatalog.enumerate(context)
        if (cameras.isEmpty()) {
            return "SKIP camera acceptance: no Camera2 device in this runtime"
        }
        val camera = cameras.firstOrNull { it.lensFacing == android.hardware.camera2.CameraCharacteristics.LENS_FACING_BACK }
            ?: cameras.first()
        cameraId = camera.id
        val capabilities = CaptureModes.collectCapabilities(context, cameraId)
            ?: return "SKIP camera acceptance: no Camera2 capabilities for $cameraId"
        resolvedMode = when (val resolution = CaptureModes.resolve(capabilities,
            CaptureModes.CapturePreference(null, null, null))) {
            is CaptureModes.Resolution.Resolved -> resolution.mode
            is CaptureModes.Resolution.Rejected ->
                return "SKIP camera acceptance: no usable mode for $cameraId (${resolution.reason})"
        }
        val mode = checkNotNull(resolvedMode)
        val modes = CaptureModes.supported(capabilities).take(Wire.MAX_QUALITY_MODES)

        // A disposable profile only: the workflow clears app data and grants
        // CAMERA before this runs. Never silently adopt existing state.
        val store = ClientStore(context)
        check(store.repository.snapshot().isEmpty()) { "camera acceptance requires a pristine authorization store" }
        val generator = KeyPairGenerator.getInstance("EC")
        generator.initialize(ECGenParameterSpec("secp256r1"))
        val keyPair = generator.generateKeyPair()
        val encoded = keyPair.public.encoded
        val point = encoded.copyOfRange(encoded.size - ClientAuthorization.PUBLIC_KEY_BYTES, encoded.size)
        val clientId = ClientAuthorization.deriveClientId(point)
        val now = System.currentTimeMillis() / 1000
        store.repository.approve(clientId, point, "instrumented-pc", now)

        val binding = ByteArray(Wire.PHONE_BINDING_BYTES) { (it + 1).toByte() }
        val authorization = PhoneAuthorization(store.repository,
            onStorageFailure = { error("authorization storage failed during camera acceptance") })
        val cancellations = CountDownLatch(1)
        val bridge = AtomicReference<CameraBridgeController?>(null)
        // The production owner, used exactly as MainActivity uses it: the
        // dispatcher is part of the tested class rather than a test look-alike.
        val cancellation = CaptureCancellation { bridge.getAndSet(null) }
        var session: LanSession? = null
        var client: Socket? = null
        try {
            ServerSocket(0).use { listener ->
                client = Socket("127.0.0.1", listener.localPort).also { it.soTimeout = 30_000 }
                val accepted = listener.accept()
                val active = LanSession(accepted, { _, _ -> }, authorization, binding,
                    onAuthorization = {},
                    onCaptureCancelled = {
                        cancellations.countDown()
                        cancellation.cancel()
                    },
                    onQuality = { _, command -> qualityController?.handle(command) },
                    capabilities = { Wire.Capabilities(1, Wire.PERMISSION_READY,
                        ADVERTISED_REVISION, admittedEpoch.toLong(), CameraTeardownInstrumentation.capabilityModes(capabilities)) })
                session = active
                active.start()
                val clientSocket = client!!
                val challenge = Wire.decodeClientChallenge(readOfType(clientSocket, Wire.TYPE_CLIENT_CHALLENGE))
                val message = Wire.clientAuthMessage(Wire.AUTH_LAN, binding, challenge.nonce, point, "instrumented-pc")
                val signature = Signature.getInstance("SHA256withECDSA").run {
                    initSign(keyPair.private)
                    update(message)
                    rawSignature(sign())
                }
                val proof = Wire.encodeClientAuth(Wire.ClientAuth(Wire.CLIENT_AUTH_VERSION, point, "instrumented-pc", signature))
                Wire.writeFrame(clientSocket.getOutputStream(),
                    Wire.Header(Wire.TYPE_CLIENT_AUTH, 0, 1, proof.size, 0), proof)
                val result = Wire.decodeClientAuthResult(readOfType(clientSocket, Wire.TYPE_CLIENT_AUTH_RESULT))
                check(result.status == Wire.AUTH_AUTHORIZED) {
                    "pre-approved grant was not authorized (status=${result.status}, reason=${result.reason})"
                }
                readOfType(clientSocket, Wire.TYPE_QUALITY_CAPABILITIES)

                val controller = CameraBridgeController(context, active, onStatus = {}, onError = { })
                bridge.set(controller)
                controller.start(CameraEncoderSession.Request(mode))

                // Real capture: require a config frame and at least one access unit.
                var configurations = 0
                var accessUnits = 0
                val started = SystemClock.elapsedRealtime()
                while (accessUnits == 0) {
                    val (header, _) = Wire.readFrame(clientSocket.getInputStream())
                    when (header.type) {
                        Wire.TYPE_VIDEO_CONFIG -> configurations++
                        Wire.TYPE_VIDEO_AU -> accessUnits++
                    }
                    check(SystemClock.elapsedRealtime() - started < 45_000) {
                        "no access unit within 45s (configs=$configurations) — the producer never ran"
                    }
                }
                val capturedAt = SystemClock.elapsedRealtime()

                // Mid-stream switch on the real camera: the production controller
                // drives the pause/commit/apply sequence, and this harness is the
                // capture owner (close/open the bridge) exactly as the Activity is.
                val switched = switchPhase(clientSocket, active, modes, mode, bridge)
                switchRecord = switched

                authorization.revoke(clientId)

                // `closeMedia` clears the queue before it closes the writer, so
                // at most the frame already inside the writer can reach the peer;
                // the JSON records the count separately.
                var trailing = 0
                val deadline = SystemClock.elapsedRealtime() + 4_000
                var terminal = false
                while (SystemClock.elapsedRealtime() < deadline) {
                    clientSocket.soTimeout = 300
                    try {
                        val (header, _) = Wire.readFrame(clientSocket.getInputStream())
                        if (header.type == Wire.TYPE_VIDEO_AU) trailing++
                    } catch (_: SocketTimeoutException) {
                        continue
                    } catch (_: Exception) {
                        terminal = true
                        break
                    }
                }
                check(terminal) { "the transport stayed readable after revocation" }
                check(trailing <= 1) {
                    "$trailing access units arrived after revocation (queue clearing did not bound it)"
                }
                check(cancellations.await(5, TimeUnit.SECONDS)) {
                    "the session-owned cancellation sink never ran"
                }
                check(SystemClock.elapsedRealtime() - capturedAt < 10_000) {
                    "capture did not stop promptly after revocation"
                }

                // Direct observation that capture stopped rather than merely
                // being withheld from this socket.
                val released = cameraReleased(cameraId)
                writeArtifact(cameraId, mode, configurations, accessUnits, trailing, terminal, released)
                check(released) { "the camera was still held after authorization loss" }
                return "PASS camera acceptance: real Camera2/MediaCodec capture stopped and the camera was " +
                    "released after revocation on $cameraId (${mode.width}x${mode.height}@${mode.targetFps}); " +
                    "configs=$configurations aus=$accessUnits trailingAfterRevoke=$trailing; " +
                    "switch=${switched.report} (emulated/virtual camera evidence, not physical hardware)"
            }
        } catch (error: Throwable) {
            writeArtifact(cameraId, resolvedMode, -1, -1, -1, false, false, error.toString())
            throw error
        } finally {
            runCatching { bridge.getAndSet(null)?.close() }
            runCatching { session?.close() }
            runCatching { client?.close() }
            cancellation.shutdown()
            runCatching { authorization.close() }
        }
    }

    /**
     * Mid-stream switch through the production controller: PREPARE -> pause
     * barrier -> COMMIT -> applied, on a real Camera2/MediaCodec producer. This
     * harness is the capture owner (close/open the bridge), as the Activity is.
     */
    private fun switchPhase(
        socket: Socket,
        session: MediaSession,
        modes: List<CaptureModes.CaptureMode>,
        current: CaptureModes.CaptureMode,
        bridge: AtomicReference<CameraBridgeController?>,
    ): SwitchRecord {
        val target = modes.firstOrNull {
            it.width != current.width || it.height != current.height || it.targetFps != current.targetFps
        } ?: return SwitchRecord(false, 0, false, "-", 0, 0, 0)

        val holder = AtomicReference<PhoneQualityController?>(null)
        val controller = PhoneQualityController(
            session = session,
            revision = { ADVERTISED_REVISION },
            modes = { modes },
            permissionReady = { true },
            foreground = { true },
            streaming = { bridge.get() != null },
            stopCapture = { runCatching { bridge.getAndSet(null)?.close() }; Unit },
            startCapture = { requested ->
                bridge.set(openBridge(session, requested) { width, height, fps ->
                    holder.get()?.captureStarted(width, height, fps)
                })
            },
            startingEpoch = admittedEpoch.toLong(),
        )
        holder.set(controller)
        qualityController = controller

        val requestId = 1L
        val prepare = Wire.encodePrepareQuality(Wire.PrepareQuality(
            requestId, ADVERTISED_REVISION, target.width, target.height, target.targetFps))
        Wire.writeFrame(socket.getOutputStream(),
            Wire.Header(Wire.TYPE_PREPARE_QUALITY, 0, 1, prepare.size, 0), prepare)

        // Exactly one pause barrier, naming this request and the current epoch.
        var paused = 0
        var epoch: Wire.Epoch? = null
        val pauseDeadline = SystemClock.elapsedRealtime() + 20_000
        while (paused == 0) {
            val frame = try {
                Wire.readFrame(socket.getInputStream())
            } catch (timeout: SocketTimeoutException) {
                error("no pause barrier after PREPARE: the controller refused the target mode " +
                    "(revision/mode/permission), it never staged")
            }
            if (frame.first.type == Wire.TYPE_PAUSED) {
                paused++
                epoch = Wire.decodeEpoch(frame.second)
            }
            check(SystemClock.elapsedRealtime() < pauseDeadline) { "no pause barrier after PREPARE" }
        }
        val committed = checkNotNull(epoch) { "pause barrier carried no epoch" }
        check(committed.requestId == requestId) { "pause barrier named request ${committed.requestId}" }
        // The barrier and the producer stop are both part of staging; wait briefly
        // for the stop rather than assuming which one the machine emits first.
        val stopDeadline = SystemClock.elapsedRealtime() + 5_000
        while (bridge.get() != null) {
            check(SystemClock.elapsedRealtime() < stopDeadline) { "the old producer survived the pause barrier" }
            Thread.sleep(20)
        }

        // `QualitySwitch.Machine.Commit` requires `event.epoch == epoch + 1`: the
        // barrier announces the epoch being left and the commit enters the next
        // one. Committing the announced epoch is silently ignored as stale.
        val commitEpoch = Wire.Epoch(requestId, committed.streamEpoch + 1)
        Wire.writeFrame(socket.getOutputStream(), Wire.Header(Wire.TYPE_COMMIT_QUALITY, 0, 2, 8, 0),
            Wire.encodeEpoch(commitEpoch))

        var applied = false
        var configs = 0
        var keyframes = 0
        var continued = 0
        val applyDeadline = SystemClock.elapsedRealtime() + 40_000
        while (continued < 2) {
            val frame = try {
                Wire.readFrame(socket.getInputStream())
            } catch (timeout: SocketTimeoutException) {
                // A commit whose epoch is stale is ignored by design, which looks
                // exactly like this; name it instead of reporting a bare timeout.
                error("no frame after COMMIT (applied=$applied configs=$configs keyframes=$keyframes): " +
                    "a stale stream epoch is ignored by the machine")
            }
            val (header, payload) = frame
            when (header.type) {
                Wire.TYPE_QUALITY_RESULT -> {
                    val result = Wire.decodeQualityResult(payload)
                    if (result.status == Wire.RESULT_APPLIED) {
                        applied = true
                        check(result.appliedWidth == target.width && result.appliedHeight == target.height &&
                            result.appliedFps == target.targetFps) {
                            "applied mode mismatch: ${result.appliedWidth}x${result.appliedHeight}@${result.appliedFps}"
                        }
                    }
                }
                Wire.TYPE_VIDEO_CONFIG -> {
                    configs++
                    val width = (payload[0].toInt() and 0xff) or ((payload[1].toInt() and 0xff) shl 8)
                    val height = (payload[2].toInt() and 0xff) or ((payload[3].toInt() and 0xff) shl 8)
                    check(width == target.width && height == target.height) {
                        "the new producer emitted ${width}x$height instead of ${target.width}x${target.height}"
                    }
                }
                Wire.TYPE_VIDEO_AU -> {
                    if (header.flags.toInt() and Wire.VIDEO_FLAG_KEYFRAME.toInt() != 0) keyframes++
                    if (applied && keyframes > 0) continued++
                }
            }
            check(SystemClock.elapsedRealtime() < applyDeadline) {
                "the switch did not apply: applied=$applied configs=$configs keyframes=$keyframes"
            }
        }
        check(applied) { "COMMIT never produced an applied result" }
        check(configs >= 1 && keyframes >= 1) {
            "the applied switch produced no config/keyframe (configs=$configs keyframes=$keyframes)"
        }
        check(controller.epoch == commitEpoch.streamEpoch) {
            "the machine applied epoch ${controller.epoch} instead of ${commitEpoch.streamEpoch}"
        }
        admittedEpoch = controller.epoch.toInt()
        return SwitchRecord(true, paused, applied,
            "${target.width}x${target.height}@${target.targetFps}", configs, keyframes, continued)
    }

    private fun openBridge(
        session: MediaSession,
        mode: CaptureModes.CaptureMode,
        onStarted: (Int, Int, Int) -> Unit,
    ): CameraBridgeController {
        val controller = CameraBridgeController(context, session, onStatus = {}, onError = {},
            onStarted = { config -> onStarted(config.size.width, config.size.height, config.fps) })
        controller.start(CameraEncoderSession.Request(mode))
        return controller
    }

    /** Opens the camera again: success means the previous client released it. */
    private fun cameraReleased(cameraId: String): Boolean {
        val manager = context.getSystemService(CameraManager::class.java) ?: return false
        val thread = HandlerThread("acceptance-camera-release")
        thread.start()
        val opened = CountDownLatch(1)
        val failed = AtomicBoolean(false)
        val device = AtomicReference<CameraDevice?>(null)
        return try {
            manager.openCamera(cameraId, object : CameraDevice.StateCallback() {
                override fun onOpened(camera: CameraDevice) {
                    device.set(camera)
                    opened.countDown()
                }
                override fun onDisconnected(camera: CameraDevice) {
                    camera.close()
                    failed.set(true)
                    opened.countDown()
                }
                override fun onError(camera: CameraDevice, error: Int) {
                    camera.close()
                    failed.set(true)
                    opened.countDown()
                }
            }, Handler(thread.looper))
            val ok = opened.await(10, TimeUnit.SECONDS) && !failed.get()
            device.getAndSet(null)?.let { runCatching { it.close() } }
            ok
        } catch (_: Exception) {
            false
        } finally {
            thread.quitSafely()
        }
    }

    private fun writeArtifact(
        cameraId: String,
        mode: CaptureModes.CaptureMode?,
        configurations: Int,
        accessUnits: Int,
        trailing: Int,
        terminal: Boolean,
        released: Boolean,
        error: String? = null,
    ) {
        val directory = context.getExternalFilesDir("camera-acceptance") ?: return
        check(directory.isDirectory || directory.mkdirs())
        val device = android.os.Build.MODEL.replace("\"", "")
        val json = buildString {
            append("{")
            append("\"androidModel\":\"$device\",")
            append("\"androidVersion\":\"${android.os.Build.VERSION.RELEASE}\",")
            append("\"hardware\":\"${android.os.Build.HARDWARE}\",")
            append("\"cameraId\":\"$cameraId\",")
            append("\"mode\":\"${mode?.let { "${it.width}x${it.height}@${it.targetFps}" } ?: "unknown"}\",")
            append("\"videoConfigs\":$configurations,")
            append("\"accessUnits\":$accessUnits,")
            append("\"accessUnitsAfterRevoke\":$trailing,")
            append("\"transportTerminal\":$terminal,")
            append("\"cameraReleased\":$released")
            switchRecord?.let {
                append(",\"switchAttempted\":${it.attempted}")
                append(",\"switchPaused\":${it.pausedCount}")
                append(",\"switchApplied\":${it.applied}")
                append(",\"switchTo\":\"${it.to}\"")
                append(",\"switchConfigs\":${it.configs}")
                append(",\"switchKeyframes\":${it.keyframes}")
            }
            if (error != null) append(",\"error\":\"${error.replace("\"", "'")}\"")
            append("}")
        }
        java.io.File(directory, "camera-acceptance.json").writeText(json)
    }

    private fun readOfType(socket: Socket, type: Byte): ByteArray {
        while (true) {
            val (header, payload) = Wire.readFrame(socket.getInputStream())
            if (header.type == type) return payload
        }
    }

    private fun rawSignature(der: ByteArray): ByteArray {
        check(der[0] == 0x30.toByte() && der[1].toInt() == der.size - 2)
        var offset = 2
        val output = ByteArray(64)
        repeat(2) { part ->
            check(der[offset++] == 2.toByte())
            val size = der[offset++].toInt() and 0xff
            val integer = java.math.BigInteger(der.copyOfRange(offset, offset + size))
            check(integer.signum() >= 0 && integer.bitLength() <= 256)
            val bytes = integer.toByteArray().takeLast(32).toByteArray()
            bytes.copyInto(output, (part + 1) * 32 - bytes.size)
            offset += size
        }
        check(offset == der.size)
        return output
    }

    private companion object {
        /** Must match the advertised catalog revision the peer prepares against. */
        const val ADVERTISED_REVISION = 1

        fun capabilityModes(capabilities: CaptureModes.Capabilities): List<Wire.QualityMode> =
            CaptureModes.supported(capabilities).take(Wire.MAX_QUALITY_MODES).map {
                Wire.QualityMode(it.width, it.height, it.targetFps, it.bitrate)
            }
    }
}
