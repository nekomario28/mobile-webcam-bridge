package dev.nekomario.amb

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class EditionPolicyTest {
    private fun mode(width: Int, height: Int, fps: Int) = CaptureModes.CaptureMode(
        cameraId = "0",
        codecName = "c",
        width = width,
        height = height,
        targetFps = fps,
        aeRange = CaptureModes.FpsRange(fps, fps),
        bitrate = 6_000_000,
        minFrameDurationKnown = true,
    )

    @Test
    fun ossCapAllows1080p30AndRejects4kAnd60fps() {
        val policy = EditionPolicy.OssCap1080p30
        assertTrue(policy.allows(1920, 1080, 30))
        assertTrue(policy.allows(1080, 1920, 30)) // portrait
        assertTrue(policy.allows(1280, 720, 30))
        assertFalse(policy.allows(3840, 2160, 30))
        assertFalse(policy.allows(1920, 1080, 60))
        assertFalse(policy.allows(1280, 720, 60))
    }

    @Test
    fun filterDropsModesAboveCap() {
        val modes = listOf(
            mode(1280, 720, 30),
            mode(1920, 1080, 30),
            mode(1920, 1080, 60),
            mode(3840, 2160, 30),
        )
        val kept = EditionPolicy.OssCap1080p30.filter(modes)
        assertEquals(listOf(modes[0], modes[1]), kept)
    }

    @Test
    fun uncappedKeeps4k() {
        assertTrue(EditionPolicy.Uncapped.allows(3840, 2160, 30))
    }
}
