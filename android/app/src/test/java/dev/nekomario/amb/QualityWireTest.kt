package dev.nekomario.amb

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File

/**
 * The quality control wire contract. Both languages read the same golden
 * fixture (protocol/fixtures/quality-capabilities-v1.bin); this file pins the
 * Android side against it, exactly as the Rust wire test does.
 */
class QualityWireTest {
    private fun locateFixture(relative: String): File {
        var dir: File? = File("").absoluteFile
        repeat(5) {
            val candidate = File(dir, relative)
            if (candidate.isFile) return candidate
            dir = dir?.parentFile
        }
        throw AssertionError("fixture not found: $relative (searched up from ${File("").absolutePath})")
    }

    private fun fixtureCapabilities() = Wire.Capabilities(
        extensionVersion = 1,
        permission = Wire.PERMISSION_READY,
        catalogRevision = 7,
        streamEpoch = 42,
        modes = listOf(
            Wire.QualityMode(1280, 720, 30, 6_000_000),
            Wire.QualityMode(1920, 1080, 30, 13_500_000),
            Wire.QualityMode(1280, 720, 60, 12_000_000),
        ),
    )

    @Test
    fun sharedAndroidPcPreferenceFixture() {
        val caps = CaptureModes.Capabilities("fixture", "fixture",
            listOf(CaptureModes.SizeFps(1280, 720, 1_000_000_000L / 60), CaptureModes.SizeFps(1920, 1080, 1_000_000_000L / 30)),
            listOf(CaptureModes.FpsRange(15, 30), CaptureModes.FpsRange(30, 30), CaptureModes.FpsRange(60, 60)))
        val declared = CaptureModes.supported(caps).map { Triple(it.width, it.height, it.targetFps) }.toSet()
        assertEquals(fixtureCapabilities().modes.map { Triple(it.width, it.height, it.targetFps) }.toSet(), declared)
        locateFixture("protocol/fixtures/quality-preferences-v1.txt").readLines()
            .filter { !it.startsWith("#") && it.isNotBlank() }.forEach { line ->
                val halves = line.split(" -> "); require(halves.size == 2)
                fun tuple(text: String) = text.split(Regex("\\s+")).map(String::toInt).also { require(it.size == 3) }
                val wish = tuple(halves[0])
                // Fixture includes 720p60. That is a device mode, above the OSS cap.
                val result = CaptureModes.resolve(
                    caps,
                    CaptureModes.CapturePreference(wish[0].takeIf { it != 0 }, wish[1].takeIf { it != 0 }, wish[2].takeIf { it != 0 }),
                    EditionPolicy.Uncapped,
                )
                if (halves[1] == "rejected") org.junit.Assert.assertTrue(result is CaptureModes.Resolution.Rejected)
                else {
                    val mode = (result as CaptureModes.Resolution.Resolved).mode
                    assertEquals(line, tuple(halves[1]), listOf(mode.width, mode.height, mode.targetFps))
                }
            }
    }

@Test
    fun clientAuthFramesMatchTheGoldenFixture() {
        val fixture = locateFixture("protocol/fixtures/client-auth-v2.bin").readBytes()
        var offset = 0
        fun frame(): Pair<Wire.Header, ByteArray> {
            val (header, payload) = Wire.readFrame(fixture.inputStream(offset, fixture.size))
            offset += Wire.HEADER_SIZE + payload.size
            return header to payload
        }

        val (challengeHeader, challengePayload) = frame()
        assertEquals(Wire.TYPE_CLIENT_CHALLENGE, challengeHeader.type)
        val challenge = Wire.decodeClientChallenge(challengePayload)
        assertEquals(Wire.CLIENT_AUTH_VERSION, challenge.version)
        assertEquals(Wire.AUTH_LAN, challenge.transport)
        assertArrayEquals(ClientCryptoFixture.phoneBinding(), challenge.phoneBinding)
        assertEquals(ByteArray(32) { it.toByte() }.toList(), challenge.nonce.toList())
        val challengeEncoded = Wire.encodeClientChallenge(challenge)
        val challengeOut = ByteArrayOutputStream()
        Wire.writeFrame(challengeOut, challengeHeader.copy(payloadLength = challengeEncoded.size), challengeEncoded)
        assertArrayEquals(
            fixture.copyOfRange(offset - challengeOut.size(), offset),
            challengeOut.toByteArray(),
        )

        val (authHeader, authPayload) = frame()
        assertEquals(Wire.TYPE_CLIENT_AUTH, authHeader.type)
        val auth = Wire.decodeClientAuth(authPayload)
        assertEquals(Wire.CLIENT_AUTH_VERSION, auth.version)
        assertArrayEquals(locateFixture("protocol/fixtures/client-signature-v1.bin").readBytes().copyOfRange(0, 65), auth.publicKey)
        assertEquals("studio-pc", auth.label)
        assertEquals(Wire.SIGNATURE_BYTES, auth.signature.size)
        val authEncoded = Wire.encodeClientAuth(auth)
        val authOut = ByteArrayOutputStream()
        Wire.writeFrame(authOut, authHeader.copy(payloadLength = authEncoded.size), authEncoded)
        assertArrayEquals(fixture.copyOfRange(offset - authOut.size(), offset), authOut.toByteArray())

        val (resultHeader, resultPayload) = frame()
        assertEquals(Wire.TYPE_CLIENT_AUTH_RESULT, resultHeader.type)
        val result = Wire.decodeClientAuthResult(resultPayload)
        assertEquals(Wire.AUTH_AUTHORIZED, result.status)
        assertEquals(Wire.AUTH_REASON_NONE, result.reason)
        assertEquals(1_800_000_000L, result.expiresAtUnixSeconds)
        val resultEncoded = Wire.encodeClientAuthResult(result)
        val resultOut = ByteArrayOutputStream()
        Wire.writeFrame(resultOut, resultHeader.copy(payloadLength = resultEncoded.size), resultEncoded)
        assertArrayEquals(
            fixture.copyOfRange(offset - resultOut.size(), offset),
            resultOut.toByteArray(),
        )

        assertEquals(fixture.size, offset)
    }

    @Test
    fun clientAuthLabelLengthIsValidatedBeforeAllocation() {
        val payload = Wire.encodeClientAuth(
            Wire.ClientAuth(Wire.CLIENT_AUTH_VERSION, ByteArray(Wire.CLIENT_PUBLIC_KEY_BYTES).also { it[0] = 4 }, "ok", ByteArray(Wire.SIGNATURE_BYTES)),
        )
        // A label count far longer than the payload carries is refused, and a
        // count that fits the ceiling but not the payload is refused too.
        val overLimit = payload.copyOf()
        overLimit[1 + Wire.CLIENT_PUBLIC_KEY_BYTES] = 200.toByte()
        assertThrows(IllegalArgumentException::class.java) { Wire.decodeClientAuth(overLimit) }
        val short = payload.copyOf()
        short[1 + Wire.CLIENT_PUBLIC_KEY_BYTES] = Wire.MAX_CLIENT_LABEL.toByte()
        assertThrows(IllegalArgumentException::class.java) { Wire.decodeClientAuth(short) }
    }

    @Test
    fun the_domain_separated_transcript_matches_the_shared_bytes() {
        val key = locateFixture("protocol/fixtures/client-signature-v1.bin").readBytes().copyOfRange(0, 65)
        val expected = locateFixture("protocol/fixtures/client-auth-transcript-v2.bin").readBytes()
        assertArrayEquals(expected, Wire.clientAuthMessage(Wire.AUTH_LAN, ClientCryptoFixture.phoneBinding(), ByteArray(32) { it.toByte() }, key, "studio-pc"))
        assertArrayEquals(locateFixture("protocol/fixtures/client-auth-transcript-usb-v2.bin").readBytes(),
            Wire.clientAuthMessage(Wire.AUTH_USB, ByteArray(32), ByteArray(32) { it.toByte() }, key, "studio-pc"))
    }

    @Test
    fun auth_labels_use_strict_utf8_and_outcomes_reject_invalid_combinations() {
        val good = Wire.encodeClientAuth(Wire.ClientAuth(Wire.CLIENT_AUTH_VERSION,
            ByteArray(65).also { it[0] = 4 }, "ok", ByteArray(64)))
        val badUtf8 = good.copyOf().also { it[67] = 0xc0.toByte(); it[68] = 0xaf.toByte() }
        assertThrows(Exception::class.java) { Wire.decodeClientAuth(badUtf8) }
        for (payload in listOf(byteArrayOf(3, 0, 0, 0, 0, 0), byteArrayOf(0, 5, 1, 0, 0, 0),
            byteArrayOf(1, 0, 1, 0, 0, 0), byteArrayOf(2, 0, 0, 0, 0, 0), byteArrayOf(0, 0, 0, 0, 0, 0))) {
            assertThrows(IllegalArgumentException::class.java) { Wire.decodeClientAuthResult(payload) }
        }
    }

    @Test
    fun auth_length_bounds_fail_from_the_header_before_payload_io() {
        for (length in listOf(91, Wire.CLIENT_AUTH_MIN_BYTES - 1, Wire.CLIENT_AUTH_MAX_BYTES + 1, 1_000_000)) {
            val out = ByteArrayOutputStream()
            Wire.writeFrame(out, Wire.Header(Wire.TYPE_CLIENT_AUTH, 0, 1, length, 0), ByteArray(length))
            assertThrows(IllegalArgumentException::class.java) {
                Wire.readHostFrame(out.toByteArray().copyOf(Wire.HEADER_SIZE).inputStream())
            }
        }
    }

    @Test
    fun qualityCapabilitiesMatchTheGoldenFixture() {
        val fixture = locateFixture("protocol/fixtures/quality-capabilities-v1.bin").readBytes()
        val (header, payload) = Wire.readFrame(ByteArrayInputStream(fixture))
        assertEquals(Wire.TYPE_QUALITY_CAPABILITIES, header.type)
        val capabilities = Wire.decodeCapabilities(payload)
        assertEquals(fixtureCapabilities(), capabilities)
        // Re-encoding the decoded values reproduces the fixture bytes.
        val encoded = Wire.encodeCapabilities(capabilities)
        val output = ByteArrayOutputStream()
        Wire.writeFrame(output, header.copy(payloadLength = encoded.size), encoded)
        assertArrayEquals(fixture, output.toByteArray())
    }

    @Test
    fun qualityPayloadLengthIsValidatedBeforeAllocation() {
        val payload = Wire.encodeCapabilities(fixtureCapabilities())
        // A count that does not match the payload is rejected before the list
        // is built; an over-limit count is never silently truncated.
        val truncated = payload.copyOf(payload.size - Wire.QUALITY_MODE_BYTES)
        assertThrows(IllegalArgumentException::class.java) { Wire.decodeCapabilities(truncated) }
        val overLimit = payload.copyOf()
        overLimit[8] = 1 // mode count low byte: 3 + 256 overflows into the high byte
        overLimit[9] = 1
        assertThrows(IllegalArgumentException::class.java) { Wire.decodeCapabilities(overLimit) }
        assertThrows(IllegalArgumentException::class.java) { Wire.decodeCapabilities(payload.copyOf(4)) }
    }

    @Test
    fun maximumModeListIsAcceptedAndEncoded() {
        val modes = (0 until Wire.MAX_QUALITY_MODES).map {
            Wire.QualityMode(640 + (it % 32) * 2, 480 + (it % 16) * 2, 30, 1_000_000)
        }
        val capabilities = fixtureCapabilities().copy(modes = modes)
        val payload = Wire.encodeCapabilities(capabilities)
        assertEquals(Wire.decodeCapabilities(payload), capabilities)
    }

    @Test
    fun overLimitModeListIsRefusedNotTruncated() {
        val modes = (0..Wire.MAX_QUALITY_MODES).map { Wire.QualityMode(1280, 720, 30, 6_000_000) }
        val capabilities = fixtureCapabilities().copy(modes = modes)
        assertThrows(IllegalArgumentException::class.java) { Wire.encodeCapabilities(capabilities) }
    }

    @Test
    fun qualityControlFramesRoundTrip() {
        val prepare = Wire.PrepareQuality(requestId = 9, catalogRevision = 7, width = 1920, height = 1080, targetFps = 30)
        val encodedPrepare = Wire.encodePrepareQuality(prepare)
        assertEquals(14, encodedPrepare.size)
        assertEquals(prepare, Wire.decodePrepareQuality(encodedPrepare))

        val result = Wire.QualityResult(
            requestId = 9,
            status = Wire.RESULT_APPLIED,
            reason = Wire.REASON_NONE,
            appliedWidth = 1920,
            appliedHeight = 1080,
            appliedFps = 30,
        )
        val encodedResult = Wire.encodeQualityResult(result)
        assertEquals(14, encodedResult.size)
        assertEquals(result, Wire.decodeQualityResult(encodedResult))

        for (epoch in listOf(Wire.Epoch(9, 43), Wire.Epoch(9, 42))) {
            assertEquals(epoch, Wire.decodeEpoch(Wire.encodeEpoch(epoch)))
        }
        assertThrows(IllegalArgumentException::class.java) { Wire.decodeEpoch(ByteArray(7)) }
    }
}
