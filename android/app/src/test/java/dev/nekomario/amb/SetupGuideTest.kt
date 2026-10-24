package dev.nekomario.amb

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SetupGuideTest {
    @Test
    fun usbWaitNamesTheSingleNextActionInBothLanguages() {
        val copy = SetupGuide.usbWaitingForHost()
        assertTrue(copy.english.contains("Connect"))
        assertTrue(copy.japanese.contains("接続"))
        assertFalse(copy.english.contains("fingerprint"))
        assertFalse(copy.japanese.contains("指紋"))
    }

    @Test
    fun pairingSentPointsAtTheTwoButtonsAlreadyOnScreen() {
        val copy = SetupGuide.pairingSent()
        assertTrue(copy.english.contains("Wi-Fi"))
        assertTrue(copy.english.contains("Switch to Wi-Fi"))
        assertTrue(copy.japanese.contains("Wi-Fi"))
        assertTrue(copy.japanese.contains("Wi-Fiに切り替える"))
        // The hand-off exists so the user never types the key. The success line
        // must not send them back to it.
        assertFalse(copy.english.lowercase().contains("key"))
        assertFalse(copy.japanese.contains("キー"))
    }
}
