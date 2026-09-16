package dev.nekomario.amb

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.view.Surface
import java.nio.ByteBuffer
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Surface-input H.264 encoder. Camera2 writes directly to [inputSurface], so the
 * target path has no per-frame CPU YUV repack/copy stage.
 *
 * Encoded output handed to [Listener] is normalized to Annex-B. The transport
 * therefore never needs to infer vendor-specific MediaCodec NAL framing.
 */
class H264Encoder(
    private val config: Config,
    private val listener: Listener,
) : AutoCloseable {
    data class Config(
        val width: Int,
        val height: Int,
        val fps: Int,
        val bitrate: Int,
        val iFrameIntervalSeconds: Int = 1,
    )

    data class CodecConfig(
        val csd0: ByteArray?,
        val csd1: ByteArray?,
    )

    data class AccessUnit(
        val data: ByteArray,
        val presentationTimeUs: Long,
        val keyFrame: Boolean,
        val codecConfig: Boolean,
    )

    interface Listener {
        fun onCodecConfig(config: CodecConfig)
        fun onAccessUnit(unit: AccessUnit)
        fun onEncoderError(message: String, cause: Throwable? = null)
    }

    private val running = AtomicBoolean(false)
    private val callbackThread = HandlerThread("amb-h264-encoder")
    private lateinit var codec: MediaCodec
    lateinit var inputSurface: Surface
        private set

    fun start(): Surface {
        check(running.compareAndSet(false, true)) { "encoder already started" }
        callbackThread.start()
        val handler = Handler(callbackThread.looper)

        try {
            codec = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
            codec.setCallback(object : MediaCodec.Callback() {
                override fun onInputBufferAvailable(codec: MediaCodec, index: Int) {
                    // Surface-input encoders do not use client input buffers.
                }

                override fun onOutputBufferAvailable(
                    codec: MediaCodec,
                    index: Int,
                    info: MediaCodec.BufferInfo,
                ) {
                    try {
                        if (!running.get() || info.size <= 0) return
                        val output = codec.getOutputBuffer(index) ?: return
                        val bytes = H264AnnexB.normalize(output.copyRange(info.offset, info.size))
                        listener.onAccessUnit(
                            AccessUnit(
                                data = bytes,
                                presentationTimeUs = info.presentationTimeUs,
                                keyFrame = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0,
                                codecConfig = info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0,
                            ),
                        )
                    } catch (t: Throwable) {
                        if (running.get()) listener.onEncoderError("failed to consume encoder output", t)
                    } finally {
                        runCatching { codec.releaseOutputBuffer(index, false) }
                    }
                }

                override fun onOutputFormatChanged(codec: MediaCodec, format: MediaFormat) {
                    try {
                        listener.onCodecConfig(
                            CodecConfig(
                                csd0 = format.getByteBuffer("csd-0")
                                    ?.copyAll()
                                    ?.let(H264AnnexB::normalize),
                                csd1 = format.getByteBuffer("csd-1")
                                    ?.copyAll()
                                    ?.let(H264AnnexB::normalize),
                            ),
                        )
                    } catch (t: Throwable) {
                        listener.onEncoderError("invalid H.264 codec configuration", t)
                    }
                }

                override fun onError(codec: MediaCodec, e: MediaCodec.CodecException) {
                    listener.onEncoderError("MediaCodec encoder error: ${e.diagnosticInfo}", e)
                }
            }, handler)

            val capabilities = codec.codecInfo
                .getCapabilitiesForType(MediaFormat.MIMETYPE_VIDEO_AVC)
                .encoderCapabilities
            val supportsCbr = capabilities?.isBitrateModeSupported(
                MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR,
            ) == true

            val format = MediaFormat.createVideoFormat(
                MediaFormat.MIMETYPE_VIDEO_AVC,
                config.width,
                config.height,
            ).apply {
                setInteger(
                    MediaFormat.KEY_COLOR_FORMAT,
                    MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface,
                )
                setInteger(MediaFormat.KEY_BIT_RATE, config.bitrate)
                setInteger(MediaFormat.KEY_FRAME_RATE, config.fps)
                setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, config.iFrameIntervalSeconds)
                if (supportsCbr) {
                    setInteger(
                        MediaFormat.KEY_BITRATE_MODE,
                        MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR,
                    )
                }
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    setInteger(MediaFormat.KEY_MAX_B_FRAMES, 0)
                }
            }

            codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            inputSurface = codec.createInputSurface()
            codec.start()
            return inputSurface
        } catch (t: Throwable) {
            running.set(false)
            runCatching { if (::inputSurface.isInitialized) inputSurface.release() }
            runCatching { if (::codec.isInitialized) codec.release() }
            callbackThread.quitSafely()
            listener.onEncoderError("unable to start H.264 surface encoder", t)
            throw t
        }
    }

    fun requestIdr() {
        if (!running.get() || !::codec.isInitialized) return
        val params = Bundle().apply {
            putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0)
        }
        runCatching { codec.setParameters(params) }
            .onFailure { listener.onEncoderError("IDR request failed", it) }
    }

    override fun close() {
        if (!running.compareAndSet(true, false)) return
        if (::codec.isInitialized) {
            runCatching { codec.stop() }
            runCatching { codec.release() }
        }
        if (::inputSurface.isInitialized) runCatching { inputSurface.release() }
        callbackThread.quitSafely()
        runCatching { callbackThread.join(1000) }
    }

    private fun ByteBuffer.copyRange(offset: Int, size: Int): ByteArray {
        val duplicate = duplicate()
        duplicate.position(offset)
        duplicate.limit(offset + size)
        return ByteArray(size).also { duplicate.get(it) }
    }

    private fun ByteBuffer.copyAll(): ByteArray {
        val duplicate = duplicate()
        duplicate.position(0)
        return ByteArray(duplicate.remaining()).also { duplicate.get(it) }
    }
}
