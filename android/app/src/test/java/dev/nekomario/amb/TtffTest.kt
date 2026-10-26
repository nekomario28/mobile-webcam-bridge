package dev.nekomario.amb

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class TtffTest {
    @Test
    fun lineIsPhaseAndDurationOnly() {
        assertEquals(
            "ttff phase=phone_first_au elapsed_ms=1500",
            Ttff.line("phone_first_au", 1_000, 2_500),
        )
        assertEquals(
            "ttff phase=phone_open elapsed_ms=0",
            Ttff.line("phone_open", 50, 40),
        )
    }

    @Test
    fun aFreeFormPhaseIsRejected() {
        assertThrows(IllegalArgumentException::class.java) {
            Ttff.line("camera error: /sdcard/x", 0, 1)
        }
        assertThrows(IllegalArgumentException::class.java) {
            Ttff.line("phase with space", 0, 1)
        }
    }
}
