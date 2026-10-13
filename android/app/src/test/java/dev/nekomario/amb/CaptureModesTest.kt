package dev.nekomario.amb

private typealias FpsRange = CaptureModes.FpsRange

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Capability-fixture tests for the capture-mode resolver: no device, no PC
 * output. Pins the no-silent-substitution rule and the regression where the
 * existence of a size and an fps does not create their combination.
 */
class CaptureModesTest {
    private fun caps(
        sizes: List<CaptureModes.SizeFps>,
        aeRanges: List<Range<Int>>,
        bitrateRange: IntRange = 100_000..100_000_000,
        widthAlignment: Int = 2,
        heightAlignment: Int = 2,
        cameraId: String = "0",
        codecName: String = "c2.android.avc.encoder",
    ) = CaptureModes.Capabilities(
        cameraId = cameraId,
        codecName = codecName,
        sizes = sizes,
        aeRanges = aeRanges,
        widthAlignment = widthAlignment,
        heightAlignment = heightAlignment,
        bitrateRange = bitrateRange,
    )

    private fun size(width: Int, height: Int, minFpsCap: Int? = null) =
        CaptureModes.SizeFps(
            width,
            height,
            minFpsCap?.let { 1_000_000_000L / it },
        )

    /** A 30-only device: 720p and 1080p, AE ranges 15–30 and fixed 30. */
    private fun fps30Only() = caps(
        sizes = listOf(size(1280, 720), size(1920, 1080)),
        aeRanges = listOf(FpsRange(15, 30), FpsRange(30, 30)),
    )

    /** A device whose 720p reaches 60 fps but whose 1080p is capped at 30. */
    private fun perSizeCapped() = caps(
        sizes = listOf(size(1280, 720, minFpsCap = 60), size(1920, 1080, minFpsCap = 30)),
        aeRanges = listOf(FpsRange(15, 30), FpsRange(30, 30), FpsRange(60, 60)),
    )

    private fun resolved(caps: CaptureModes.Capabilities, preference: CaptureModes.CapturePreference) =
        (CaptureModes.resolve(caps, preference) as CaptureModes.Resolution.Resolved).mode

    @Test
    fun listsOnlyIntersectedSizeFpsModes() {
        val modes = CaptureModes.supported(fps30Only())
        assertEquals(
            setOf(
                Triple(1280, 720, 15),
                Triple(1280, 720, 30),
                Triple(1920, 1080, 15),
                Triple(1920, 1080, 30),
            ),
            modes.map { Triple(it.width, it.height, it.targetFps) }.toSet(),
        )
        // 24 fps has no AE range whose upper bound is 24, and no 60 range exists.
        assertTrue(modes.none { it.targetFps == 24 })
        assertTrue(modes.none { it.targetFps == 60 })
    }

    @Test
    fun perSizeMinimumDurationCapsTheSizeNotTheDevice() {
        val modes = CaptureModes.supported(perSizeCapped())
        assertTrue(modes.any { it.width == 1280 && it.height == 720 && it.targetFps == 60 })
        // The regression: 1080p exists and 60 fps exists, but not together.
        assertTrue(modes.none { it.width == 1920 && it.height == 1080 && it.targetFps == 60 })
        val auto = resolved(perSizeCapped(), CaptureModes.CapturePreference())
        // 720p/30 is still preferred over 720p/60.
        assertEquals(Triple(1280, 720, 30), Triple(auto.width, auto.height, auto.targetFps))
    }

    @Test
    fun unknownMinimumDurationKeepsTheMode() {
        val caps = caps(
            sizes = listOf(size(1280, 720, minFpsCap = null)),
            aeRanges = listOf(FpsRange(60, 60)),
        )
        val modes = CaptureModes.supported(caps)
        assertTrue(modes.any { it.width == 1280 && it.height == 720 && it.targetFps == 60 })
    }

    @Test
    fun aeRangeUpperMustEqualTheCandidate() {
        // A 15–30 range cannot promise 24, and a fixed range wins over a variable one.
        assertNull(CaptureModes.chooseAeRange(listOf(FpsRange(15, 30)), 24))
        assertEquals(CaptureModes.FpsRange(30, 30), CaptureModes.chooseAeRange(listOf(FpsRange(15, 30), FpsRange(30, 30)), 30))
        assertEquals(CaptureModes.FpsRange(30, 30), CaptureModes.chooseAeRange(listOf(FpsRange(30, 30), FpsRange(15, 30)), 30))
    }

    @Test
    fun emptyCapabilitiesAreRejected() {
        val resolution = CaptureModes.resolve(
            caps(sizes = emptyList(), aeRanges = listOf(FpsRange(30, 30))),
            CaptureModes.CapturePreference(),
        )
        assertTrue(resolution is CaptureModes.Resolution.Rejected)
    }

    @Test
    fun unalignedSizesAreExcluded() {
        val modes = CaptureModes.supported(
            caps(
                sizes = listOf(size(1281, 721), size(1280, 720)),
                aeRanges = listOf(FpsRange(30, 30)),
            ),
        )
        assertEquals(listOf(1280), modes.map { it.width }.distinct())
    }

    @Test
    fun explicitRequestMatchesExactlyOrIsRejected() {
        val exact = resolved(fps30Only(), CaptureModes.CapturePreference(1280, 720, 30))
        assertEquals(Triple(1280, 720, 30), Triple(exact.width, exact.height, exact.targetFps))
        // 1080p exists and 60 fps exists on this device family, but 1080p/60 does not.
        assertTrue(
            CaptureModes.resolve(
                perSizeCapped(),
                CaptureModes.CapturePreference(1920, 1080, 60),
            ) is CaptureModes.Resolution.Rejected,
        )
        // 24 is never a supported combination on the 30-only device.
        assertTrue(
            CaptureModes.resolve(
                fps30Only(),
                CaptureModes.CapturePreference(1280, 720, 24),
            ) is CaptureModes.Resolution.Rejected,
        )
    }

    @Test
    fun autoBitrateScalesFromTheBaselineAndClamps() {
        val wide = 100_000..100_000_000
        assertEquals(6_000_000, CaptureModes.autoBitrate(1280, 720, 30, wide).toLong())
        assertEquals(13_500_000, CaptureModes.autoBitrate(1920, 1080, 30, wide).toLong())
        assertEquals(12_000_000, CaptureModes.autoBitrate(1280, 720, 60, wide).toLong())
        assertEquals(8_000_000, CaptureModes.autoBitrate(1920, 1080, 30, 1_000_000..8_000_000).toLong())
        assertEquals(1_000_000, CaptureModes.autoBitrate(320, 240, 15, 1_000_000..8_000_000).toLong())
    }

    @Test
    fun autoPrefers720pAspectAndAreaOnTies() {
        // Same fps and aspect; the closer area to 720p wins.
        val tied = caps(
            sizes = listOf(size(640, 360), size(1280, 720)),
            aeRanges = listOf(FpsRange(30, 30)),
        )
        val auto = resolved(tied, CaptureModes.CapturePreference())
        assertEquals(1280, auto.width)
        assertEquals(2, CaptureModes.supported(tied).size)
    }

    @Test
    fun differentDevicesReResolveTheSamePreference() {
        val frontOnly1080 = caps(
            sizes = listOf(size(1920, 1080)),
            aeRanges = listOf(FpsRange(15, 30), FpsRange(30, 30)),
            cameraId = "1",
        )
        val autoA = resolved(fps30Only(), CaptureModes.CapturePreference())
        val autoB = resolved(frontOnly1080, CaptureModes.CapturePreference())
        assertEquals("0", autoA.cameraId)
        assertEquals("1", autoB.cameraId)
        assertEquals(1920, autoB.width)
        // The 30-only device keeps 720p; no fps above 30 exists anywhere.
        assertTrue(autoA.targetFps <= 30)
    }
}
