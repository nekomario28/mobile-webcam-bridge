package dev.nekomario.amb

import dev.nekomario.amb.ClientCryptoFixture.keyPair
import dev.nekomario.amb.ClientCryptoFixture.sign
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketTimeoutException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * The two halves the 2026-10-24 review found guarded only at the method level.
 *
 * Its WR1/WR4 note: `a_host_hello_ends_a_session_that_is_waiting_for_a_proof` calls
 * `onHostHello()` directly and nothing in the suite sends `0x0e` through `readMedia`, so
 * ID-001's *routing* was read rather than tested. Its ID-003 note: the ungated write was
 * measured, but no test drove the refusal.
 *
 * Both tests below write real frames into a real socket and let the session's own reader
 * thread call `readMedia`, so deleting either gate breaks them — which is the property the
 * method-level tests do not have.
 */
class HostHelloAndPairingGateWireTest {

    private class ProbeSession(socket: Socket, owner: PhoneAuthorization) : AuthorizedMediaSession(
        socket.getOutputStream(),
        owner,
        Wire.AUTH_USB,
        ByteArray(Wire.PHONE_BINDING_BYTES),
        capabilities = {
            Wire.Capabilities(1, Wire.PERMISSION_READY, 1, 0,
                listOf(Wire.QualityMode(1280, 720, 30, 6_000_000)))
        },
        onQuality = { _, _ -> },
        onAuthorization = {},
        onCaptureCancelled = {},
        threadName = "probe-writer",
    ) {
        private val running = AtomicBoolean(false)
        private val input = socket.getInputStream()
        private var reader: Thread? = null
        val ended = CountDownLatch(1)

        @Volatile var endReason: String? = null

        fun start() {
            if (!running.compareAndSet(false, true)) return
            startMedia()
            reader = Thread({
                try {
                    while (running.get()) readMedia(input)
                } catch (_: Exception) {
                    // EOF, a socket timeout and a refused frame all end the reader here.
                } finally {
                    closeMedia()
                }
            }, "probe-reader").also { it.start() }
        }

        override fun failMedia(reason: String) {
            endReason = reason
            running.set(false)
            ended.countDown()
            runCatching { closeMedia() }
        }

        override fun close() {
            running.set(false)
            reader?.interrupt()
            closeMedia()
        }
    }

    private class Link(
        val client: Socket,
        val session: ProbeSession,
        val owner: PhoneAuthorization,
        val challenge: Wire.ClientChallenge,
    ) : AutoCloseable {
        override fun close() {
            runCatching { session.close() }
            runCatching { owner.close() }
            runCatching { client.close() }
        }
    }

    /** A live USB session that has sent its one challenge and is waiting for a proof. */
    private fun link(): Link {
        val owner = PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = {
            error("fixture storage failed")
        })
        val listener = ServerSocket(0)
        val client = Socket("127.0.0.1", listener.localPort).apply { soTimeout = 4_000 }
        val session = ProbeSession(listener.accept(), owner)
        listener.close()
        session.start()
        val (header, bytes) = Wire.readFrame(client.getInputStream())
        assertEquals(Wire.TYPE_CLIENT_CHALLENGE, header.type)
        return Link(client, session, owner, Wire.decodeClientChallenge(bytes))
    }

    private fun sendProof(link: Link) {
        val (publicKey, privateKey) = keyPair()
        val label = "probe-pc"
        val message = Wire.clientAuthMessage(
            Wire.AUTH_USB, ByteArray(Wire.PHONE_BINDING_BYTES), link.challenge.nonce, publicKey, label,
        )
        val proof = Wire.encodeClientAuth(
            Wire.ClientAuth(Wire.CLIENT_AUTH_VERSION, publicKey, label, sign(privateKey, message)),
        )
        Wire.writeFrame(
            link.client.getOutputStream(),
            Wire.Header(Wire.TYPE_CLIENT_AUTH, 0, 1, proof.size, 0),
            proof,
        )
    }

    private fun awaitResult(link: Link): Wire.ClientAuthResult {
        val (header, bytes) = Wire.readFrame(link.client.getInputStream())
        assertEquals(Wire.TYPE_CLIENT_AUTH_RESULT, header.type)
        return Wire.decodeClientAuthResult(bytes)
    }

    /** What the peer sees within the timeout: a frame, silence, or a closed socket. */
    private fun listen(link: Link, timeoutMs: Int = 800): String {
        link.client.soTimeout = timeoutMs
        return try {
            "a frame of type 0x%02x".format(Wire.readFrame(link.client.getInputStream()).first.type)
        } catch (_: SocketTimeoutException) {
            "silence"
        } catch (_: java.io.IOException) {
            "closed"
        }
    }

    @Test
    fun a_host_hello_on_the_wire_ends_the_session_instead_of_being_answered() {
        link().use { link ->
            // The phone has sent its one challenge; a reconnecting host says hello.
            Wire.writeFrame(
                link.client.getOutputStream(),
                Wire.Header(Wire.TYPE_HOST_HELLO, 0, 1, 0, 0),
                ByteArray(0),
            )
            assertTrue(
                "a hello during PROOF must end the session",
                link.session.ended.await(4, TimeUnit.SECONDS),
            )
            assertTrue(
                "the end must name the refused proof, not an expired deadline: " +
                    link.session.endReason,
                link.session.endReason.orEmpty().contains("rejected"),
            )
            // The old unreachable branch would have re-sent a challenge here.
            assertEquals(
                "a hello must end the session, not answer it with a fresh challenge",
                "silence",
                listen(link),
            )
            assertFalse(
                "a hello is not a credential",
                checkNotNull(link.session.clientAuthorization).isAuthorized(),
            )
        }
    }

    @Test
    fun the_pairing_handoff_writes_nothing_until_the_pc_is_approved() {
        link().use { link ->
            sendProof(link)
            assertEquals(Wire.AUTH_PENDING, awaitResult(link).status)

            val payload = Wire.encodePairingInfo(listOf("192.168.1.3"), "ab".repeat(32), "cd".repeat(16))
            link.session.sendPairingInfo(payload)
            assertEquals(
                "an unauthorized hand-off must not reach the wire (ID-003)",
                "silence",
                listen(link),
            )

            checkNotNull(link.session.clientAuthorization).approve()
            assertEquals(Wire.AUTH_AUTHORIZED, awaitResult(link).status)
            val capabilities = Wire.readFrame(link.client.getInputStream())
            assertEquals(Wire.TYPE_QUALITY_CAPABILITIES, capabilities.first.type)

            // The same call, one approval later, must reach the wire: the gate is what
            // suppressed it above, not a missing sink.
            link.session.sendPairingInfo(payload)
            val (header, bytes) = Wire.readFrame(link.client.getInputStream())
            assertEquals(Wire.TYPE_PAIRING_INFO, header.type)
            assertArrayEquals(payload, bytes)
        }
    }

    @Test
    fun no_capabilities_reach_the_wire_before_the_authorization_result() {
        link().use { link ->
            // Issue #78, item 1: the phone answered a proof with capabilities instead of
            // an outcome. Both sends below are queued for a session that has not sent a
            // result yet, so the wire must stay quiet.
            link.session.updateCapabilities()
            assertEquals("nothing may precede the authorization result", "silence", listen(link))

            sendProof(link)
            assertEquals(Wire.AUTH_PENDING, awaitResult(link).status)
            link.session.updateCapabilities()
            assertEquals("pending is not authorized either", "silence", listen(link))

            checkNotNull(link.session.clientAuthorization).approve()
            assertEquals(Wire.AUTH_AUTHORIZED, awaitResult(link).status)
            assertEquals(
                "once authorized the advertisement is expected",
                Wire.TYPE_QUALITY_CAPABILITIES,
                Wire.readFrame(link.client.getInputStream()).first.type,
            )
        }
    }
}
