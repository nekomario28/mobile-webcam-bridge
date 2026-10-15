package dev.nekomario.amb

import java.io.EOFException
import java.io.FilterInputStream
import java.net.Inet4Address
import java.net.NetworkInterface
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketTimeoutException
import javax.net.ssl.SSLSocket
import java.util.Collections
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.Semaphore
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

class LanSession(
    private val socket: Socket,
    private val onClosed: (LanSession, String?) -> Unit,
    authorization: PhoneAuthorization,
    phoneBinding: ByteArray,
    onAuthorization: (MediaSession) -> Unit,
    capabilities: (() -> Wire.Capabilities?)? = null,
    onQuality: (MediaSession, Wire.HostCommand) -> Unit = { _, _ -> },
) : AuthorizedMediaSession(socket.getOutputStream(), authorization, Wire.AUTH_LAN, phoneBinding,
    capabilities, onQuality, onAuthorization, "mwb-lan-writer") {
    private val running = AtomicBoolean(false)
    private val input = socket.getInputStream()
    private var readerThread: Thread? = null

    fun isActive(): Boolean = running.get()

    fun start(onStarted: () -> Unit = {}) {
        if (socket.isClosed || !running.compareAndSet(false, true)) return
        try {
            onStarted()
            startMedia()
            readerThread = Thread({
                try {
                    while (running.get()) readMedia(input)
                } catch (t: Exception) {
                    if (running.get()) finish(if (t is EOFException) null else t.message ?: t.javaClass.simpleName)
                } finally { finish(null) }
            }, "mwb-lan-reader").also { it.start() }
        } catch (e: Exception) { finish(e.message ?: "Wi-Fi authorization startup failed") }
    }

    override fun failMedia(reason: String) = finish(reason)

    private fun finish(error: String?) {
        // A server shutdown may close a constructed session before start().
        runCatching { socket.close() }
        if (!running.compareAndSet(true, false)) return
        closeMedia()
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
    private val identity: SecureLanIdentity,
    private val authorization: PhoneAuthorization,
    private val onAuthorization: (MediaSession) -> Unit,
    private val port: Int = DEFAULT_PORT,
    private val onSession: (LanSession, Boolean) -> Unit,
    private val onStatus: (String) -> Unit,
    private val onError: (String) -> Unit,
    private val capabilities: (() -> Wire.Capabilities?)? = null,
    private val onQuality: (MediaSession, Wire.HostCommand) -> Unit = { _, _ -> },
) : AutoCloseable {
    private val running = AtomicBoolean(false)
    private val active = AtomicReference<LanSession?>(null)
    private var server: ServerSocket? = null
    private var acceptThread: Thread? = null
    private val handshakeTimer = Executors.newSingleThreadScheduledExecutor { task ->
        Thread(task, "mwb-tls-handshake-deadline").apply { isDaemon = true }
    }
    private val handshakeWorkers = Executors.newFixedThreadPool(MAX_HANDSHAKES) { task ->
        Thread(task, "mwb-tls-handshake").apply { isDaemon = true }
    }
    private val handshakeSlots = Semaphore(MAX_HANDSHAKES)
    private val unauthenticated = Collections.newSetFromMap(ConcurrentHashMap<Socket, Boolean>())

    fun start() {
        if (!running.compareAndSet(false, true)) return
        try {
            val socket: ServerSocket = identity.listener(port)
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
            // Do not let one stalled TLS handshake monopolize the accept loop.
            // Only a fixed number of untrusted sockets may consume worker slots.
            if (active.get() != null || !handshakeSlots.tryAcquire()) {
                runCatching { socket.close() }
                continue
            }
            unauthenticated.add(socket)
            try {
                handshakeWorkers.execute { completeHandshake(socket) }
            } catch (_: java.util.concurrent.RejectedExecutionException) {
                unauthenticated.remove(socket)
                handshakeSlots.release()
                runCatching { socket.close() }
            }
        }
    }

    private fun completeHandshake(socket: Socket) {
        // Per-read SO_TIMEOUT is insufficient against a TLS record byte trickle.
        // This task closes the unauthenticated connection after an absolute deadline.
        val timeout = handshakeTimer.schedule(
            { runCatching { socket.close() } }, HANDSHAKE_TIMEOUT_MS, TimeUnit.MILLISECONDS
        )
        try {
            runCatching { authenticate(socket as SSLSocket) }
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
                    }, authorization, identity.fingerprint().chunked(2).map { it.toInt(16).toByte() }.toByteArray(),
                        onAuthorization, capabilities, onQuality)
                    // Parallel handshakes must never seize the same receiver slot.
                    if (!active.compareAndSet(null, session)) {
                        runCatching { socket.close() }
                        return@onSuccess
                    }
                    if (!running.get()) {
                        session.close()
                        return@onSuccess
                    }
                    session.start { onSession(session, true) }
                }
                .onFailure {
                    runCatching { socket.close() }
                    // Do not echo arbitrary peer-induced TLS exceptions into UI.
                    if (running.get()) onError("Wi-Fi client rejected (check pairing details)")
                }
        } finally {
            timeout.cancel(false)
            unauthenticated.remove(socket)
            handshakeSlots.release()
        }
    }

    private fun authenticate(socket: SSLSocket) {
        socket.tcpNoDelay = true
        socket.keepAlive = true
        socket.useClientMode = false
        socket.enabledProtocols = arrayOf("TLSv1.3")
        socket.soTimeout = HANDSHAKE_TIMEOUT_MS.toInt()
        socket.startHandshake()
        val (hello, credential) = readHelloWithDeadline(socket, HANDSHAKE_TIMEOUT_MS.toInt(), 16)
        require(credential.size == 16 && identity.authenticates(credential)) {
            "Wi-Fi pairing key rejected"
        }
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
        // Closing pending sockets wakes every worker that was blocked in TLS.
        unauthenticated.forEach { runCatching { it.close() } }
        handshakeWorkers.shutdownNow()
        acceptThread?.interrupt()
        runCatching { acceptThread?.join(500) }
        acceptThread = null
        server = null
        handshakeTimer.shutdownNow()
    }

    companion object {
        const val DEFAULT_PORT = 48_527
        private const val MAX_HANDSHAKES = 4
        private const val HANDSHAKE_TIMEOUT_MS = 5_000L

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
internal fun readHelloWithDeadline(
    socket: Socket,
    timeoutMillis: Int,
    credentialBytes: Int = 0,
): Pair<Wire.Header, ByteArray> {
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
    val (hello, payload) = Wire.readFrame(limitedInput, maxPayloadLength = credentialBytes)
    require(hello.type == Wire.TYPE_HELLO) { "expected HELLO" }
    require(payload.size == credentialBytes) { "invalid Wi-Fi HELLO credential length" }
    return hello to payload
}
