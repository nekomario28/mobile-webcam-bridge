package dev.nekomario.amb

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.OutputStream
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketTimeoutException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

class WireAndQueueTest {
    @Test
    fun controlFrameLimitRejectsBeforePayloadAllocation() {
        val output = ByteArrayOutputStream()
        Wire.writeFrame(
            output,
            Wire.Header(Wire.TYPE_PING, 0, 1, 1024, 0),
            ByteArray(1024),
        )
        // The header alone must be rejected before reading or allocating payload.
        val headerOnly = output.toByteArray().copyOfRange(0, Wire.HEADER_SIZE)
        assertThrows(IllegalArgumentException::class.java) {
            Wire.readFrame(ByteArrayInputStream(headerOnly), maxPayloadLength = 8)
        }
        assertThrows(IllegalArgumentException::class.java) {
            Wire.readFrame(ByteArrayInputStream(headerOnly), maxPayloadLength = 0)
        }
    }

    @Test
    fun pairingCredentialIsExactly128Bits() {
        val code = "00112233445566778899aabbccddeeff"
        assertTrue(SecureLanIdentity.isHex(code, 32))
        assertEquals(16, SecureLanIdentity.decodeHex(code).size)
        assertTrue(!SecureLanIdentity.isHex(code.dropLast(2), 32))
        assertThrows(IllegalArgumentException::class.java) {
            SecureLanIdentity.decodeHex("invalid-pairing-credential")
        }
    }

    @Test
    fun eightByteControlFrameStillRoundTrips() {
        val payload = byteArrayOf(1, 2, 3, 4, 5, 6, 7, 8)
        val output = ByteArrayOutputStream()
        Wire.writeFrame(output, Wire.Header(Wire.TYPE_PING, 0, 42, 8, 0), payload)
        val (header, decoded) = Wire.readFrame(ByteArrayInputStream(output.toByteArray()), 8)
        assertEquals(Wire.TYPE_PING, header.type)
        assertEquals(42, header.sequence)
        assertArrayEquals(payload, decoded)
    }

    @Test
    fun aByteAtATimeHelloCannotExtendTheDeadline() {
        ServerSocket(0).use { listener ->
            val sender = Thread {
                runCatching {
                    Socket("127.0.0.1", listener.localPort).use { socket ->
                        repeat(20) {
                            socket.getOutputStream().write('A'.code)
                            Thread.sleep(40)
                        }
                    }
                }
            }
            sender.start()
            try {
                listener.accept().use { socket ->
                    val began = System.nanoTime()
                    assertThrows(SocketTimeoutException::class.java) {
                        readHelloWithDeadline(socket, 200)
                    }
                    val elapsedMs = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - began)
                    assertTrue("deadline took $elapsedMs ms", elapsedMs < 1500)
                }
            } finally {
                sender.join(1500)
            }
        }
    }

    @Test
    fun blockedWriterDoesNotAccumulateUnlimitedControlFrames() {
        val blocked = CountDownLatch(1)
        val release = CountDownLatch(1)
        val failures = AtomicInteger()
        val output = object : OutputStream() {
            override fun write(b: Int) {
                blocked.countDown()
                release.await(5, TimeUnit.SECONDS)
            }
            override fun write(b: ByteArray, off: Int, len: Int) {
                blocked.countDown()
                release.await(5, TimeUnit.SECONDS)
            }
        }
        val writer = FramedOutbound(output, onNeedIdr = {}, onError = { failures.incrementAndGet() })
        try {
            writer.start()
            val request = Wire.Header(Wire.TYPE_PING, 0, 1, 8, 0)
            writer.sendPong(request, ByteArray(8))
            assertTrue(blocked.await(2, TimeUnit.SECONDS))
            repeat(100) { writer.sendPong(request, ByteArray(8)) }
            assertEquals(1, failures.get())
        } finally {
            release.countDown()
            writer.close()
        }
    }
}
