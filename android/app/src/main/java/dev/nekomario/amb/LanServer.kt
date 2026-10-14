package dev.nekomario.amb

import java.io.EOFException
import java.io.FilterInputStream
import java.net.Inet4Address
import java.net.NetworkInterface
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketTimeoutException
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

class LanSession(
    private val socket: Socket,
    private val onClosed: (LanSession, String?) -> Unit,
    private val capabilities: (() -> Wire.Capabilities?)? = null,
) : MediaSession {
    private val running = AtomicBoolean(false)
    private val input = socket.getInputStream()
    private val output = socket.getOutputStream()
    private val idrRequester = AtomicReference<(() -> Unit)?>(null)
    private val outbound = FramedOutbound(
        output = output,
        onNeedIdr = { idrRequester.get()?.invoke() },
        onError = { finish(it) },
        threadName = "mwb-lan-writer",
    )
    private var readerThread: Thread? = null

    fun isActive(): Boolean = running.get()

    fun start() {
        if (!running.compareAndSet(false, true)) return
        outbound.start()
        // The new APK advertises its capabilities right after pairing; an old
        // PC ignores the unknown type (spec slice 02).
        capabilities?.invoke()?.let { outbound.sendCapabilities(it) }
        readerThread = Thread({ readLoop() }, "mwb-lan-reader").also { it.start() }
    }

    /** Re-advertise after the capability list changes (e.g. permission granted). */
    override fun updateCapabilities() {
        capabilities?.invoke()?.let { outbound.sendCapabilities(it) }
    }

    override fun attachIdrRequester(requester: (() -> Unit)?) {
        idrRequester.set(requester)
    }

    override fun onCodecConfig(config: H264Encoder.CodecConfig) = outbound.setCodecConfig(config)
    override fun onCameraStarted(config: CameraEncoderSession.ActiveConfig) = outbound.sendVideoConfig(config)
    override fun onAccessUnit(unit: H264Encoder.AccessUnit) = outbound.sendAccessUnit(unit)
    override fun droppedVideoFrames(): Long = outbound.droppedVideoFrames.get()

    private fun readLoop() {
        try {
            var pingWindowStart = System.nanoTime()
            var pingCount = 0
            while (running.get()) {
                val (header, payload) = Wire.readFrame(input, maxPayloadLength = 8)
                when (header.type) {
                    Wire.TYPE_PING -> {
                        require(payload.size == 8) { "PING payload must be 8 bytes" }
                        val now = System.nanoTime()
                        if (now - pingWindowStart >= 1_000_000_000L) {
                            pingWindowStart = now
                            pingCount = 0
                        }
                        require(++pingCount <= 64) { "PING rate limit exceeded" }
                        outbound.sendPong(header, payload)
                    }
                    Wire.TYPE_VIDEO_IDR_REQUEST -> {
                        require(payload.isEmpty()) { "VIDEO_IDR_REQUEST payload must be empty" }
                        outbound.forceRecovery()
                    }
                    else -> error("unexpected host message type 0x%02x".format(header.type))
                }
            }
        } catch (t: Throwable) {
            if (running.get()) finish(if (t is EOFException) null else t.message ?: t.javaClass.simpleName)
        } finally {
            finish(null)
        }
    }

    private fun finish(error: String?) {
        if (!running.compareAndSet(true, false)) return
        idrRequester.set(null)
        outbound.close()
        runCatching { socket.close() }
        onClosed(this, error)
    }

    override fun close() {
        finish(null)
        readerThread?.interrupt()
        if (Thread.currentThread() !== readerThread) runCatching { readerThread?.join(500) }
        readerThread = null
    }
}

class LanServer(
    private val port: Int = DEFAULT_PORT,
    private val onSession: (LanSession, Boolean) -> Unit,
    private val onStatus: (String) -> Unit,
    private val onError: (String) -> Unit,
) : AutoCloseable {
    private val running = AtomicBoolean(false)
    private val active = AtomicReference<LanSession?>(null)
    private var server: ServerSocket? = null
    private var acceptThread: Thread? = null

    fun start() {
        if (!running.compareAndSet(false, true)) return
        try {
            val socket = ServerSocket(port).apply { reuseAddress = true }
            server = socket
            acceptThread = Thread({ acceptLoop(socket) }, "mwb-lan-accept").also { it.start() }
            onStatus("Wi-Fi listening on ${endpoints().joinToString()}")
        } catch (t: Throwable) {
            running.set(false)
            server = null
            throw t
        }
    }

    private fun acceptLoop(server: ServerSocket) {
        while (running.get()) {
            val socket = try {
                server.accept()
            } catch (t: Throwable) {
                if (running.get()) onError(t.message ?: t.javaClass.simpleName)
                break
            }
            if (active.get() != null) {
                runCatching { socket.close() }
                continue
            }
            runCatching { authenticate(socket) }
                .onSuccess {
                    if (!running.get()) {
                        runCatching { socket.close() }
                        return@onSuccess
                    }
                    lateinit var session: LanSession
                    session = LanSession(socket, { closed, error ->
                        if (active.compareAndSet(closed, null)) {
                            onSession(closed, false)
                        }
                        if (!error.isNullOrBlank()) onError(error)
                    }, capabilities)
                    active.set(session)
                    session.start()
                    onSession(session, true)
                }
                .onFailure {
                    runCatching { socket.close() }
                    onError("Wi-Fi client rejected: ${it.message}")
                }
        }
    }

    private fun authenticate(socket: Socket) {
        socket.tcpNoDelay = true
        socket.keepAlive = true
        val hello = readHelloWithDeadline(socket, 5_000)
        Wire.writeFrame(
            socket.getOutputStream(),
            Wire.Header(Wire.TYPE_HELLO_ACK, 0, hello.sequence, 0, 0),
            ByteArray(0),
        )
        socket.soTimeout = 0
    }

    fun endpoints(): List<String> = localIpv4Addresses().map { "$it:$port" }

    override fun close() {
        if (!running.compareAndSet(true, false)) return
        active.getAndSet(null)?.close()
        runCatching { server?.close() }
        acceptThread?.interrupt()
        runCatching { acceptThread?.join(500) }
        acceptThread = null
        server = null
    }

    companion object {
        const val DEFAULT_PORT = 48_527

        fun localIpv4Addresses(): List<String> = runCatching {
            NetworkInterface.getNetworkInterfaces().toList()
                .filter { it.isUp && !it.isLoopback }
                .flatMap { it.inetAddresses.toList() }
                .filterIsInstance<Inet4Address>()
                .filter { !it.isLoopbackAddress }
                .map { it.hostAddress ?: "" }
                .filter { it.isNotBlank() }
                .distinct()
        }.getOrDefault(emptyList())
    }
}

/**
 * Enforce one absolute HELLO deadline, not a new socket timeout per received byte.
 * A slow sender must not occupy the single-threaded accept loop indefinitely.
 */
internal fun readHelloWithDeadline(socket: Socket, timeoutMillis: Int): Wire.Header {
    require(timeoutMillis > 0)
    val deadline = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(timeoutMillis.toLong())
    val limitedInput = object : FilterInputStream(socket.getInputStream()) {
        override fun read(bytes: ByteArray, offset: Int, length: Int): Int {
            val remaining = deadline - System.nanoTime()
            if (remaining <= 0) throw SocketTimeoutException("Wi-Fi HELLO deadline expired")
            socket.soTimeout = TimeUnit.NANOSECONDS.toMillis(remaining)
                .coerceIn(1L, Int.MAX_VALUE.toLong()).toInt()
            return super.read(bytes, offset, length)
        }
    }
    val (hello, payload) = Wire.readFrame(limitedInput, maxPayloadLength = 0)
    require(hello.type == Wire.TYPE_HELLO) { "expected HELLO" }
    require(payload.isEmpty()) { "HELLO payload must be empty" }
    return hello
}
