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
    fun a_connection_uses_the_binding_bytes_instead_of_reparsing_the_fingerprint() {
        // Measured while reading the phone side: every accepted Wi-Fi connection read
        // the certificate from the Keystore, hashed it, formatted 32 bytes as hex, and
        // then parsed that hex straight back into bytes. The identity now answers with
        // the digest itself, and the two forms cannot drift because one derives the
        // other.
        fun read(name: String): String =
            generateSequence(File(checkNotNull(System.getProperty("user.dir")))) { it.parentFile }
                .flatMap { root ->
                    listOf(
                        File(root, "android/app/src/main/java/dev/nekomario/amb/$name"),
                        File(root, "app/src/main/java/dev/nekomario/amb/$name"),
                    ).asSequence()
                }
                .firstOrNull { it.isFile }
                ?.readText()
                ?: error("$name not found")
        val identity = read("SecureLanIdentity.kt")
        val server = read("LanServer.kt")
        assertTrue(
            "the identity must expose the digest as bytes",
            identity.contains("fun fingerprintBytes()"),
        )
        assertTrue(
            "the hex form must derive from the bytes, not be computed separately",
            identity.contains("fun fingerprint(): String = hex(fingerprintBytes())"),
        )
        assertTrue(
            "the session binding must not hex-encode and parse the fingerprint",
            server.contains("identity.fingerprintBytes()") &&
                !server.contains("fingerprint().chunked(2)"),
        )
        assertTrue(
            "the access key must be decoded once, not per connection",
            identity.contains("secretBytes: ByteArray = decodeHex(secret)") &&
                identity.contains("authenticatesWith(secretBytes, candidate)"),
        )
        // Note on what this file can and cannot prove: it reads source text, so it is a
        // one-owner guard, not evidence about behaviour. The review of 2026-10-23 said
        // so about the earlier version of this assertion, and the behaviour now has its
        // own test (`WireAndQueueTest.pairing_code_comparison_is_exact`).
        assertTrue(
            "a rotated key must refresh the cached bytes",
            identity.contains("secretBytes = decodeHex(next)"),
        )
    }

    @Test
    fun the_details_rule_has_exactly_one_owner() {
        // Measured: updateControls set pairAction.visibility on its own and overruled
        // the disclosure, so the key button was back on the main screen while every
        // test stayed green.
        val source = source()
        val controls = bodyOf(source, "private fun updateControls()")
        for (control in listOf("pairAction", "pairInfo", "storageAction", "rotateAction", "lanHelp")) {
            assertFalse("updateControls must not decide $control's visibility",
                controls.contains("$control.visibility"))
        }
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
            "retryAction",
        )) {
            assertTrue(
                "$control is built but never added to the layout",
                source.contains("addView($control"),
            )
        }
    }

    @Test
    fun the_pairing_handoff_is_offered_only_for_an_authorized_pc() {
        // ID-003: the hand-off exists for a PC that has been approved, and its write is
        // gated in the session. Nothing asserted the button's own rule, so a revert that
        // offered it for any live session would leave every other test green.
        val secondary = bodyOf(source(), "private fun applySecondaryVisibility()")
        assertTrue("the hand-off must be offered for a session at all",
            secondary.contains("sendPairingAction.visibility"))
        assertTrue("it must be offered only while that session is authorized",
            secondary.contains("clientAuthorization?.isAuthorized() == true"))
    }

    @Test
    fun the_pairing_key_is_a_fallback_behind_details_and_never_appears_by_itself() {
        // The PC takes the pairing itself over the cable, so the key is not the next
        // thing the user needs and must not be pushed onto the main screen.
        val source = source()
        val start = bodyOf(source, "private fun startLanMode(")
        assertFalse("nothing may reveal the key on its own",
            start.contains("setPairingKeyVisible(true)"))
        val secondary = bodyOf(source, "private fun applySecondaryVisibility()")
        assertTrue("the key controls must follow the Details disclosure",
            secondary.contains("pairAction.visibility =") && secondary.contains("pairInfo.visibility ="))
        assertTrue("the key itself is shown only in Details and only when revealed",
            secondary.contains("pairInfo.visibility = if (open && pairingKeyVisible)"))
        assertTrue(
            "sending the pairing over USB is the short Wi-Fi path and must not wait for Details",
            secondary.contains("sendPairingAction.visibility") &&
                !secondary.contains("open && connectionMode == ConnectionMode.USB"),
        )
        assertTrue(
            "erasing the record is destructive and belongs behind Details, except when a failed read needs its remedy reachable",
            secondary.contains("storageAction.visibility = if (open || storageFailed)"),
        )
        val reveal = bodyOf(source, "private fun setPairingKeyVisible(")
        assertTrue("hiding must clear the text and the secure flag",
            reveal.contains("pairInfo.text = \"\"") && reveal.contains("FLAG_SECURE"))
    }

    @Test
    fun a_dropped_link_names_retry_and_does_not_resume_a_refusal() {
        val source = source()
        assertTrue(
            "the retry button must be in the layout",
            source.contains("addView(retryAction"),
        )
        assertTrue(
            "a refusal must not resume capture",
            source.contains("ReconnectPolicy.shouldResume"),
        )
        val controls = bodyOf(source, "private fun updateControls()")
        assertTrue(
            "updateControls may show Retry while a connection problem is on screen",
            controls.contains("retryAction.visibility"),
        )
    }
}
