package dev.nekomario.amb

import java.io.OutputStream
import java.util.ArrayDeque
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.Semaphore
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong

/** Bounded writer shared by USB AOA and LAN transports. */
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
    private val controlQueue = ConcurrentLinkedQueue<Frame>()
    private val videoLock = Any()
    private val videoQueue = ArrayDeque<Frame>()
    private val wake = Semaphore(0)
    private var writerThread: Thread? = null
    private var latestCodecConfig = ByteArray(0)
    private var latestVideoConfig = ByteArray(0)
    private var recovering = false

    val droppedVideoFrames = AtomicLong(0)

    init {
        require(videoCapacity >= 1)
    }

    fun start() {
        if (!running.compareAndSet(false, true)) return
        writerThread = Thread({ writerLoop() }, threadName).also { it.start() }
    }

    fun sendPong(request: Wire.Header, payload: ByteArray) {
        enqueueControl(
            Frame(
                request.copy(type = Wire.TYPE_PONG, payloadLength = payload.size),
                payload,
            ),
        )
    }

    fun setCodecConfig(config: H264Encoder.CodecConfig) {
        synchronized(videoLock) { latestCodecConfig = VideoWire.joinCodecConfig(config) }
    }

    fun sendVideoConfig(config: CameraEncoderSession.ActiveConfig) {
        val payload = VideoWire.encodeConfig(
            width = config.size.width,
            height = config.size.height,
            fps = config.fps,
            bitrate = config.bitrate,
        )
        synchronized(videoLock) { latestVideoConfig = payload }
        enqueueVideoConfig(payload)
    }

    fun sendAccessUnit(unit: H264Encoder.AccessUnit) {
        if (!running.get()) return
        if (unit.codecConfig && !unit.keyFrame) return

        var requestIdr = false
        synchronized(videoLock) {
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
                    return@synchronized
                }
            }

            var flags: Short = 0
            var payload = unit.data
            if (unit.keyFrame) {
                flags = (flags.toInt() or Wire.VIDEO_FLAG_KEYFRAME.toInt()).toShort()
                if (latestCodecConfig.isNotEmpty()) {
                    payload = ByteArray(latestCodecConfig.size + unit.data.size).also {
                        System.arraycopy(latestCodecConfig, 0, it, 0, latestCodecConfig.size)
                        System.arraycopy(unit.data, 0, it, latestCodecConfig.size, unit.data.size)
                    }
                    flags = (flags.toInt() or Wire.VIDEO_FLAG_CONFIG_INCLUDED.toInt()).toShort()
                }
                if (recovering) {
                    flags = (flags.toInt() or Wire.VIDEO_FLAG_DISCONTINUITY.toInt()).toShort()
                    recovering = false
                }
            }

            videoQueue.addLast(
                Frame(
                    Wire.Header(
                        type = Wire.TYPE_VIDEO_AU,
                        flags = flags,
                        sequence = nextSequence(),
                        payloadLength = payload.size,
                        ptsUs = unit.presentationTimeUs,
                    ),
                    payload,
                ),
            )
            wake.release()
        }
        if (requestIdr) onNeedIdr()
    }

    fun forceRecovery() {
        val config = synchronized(videoLock) {
            droppedVideoFrames.addAndGet(videoQueue.size.toLong())
            videoQueue.clear()
            recovering = true
            latestVideoConfig
        }
        if (config.isNotEmpty()) enqueueVideoConfig(config)
        onNeedIdr()
    }

    private fun enqueueVideoConfig(payload: ByteArray) {
        enqueueControl(
            Frame(
                Wire.Header(
                    type = Wire.TYPE_VIDEO_CONFIG,
                    flags = 0,
                    sequence = nextSequence(),
                    payloadLength = payload.size,
                    ptsUs = 0,
                ),
                payload,
            ),
        )
    }

    private fun enqueueControl(frame: Frame) {
        if (!running.get()) return
        controlQueue.add(frame)
        wake.release()
    }

    private fun writerLoop() {
        try {
            while (running.get()) {
                wake.acquire()
                if (!running.get()) break
                val frame = controlQueue.poll() ?: synchronized(videoLock) {
                    if (videoQueue.isEmpty()) null else videoQueue.removeFirst()
                } ?: continue
                Wire.writeFrame(output, frame.header, frame.payload)
            }
        } catch (t: Throwable) {
            if (running.get()) onError(t.message ?: t.javaClass.simpleName)
        } finally {
            running.set(false)
        }
    }

    private fun nextSequence(): Int = sequence.getAndUpdate {
        if (it == Int.MAX_VALUE) 1 else it + 1
    }

    override fun close() {
        if (!running.compareAndSet(true, false)) return
        wake.release()
        writerThread?.interrupt()
        if (Thread.currentThread() !== writerThread) {
            runCatching { writerThread?.join(500) }
        }
        writerThread = null
        controlQueue.clear()
        synchronized(videoLock) { videoQueue.clear() }
    }
}
