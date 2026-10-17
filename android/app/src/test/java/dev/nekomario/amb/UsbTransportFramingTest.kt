package dev.nekomario.amb

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.net.ServerSocket
import java.net.Socket
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.KeyPairGenerator
import java.security.Signature
import java.security.spec.ECGenParameterSpec
import java.util.concurrent.atomic.AtomicBoolean

/**
 * USB/AOA and Wi-Fi share the AMB1 boundary in [AuthorizedMediaSession]. This pins
 * the frames themselves on the USB transport, so neither the capability
 * advertisement nor the authorization gate can change AOA framing: challenge,
 * proof, result, capabilities, then the video config and access-unit headers.
 */
class UsbTransportFramingTest {
    private class UsbSession(
        socket: Socket,
        authorization: PhoneAuthorization,
    ) : AuthorizedMediaSession(
        socket.getOutputStream(),
        authorization,
        Wire.AUTH_USB,
        ByteArray(Wire.PHONE_BINDING_BYTES),
        capabilities = {
            Wire.Capabilities(1, Wire.PERMISSION_READY, 1, 0,
                listOf(Wire.QualityMode(1280, 720, 30, 6_000_000)))
        },
        onQuality = { _, _ -> },
        onAuthorization = {},
        onCaptureCancelled = {},
        threadName = "test-usb-writer",
    ) {
        private val running = AtomicBoolean(false)
        private val input = socket.getInputStream()
        private var reader: Thread? = null

        fun start() {
            if (!running.compareAndSet(false, true)) return
            startMedia()
            reader = Thread({
                try {
                    while (running.get()) readMedia(input)
                } catch (_: Exception) {
                    // EOF and a reset both end the session here.
                } finally {
                    closeMedia()
                }
            }, "test-usb-reader").also { it.start() }
        }

        override fun failMedia(reason: String) {
            running.set(false)
            runCatching { closeMedia() }
        }

        override fun close() {
            running.set(false)
            reader?.interrupt()
            closeMedia()
        }
    }

    private fun read(client: Socket, type: Byte): Pair<Wire.Header, ByteArray> {
        val frame = Wire.readFrame(client.getInputStream())
        assertEquals(type, frame.first.type)
        return frame
    }

    /** DER ECDSA from the platform, as the raw r||s the wire carries. */
    private fun rawSignature(der: ByteArray): ByteArray {
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
        return output
    }

    @Test
    fun usbFramingIsUnchangedByTheCapabilityAdvertisement() {
        val generator = KeyPairGenerator.getInstance("EC")
        generator.initialize(ECGenParameterSpec("secp256r1"))
        val keys = generator.generateKeyPair()
        val encoded = keys.public.encoded
        val point = encoded.copyOfRange(encoded.size - ClientAuthorization.PUBLIC_KEY_BYTES, encoded.size)

        val repository = ClientRepository({ null }, {})
        PhoneAuthorization(repository, onStorageFailure = { fail("unexpected storage failure") })
            .use { owner ->
                ServerSocket(0).use { listener ->
                    Socket("127.0.0.1", listener.localPort).use { client ->
                        client.soTimeout = 4000
                        UsbSession(listener.accept(), owner).use { session ->
                            session.start()
                            // The challenge carries the USB transport and the zero
                            // binding USB deliberately uses (no phone attestation).
                            val challenge = Wire.decodeClientChallenge(
                                read(client, Wire.TYPE_CLIENT_CHALLENGE).second,
                            )
                            assertEquals(Wire.AUTH_USB, challenge.transport)
                            assertArrayEquals(
                                ByteArray(Wire.PHONE_BINDING_BYTES),
                                challenge.phoneBinding,
                            )
                            assertEquals(Wire.NONCE_BYTES, challenge.nonce.size)

                            val message = Wire.clientAuthMessage(
                                Wire.AUTH_USB,
                                ByteArray(Wire.PHONE_BINDING_BYTES),
                                challenge.nonce,
                                point,
                                "usb-pc",
                            )
                            val signature = Signature.getInstance("SHA256withECDSA").run {
                                initSign(keys.private)
                                update(message)
                                rawSignature(sign())
                            }
                            val proof = Wire.encodeClientAuth(
                                Wire.ClientAuth(Wire.CLIENT_AUTH_VERSION, point, "usb-pc", signature),
                            )
                            Wire.writeFrame(
                                client.getOutputStream(),
                                Wire.Header(Wire.TYPE_CLIENT_AUTH, 0, 1, proof.size, 0),
                                proof,
                            )

                            val pending = Wire.decodeClientAuthResult(
                                read(client, Wire.TYPE_CLIENT_AUTH_RESULT).second,
                            )
                            assertEquals(Wire.AUTH_PENDING, pending.status)
                            assertFalse(
                                "USB must not be authorized before approval",
                                checkNotNull(session.clientAuthorization).isAuthorized(),
                            )
                            checkNotNull(session.clientAuthorization).approve()
                            val granted = Wire.decodeClientAuthResult(
                                read(client, Wire.TYPE_CLIENT_AUTH_RESULT).second,
                            )
                            assertEquals(Wire.AUTH_AUTHORIZED, granted.status)

                            // Framing after the capability advertisement.
                            assertEquals(24, Wire.HEADER_SIZE)
                            val capabilities = read(client, Wire.TYPE_QUALITY_CAPABILITIES)
                            assertTrue(
                                "the advertisement must reach the wire after approval",
                                capabilities.second.isNotEmpty(),
                            )

                            val generation = session.beginCapture()
                            session.onVideoConfig(1280, 720, 30, 6_000_000, generation)
                            val config = read(client, Wire.TYPE_VIDEO_CONFIG)
                            assertEquals(16, config.second.size)
                            val fields = ByteBuffer.wrap(config.second).order(ByteOrder.LITTLE_ENDIAN)
                            assertEquals(1280, fields.short.toInt())
                            assertEquals(720, fields.short.toInt())
                            assertEquals(30, fields.short.toInt())
                            assertEquals(0, fields.short.toInt())
                            assertEquals(6_000_000, fields.int)

                            session.onAccessUnit(
                                H264Encoder.AccessUnit(byteArrayOf(1, 2, 3), 42_000, true, false),
                                generation,
                            )
                            val unit = read(client, Wire.TYPE_VIDEO_AU)
                            assertArrayEquals(byteArrayOf(1, 2, 3), unit.second)
                            assertEquals(42_000, unit.first.ptsUs)
                            assertTrue(
                                "the keyframe flag must survive framing",
                                unit.first.flags.toInt() and Wire.VIDEO_FLAG_KEYFRAME.toInt() != 0,
                            )
                        }
                    }
                }
            }
    }
}
