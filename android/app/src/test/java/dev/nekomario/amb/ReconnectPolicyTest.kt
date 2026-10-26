package dev.nekomario.amb

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ReconnectPolicyTest {
    @Test
    fun aCablePullResumesAndARefusalDoesNot() {
        assertTrue(ReconnectPolicy.shouldResume(null))
        assertTrue(ReconnectPolicy.shouldResume("Broken pipe"))
        assertTrue(ReconnectPolicy.shouldResume("a new host asked for a fresh exchange"))
        assertFalse(ReconnectPolicy.shouldResume("PC authorization revoked or expired"))
        assertFalse(ReconnectPolicy.shouldResume("Wi-Fi client rejected (check pairing details)"))
        assertFalse(ReconnectPolicy.shouldResume("Authorization storage failed"))
        assertFalse(ReconnectPolicy.shouldResume("USB access denied"))
    }

    @Test
    fun rediscoverBacksOffAndCaps() {
        assertEquals(1_000L, ReconnectPolicy.rediscoverDelayMs(0))
        assertEquals(2_000L, ReconnectPolicy.rediscoverDelayMs(1))
        assertEquals(4_000L, ReconnectPolicy.rediscoverDelayMs(2))
        assertEquals(8_000L, ReconnectPolicy.rediscoverDelayMs(3))
        assertEquals(8_000L, ReconnectPolicy.rediscoverDelayMs(99))
        assertEquals(1_000L, ReconnectPolicy.rediscoverDelayMs(-1))
    }

    @Test
    fun theLogLineHasNoFreeFormText() {
        assertEquals(
            "reconnect transport=usb attempt=2 delay_ms=4000",
            ReconnectPolicy.logLine("usb", 2, 4_000),
        )
        assertEquals(
            "reconnect transport=unknown attempt=0 delay_ms=0",
            ReconnectPolicy.logLine("192.168.1.5", -3, -1),
        )
    }

    @Test
    fun reconnectCopyNamesTheStateAndTheNextActionInBothLanguages() {
        val usb = ReconnectPolicy.usbReconnecting()
        assertTrue(usb.english.contains("Reconnecting"))
        assertTrue(usb.english.contains("Retry"))
        assertTrue(usb.japanese.contains("再接続中"))
        assertTrue(usb.japanese.contains("再試行"))
        val wifi = ReconnectPolicy.wifiReconnecting()
        assertTrue(wifi.english.contains("retries"))
        assertTrue(wifi.japanese.contains("再試行"))
        val refused = ReconnectPolicy.wifiRefused()
        assertTrue(refused.english.contains("Retry"))
        assertTrue(refused.japanese.contains("再試行"))
        val usbRefused = ReconnectPolicy.usbRefused()
        assertTrue(usbRefused.english.contains("Retry"))
        assertTrue(usbRefused.japanese.contains("再試行"))
    }
}
