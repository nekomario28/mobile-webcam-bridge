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
                val result = CaptureModes.resolve(caps, CaptureModes.CapturePreference(wish[0].takeIf { it != 0 }, wish[1].takeIf { it != 0 }, wish[2].takeIf { it != 0 }))
                if (halves[1] == "rejected") org.junit.Assert.assertTrue(result is CaptureModes.Resolution.Rejected)
                else {
                    val mode = (result as CaptureModes.Resolution.Resolved).mode
                    assertEquals(line, tuple(halves[1]), listOf(mode.width, mode.height, mode.targetFps))
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
