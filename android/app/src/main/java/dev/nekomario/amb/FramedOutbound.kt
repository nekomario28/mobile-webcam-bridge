package dev.nekomario.amb

import java.io.OutputStream
import java.util.ArrayDeque
import java.util.concurrent.Semaphore
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong

/** One bounded writer owns control/video order and the capture-generation fence. */
class FramedOutbound(
    private val output: OutputStream,
    private val onNeedIdr: () -> Unit,
    private val onError: (String) -> Unit,
    private val videoCapacity: Int = 2,
    private val threadName: String = "mwb-framed-writer",
) : AutoCloseable {
    private data class Frame(val header: Wire.Header, val payload: ByteArray)
    private val running = AtomicBoolean(false)
    private val sequence = AtomicInteger(1)
    private val lock = Any()
    private val controlQueue = ArrayDeque<Frame>()
    private val videoQueue = ArrayDeque<Frame>()
    private val controlOverflowed = AtomicBoolean(false)
    private val wake = Semaphore(0)
    private var writerThread: Thread? = null
    private var generation = 0L
    private var videoEnabled = false
    private var latestCodecConfig = ByteArray(0)
    private var latestVideoConfig = ByteArray(0)
    private var recovering = false
    val droppedVideoFrames = AtomicLong(0)

    init { require(videoCapacity >= 1) }

    fun start() {
        if (!running.compareAndSet(false, true)) return
        writerThread = Thread({ writerLoop() }, threadName).also { it.start() }
    }

    fun beginCapture(): Long = synchronized(lock) {
        clearVideo()
        generation += 1
        videoEnabled = true
        generation
    }

    fun stopVideo() = synchronized(lock) {
        videoEnabled = false
        clearVideo()
    }

    private fun clearVideo() {
        droppedVideoFrames.addAndGet(videoQueue.size.toLong())
        videoQueue.clear()
        controlQueue.removeAll { it.header.type == Wire.TYPE_VIDEO_CONFIG }
        latestCodecConfig = ByteArray(0)
        latestVideoConfig = ByteArray(0)
        recovering = false
    }

    private fun accepts(token: Long): Boolean = running.get() && videoEnabled && token == generation

    fun sendPong(request: Wire.Header, payload: ByteArray) = enqueueControl(
        Frame(request.copy(type = Wire.TYPE_PONG, payloadLength = payload.size), payload),
    )

    fun sendCapabilities(capabilities: Wire.Capabilities) = sendControl(
        Wire.TYPE_QUALITY_CAPABILITIES, Wire.encodeCapabilities(capabilities),
    )

    fun sendQualityResult(result: Wire.QualityResult) = sendControl(
        Wire.TYPE_QUALITY_RESULT, Wire.encodeQualityResult(result),
    )

    /** The in-flight old frame may finish, but the same writer sends it BEFORE
     * PAUSED. Queued/config/late old-generation video cannot follow PAUSED. */
    fun pauseQuality(epoch: Wire.Epoch) {
        stopVideo()
        sendControl(Wire.TYPE_PAUSED, Wire.encodeEpoch(epoch))
    }

    private fun sendControl(type: Byte, payload: ByteArray) = enqueueControl(
        Frame(Wire.Header(type, 0, nextSequence(), payload.size, 0), payload),
    )

    fun setCodecConfig(config: H264Encoder.CodecConfig, token: Long) = synchronized(lock) {
        if (accepts(token)) latestCodecConfig = VideoWire.joinCodecConfig(config)
    }

    fun sendVideoConfig(width: Int, height: Int, fps: Int, bitrate: Int, token: Long) {
        val payload = VideoWire.encodeConfig(width, height, fps, bitrate)
        synchronized(lock) {
            if (!accepts(token)) return
            latestVideoConfig = payload
        }
        enqueueVideoConfig(payload, token)
        onNeedIdr()
    }

    fun sendAccessUnit(unit: H264Encoder.AccessUnit, token: Long) {
        if (unit.codecConfig && !unit.keyFrame) return
        var requestIdr = false
        synchronized(lock) {
            if (!accepts(token) || latestVideoConfig.isEmpty()) return
            if (recovering && !unit.keyFrame) {
                droppedVideoFrames.incrementAndGet()
                return
            }
            if (videoQueue.size >= videoCapacity) {
                droppedVideoFrames.addAndGet(videoQueue.size.toLong())
                videoQueue.clear()
                recovering = true
                requestIdr = true
                if (!unit.keyFrame) {
                    droppedVideoFrames.incrementAndGet()
                    // Request IDR outside the lock, including the dropped-AU path.
                }
            }
            if (!recovering || unit.keyFrame) {
                var flags: Short = 0
                var payload = unit.data
                if (unit.keyFrame) {
                    flags = Wire.VIDEO_FLAG_KEYFRAME
                    if (latestCodecConfig.isNotEmpty()) {
                        payload = latestCodecConfig + payload
                        flags = (flags.toInt() or Wire.VIDEO_FLAG_CONFIG_INCLUDED.toInt()).toShort()
                    }
                    if (recovering) {
                        flags = (flags.toInt() or Wire.VIDEO_FLAG_DISCONTINUITY.toInt()).toShort()
                        recovering = false
                    }
                }
                videoQueue.addLast(Frame(Wire.Header(Wire.TYPE_VIDEO_AU, flags, nextSequence(), payload.size, unit.presentationTimeUs), payload))
                wake.release()
            }
        }
        if (requestIdr) onNeedIdr()
    }

    fun forceRecovery() {
        val snapshot = synchronized(lock) {
            if (!videoEnabled || !running.get()) return
            droppedVideoFrames.addAndGet(videoQueue.size.toLong())
            videoQueue.clear()
            recovering = true
            generation to latestVideoConfig
        }
        if (snapshot.second.isNotEmpty()) enqueueVideoConfig(snapshot.second, snapshot.first)
        onNeedIdr()
    }

    private fun enqueueVideoConfig(payload: ByteArray, token: Long) = enqueueControl(
        Frame(Wire.Header(Wire.TYPE_VIDEO_CONFIG, 0, nextSequence(), payload.size, 0), payload), token,
    )

    private fun enqueueControl(frame: Frame, token: Long? = null) {
        if (!running.get() || controlOverflowed.get()) return
        val admitted = synchronized(lock) {
            if (!running.get() || (token != null && !accepts(token))) return
            if (controlQueue.size >= MAX_CONTROL_FRAMES) false else {
                controlQueue.addLast(frame)
                true
            }
        }
        if (admitted) wake.release()
        else if (controlOverflowed.compareAndSet(false, true)) onError("control queue limit exceeded")
    }

    private fun writerLoop() {
        try {
            while (running.get()) {
                wake.acquire()
                if (!running.get()) break
                val frame = synchronized(lock) {
                    if (controlQueue.isNotEmpty()) controlQueue.removeFirst()
                    else if (videoQueue.isNotEmpty()) videoQueue.removeFirst() else null
                } ?: continue
                Wire.writeFrame(output, frame.header, frame.payload)
            }
        } catch (t: Throwable) {
            if (running.get()) onError(t.message ?: t.javaClass.simpleName)
        } finally { running.set(false) }
    }

    private fun nextSequence(): Int = sequence.getAndUpdate { if (it == Int.MAX_VALUE) 1 else it + 1 }

    override fun close() {
        running.set(false)
        wake.release()
        writerThread?.interrupt()
        if (Thread.currentThread() !== writerThread) runCatching { writerThread?.join(500) }
        writerThread = null
        synchronized(lock) { controlQueue.clear(); clearVideo(); videoEnabled = false }
    }

    companion object { private const val MAX_CONTROL_FRAMES = 32 }
}
