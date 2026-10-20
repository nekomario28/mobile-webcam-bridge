package dev.nekomario.amb

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * Pins the rule that the phone's screen shows state, not procedure.
 *
 * The address, the how-to and the key rotation are secondary: rotating the key
 * disconnects every Wi-Fi client, and instructions belong behind a disclosure.
 * The behavioural tests drive the session rather than the Activity's layout, so a
 * revert that puts them back on the main screen would leave every one of them
 * green — which is what this file refuses.
 */
class SecondarySurfaceWiringTest {
    private fun source(): String =
        generateSequence(File(checkNotNull(System.getProperty("user.dir")))) { it.parentFile }
            .flatMap { root ->
                listOf(
                    File(root, "android/app/src/main/java/dev/nekomario/amb/MainActivity.kt"),
                    File(root, "app/src/main/java/dev/nekomario/amb/MainActivity.kt"),
                ).asSequence()
            }
            .firstOrNull { it.isFile }
            ?.readText()
            ?: error("MainActivity.kt not found")

    private fun bodyOf(source: String, signature: String): String {
        val declaration = source.indexOf(signature)
        check(declaration >= 0) { "missing $signature" }
        val open = source.indexOf('{', declaration)
        check(open > 0) { "missing body for $signature" }
        var depth = 0
        var index = open
        while (index < source.length) {
            when (source[index]) {
                '{' -> depth++
                '}' -> {
                    depth--
                    if (depth == 0) return source.substring(open + 1, index)
                }
            }
            index++
        }
        error("unbalanced braces after $signature")
    }

    @Test
    fun rotating_the_key_is_behind_details() {
        val source = source()
        val secondary = bodyOf(source, "private fun applySecondaryVisibility()")
        assertTrue("the rotate control must follow the Details disclosure",
            secondary.contains("rotateAction.visibility"))
        assertTrue("it must be shown only while Details is open",
            secondary.contains("open && connectionMode == ConnectionMode.LAN"))
        assertFalse("updateControls must not show it unconditionally",
            bodyOf(source, "private fun updateControls()").contains("rotateAction.visibility = pairAction.visibility"))
        assertTrue("opening Details must apply the rule",
            source.contains("applySecondaryVisibility()"))
    }

    @Test
    fun the_status_line_carries_state_and_the_procedure_lives_behind_details() {
        val waiting = bodyOf(source(), "private fun showLanWaiting(")
        assertTrue("the address and how-to belong to the hidden help text",
            waiting.contains("lanHelpText = buildString"))
        assertTrue("the status must be the short state, not a built procedure",
            waiting.contains("show(ui(\"Wi-Fi · waiting for the PC\", \"Wi-Fi · PCの接続を待っています\"))"))
        assertFalse("the status call must not assemble the address",
            waiting.contains("show(\n            buildString"))
    }

    @Test
    fun the_pairing_key_leaves_the_screen_once_a_pc_is_approved() {
        // The key exists to get the first PC approved; leaving it up afterwards is
        // clutter and a secret with nothing left to do.
        val source = source()
        val controls = bodyOf(source, "private fun updateControls()")
        assertTrue(
            "updateControls must hide the key when the first PC is approved",
            controls.contains("approved > 0 && approvedCount == 0") &&
                controls.contains("setPairingKeyVisible(false)"),
        )
        assertTrue("the count must be remembered across updates", source.contains("private var approvedCount"))
    }

    @Test
    fun the_approved_list_says_how_many() {
        val ui = generateSequence(File(checkNotNull(System.getProperty("user.dir")))) { it.parentFile }
            .flatMap { root ->
                listOf(
                    File(root, "android/app/src/main/java/dev/nekomario/amb/ClientApprovalUi.kt"),
                    File(root, "app/src/main/java/dev/nekomario/amb/ClientApprovalUi.kt"),
                ).asSequence()
            }
            .firstOrNull { it.isFile }
            ?.readText()
            ?: error("ClientApprovalUi.kt not found")
        assertTrue("the Authorized PCs button must carry the count", ui.contains("Authorized PCs (\u0024count)"))
        assertTrue("an empty list must say so", ui.contains("none yet"))
    }

    @Test
    fun every_control_is_actually_in_the_layout() {
        // Measured: `rotateAction` was built and given a visibility rule, but the
        // addView call had been dropped when the Details group was reorganised, so
        // the button existed and could never be reached — no behavioural test and no
        // visibility assertion noticed.
        val source = source()
        for (control in listOf(
            "sendPairingAction", "rotateAction", "lanHelp", "storageAction", "pairAction",
            "pairInfo", "cameraButton", "details", "usbMode", "lanMode", "cameras",
        )) {
            assertTrue(
                "$control is built but never added to the layout",
                source.contains("addView($control"),
            )
        }
    }

    @Test
    fun the_pairing_key_is_shown_without_a_tap_when_nothing_is_approved() {
        val start = bodyOf(source(), "private fun startLanMode()")
        assertTrue("an unpaired phone must reveal the key by itself",
            start.contains("authorization.clients().isEmpty()") && start.contains("setPairingKeyVisible(true)"))
        val reveal = bodyOf(source(), "private fun setPairingKeyVisible(")
        assertTrue("hiding must clear the text and the secure flag",
            reveal.contains("pairInfo.text = \"\"") && reveal.contains("FLAG_SECURE"))
    }
}
