package dev.nekomario.amb

import org.junit.Assert.*
import org.junit.Test
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicInteger

class AuthorizedMediaSessionTest {
    private fun send(client: Socket, type: Byte, payload: ByteArray) = Wire.writeFrame(client.getOutputStream(),
        Wire.Header(type, 0, 1, payload.size, 0), payload)
    private fun read(client: Socket, type: Byte): ByteArray {
        val frame = Wire.readFrame(client.getInputStream())
        assertEquals(type, frame.first.type)
        return frame.second
    }
    private fun proof(client: Socket, key: Pair<ByteArray, java.security.PrivateKey>): ByteArray {
        val challenge = Wire.decodeClientChallenge(read(client, Wire.TYPE_CLIENT_CHALLENGE))
        val signature = ClientCryptoFixture.sign(key.second,
            Wire.clientAuthMessage(Wire.AUTH_LAN, ClientCryptoFixture.phoneBinding(), challenge.nonce, key.first, "test-pc"))
        return Wire.encodeClientAuth(Wire.ClientAuth(Wire.CLIENT_AUTH_VERSION, key.first, "test-pc", signature))
    }
    private fun eof(client: Socket) {
        try { assertEquals("unauthorized/revoked socket sent bytes or remained open", -1, client.getInputStream().read()) }
        catch (_: java.net.SocketException) {} // terminal reset is also a close, not a timeout
    }

    @Test fun closing_a_session_before_start_closes_its_socket_and_cannot_reopen_it() {
        PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = { fail("unexpected storage failure") }).use { owner ->
            ServerSocket(0).use { listener ->
                Socket("127.0.0.1", listener.localPort).use { client ->
                    client.soTimeout = 200
                    LanSession(listener.accept(), { _, _ -> }, owner, ClientCryptoFixture.phoneBinding(), {}).use { session ->
                        session.close()
                        eof(client)
                        session.close()
                        session.start { fail("a closed session was restarted") }
                        assertFalse(session.isActive())
                        assertNull(session.clientAuthorization)
                    }
                }
            }
        }
    }

    @Test fun real_reader_rejects_every_control_before_authorization_and_capture_cannot_begin() {
        for ((type, size) in listOf(Wire.TYPE_PING to 8, Wire.TYPE_VIDEO_IDR_REQUEST to 0,
            Wire.TYPE_PREPARE_QUALITY to 14, Wire.TYPE_COMMIT_QUALITY to 8)) {
            val calls = AtomicInteger()
            PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = { fail("unexpected storage failure") }).use { owner ->
                ServerSocket(0).use { listener ->
                    Socket("127.0.0.1", listener.localPort).use { client ->
                        client.soTimeout = 2000
                        LanSession(listener.accept(), { _, _ -> }, owner, ClientCryptoFixture.phoneBinding(), {},
                            capabilities = { Wire.Capabilities(1, 0, 1, 0, emptyList()) },
                            onQuality = { _, _ -> calls.incrementAndGet() }).use { session ->
                            session.start()
                            read(client, Wire.TYPE_CLIENT_CHALLENGE)
                            assertThrows(IllegalStateException::class.java) { session.beginCapture() }
                            send(client, type, ByteArray(size))
                            eof(client)
                            assertEquals(0, calls.get())
                        }
                    }
                }
            }
        }
    }

    @Test fun real_writer_withholds_capabilities_until_saved_approval_and_stops_output_on_revoke() {
        val key = ClientCryptoFixture.keyPair()
        var disk: ByteArray? = null
        val repository = ClientRepository({ disk }, { disk = it.copyOf() })
        PhoneAuthorization(repository, onStorageFailure = { fail("unexpected storage failure") }).use { owner ->
            ServerSocket(0).use { listener ->
                Socket("127.0.0.1", listener.localPort).use { client ->
                    client.soTimeout = 2000
                    LanSession(listener.accept(), { _, _ -> }, owner, ClientCryptoFixture.phoneBinding(), {},
                        capabilities = { Wire.Capabilities(1, 0, 1, 0, emptyList()) }).use { session ->
                        session.start()
                        send(client, Wire.TYPE_CLIENT_AUTH, proof(client, key))
                        assertEquals(Wire.AUTH_PENDING, Wire.decodeClientAuthResult(read(client, Wire.TYPE_CLIENT_AUTH_RESULT)).status)
                        assertNull(disk)
                        assertThrows(IllegalStateException::class.java) { session.beginCapture() }
                        checkNotNull(session.clientAuthorization).approve()
                        assertNotNull(disk)
                        assertEquals(Wire.AUTH_AUTHORIZED, Wire.decodeClientAuthResult(read(client, Wire.TYPE_CLIENT_AUTH_RESULT)).status)
                        read(client, Wire.TYPE_QUALITY_CAPABILITIES)
                        val generation = session.beginCapture()
                        session.onVideoConfig(64, 32, 30, 100000, generation)
                        val config = java.nio.ByteBuffer.wrap(read(client, Wire.TYPE_VIDEO_CONFIG))
                            .order(java.nio.ByteOrder.LITTLE_ENDIAN)
                        assertEquals(64, config.short.toInt()); assertEquals(32, config.short.toInt())
                        assertEquals(30, config.short.toInt()); assertEquals(0, config.short.toInt())
                        assertEquals(100000, config.int)
                        session.onAccessUnit(H264Encoder.AccessUnit(byteArrayOf(1), 0, true, false), generation)
                        assertArrayEquals(byteArrayOf(1), read(client, Wire.TYPE_VIDEO_AU))
                        owner.revoke(ClientAuthorization.deriveClientId(key.first))
                        session.onAccessUnit(H264Encoder.AccessUnit(byteArrayOf(2), 1, true, false), generation)
                        assertThrows(IllegalStateException::class.java) { session.beginCapture() }
                        eof(client)
                        assertTrue(ClientRecordCodec.decode(checkNotNull(disk)).isEmpty())
                    }
                }
            }
        }
    }

    @Test fun actual_socket_closes_without_a_grant_if_saving_approval_fails() {
        val key = ClientCryptoFixture.keyPair()
        val failures = AtomicInteger()
        val repository = ClientRepository({ null }, { throw java.io.IOException("disk full") })
        PhoneAuthorization(repository, onStorageFailure = { failures.incrementAndGet() }).use { owner ->
            ServerSocket(0).use { listener ->
                Socket("127.0.0.1", listener.localPort).use { client ->
                    client.soTimeout = 2000
                    LanSession(listener.accept(), { _, _ -> }, owner, ClientCryptoFixture.phoneBinding(), {}).use { session ->
                        session.start()
                        send(client, Wire.TYPE_CLIENT_AUTH, proof(client, key))
                        assertEquals(Wire.AUTH_PENDING, Wire.decodeClientAuthResult(read(client, Wire.TYPE_CLIENT_AUTH_RESULT)).status)
                        assertThrows(IllegalStateException::class.java) { checkNotNull(session.clientAuthorization).approve() }
                        eof(client)
                        assertEquals(1, failures.get())
                        assertFalse(checkNotNull(session.clientAuthorization).isAuthorized())
                    }
                }
            }
        }
    }
}
