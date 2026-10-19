package dev.nekomario.amb

import android.app.Instrumentation
import android.app.LocaleManager
import android.content.Context
import android.content.Intent
import android.graphics.Rect
import android.os.Build
import android.os.LocaleList
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.widget.Button
import android.widget.TextView
import java.io.File
import java.security.SecureRandom
import java.security.cert.X509Certificate
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import javax.net.ssl.X509TrustManager

/** Real production Activity, store, TLS gate and pointer input on a disposable
 * API-35 emulator. No approval hook is injected into the application. */
internal class ClientAuthorizationUiInstrumentation(
    private val test: Instrumentation,
    /** The caller has established that resizing the display and switching the app
     * locale is acceptable, which it only is on a disposable emulator or a device
     * the caller designated for conformance. Defaults to the emulator check. */
    private val allowDisplayChanges: Boolean = false,
) {
    private val context = test.targetContext
    private lateinit var activity: MainActivity
    private fun shell(command: String) = ParcelFileDescriptor.AutoCloseInputStream(
        test.uiAutomation.executeShellCommand(command)).bufferedReader().use { it.readText().trim() }

    fun run() {
        check(Build.VERSION.SDK_INT >= 33)
        check(allowDisplayChanges ||
            Build.MODEL.contains("sdk") || Build.FINGERPRINT.contains("generic")) {
            "display-changing UI conformance runs only on a disposable emulator"
        }
        val locale = checkNotNull(context.getSystemService(LocaleManager::class.java))
        val originalLocale = locale.applicationLocales
        val originalFont = shell("settings get system font_scale")
        check(originalFont == "null" || originalFont.toFloatOrNull()?.let { it in .1f..10f } == true)
        val originalSize = Regex("Override size: (\\d+x\\d+)").find(shell("wm size"))?.groupValues?.get(1)
        val preferences = context.getSharedPreferences("settings", Context.MODE_PRIVATE)
        val originalMode = preferences.getString("connectionMode", "USB")
        try {
            shell("wm size 840x1400")
            for ((language, scale) in listOf("en" to "1.0", "ja" to "1.3")) {
                shell("settings put system font_scale $scale")
                locale.applicationLocales = LocaleList.forLanguageTags(language)
                waitUi { context.resources.configuration.locales[0].language == language &&
                    kotlin.math.abs(context.resources.configuration.fontScale - scale.toFloat()) < .02f }
                activity = test.startActivitySync(Intent(context, MainActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
                test.waitForIdleSync()
                check(activity.resources.configuration.locales[0].language == language)
                check(activity.resources.configuration.screenWidthDp <= 360) { "UI gate is not narrow" }
                check(kotlin.math.abs(activity.resources.configuration.fontScale - scale.toFloat()) < .02f)
                android.util.Log.i("SecureWifiGate", "UI authorization scenario: $language, font=$scale")
                try { exercise(language) } finally {
                    test.runOnMainSync { activity.finish() }
                    waitUi { activity.isDestroyed }
                }
            }
            locale.applicationLocales = LocaleList.forLanguageTags("en")
            waitUi { context.resources.configuration.locales[0].language == "en" }
            storageFailureUi()
        } finally {
            locale.applicationLocales = originalLocale
            shell(if (originalFont == "null") "settings delete system font_scale" else "settings put system font_scale $originalFont")
            shell(if (originalSize == null) "wm size reset" else "wm size $originalSize")
            check(preferences.edit().putString("connectionMode", originalMode).commit())
        }
    }

    private fun exercise(language: String) {
        val japanese = language == "ja"
        tap(view<View>(R.id.lan_mode))
        val identity = SecureLanIdentity(context)
        val generator = java.security.KeyPairGenerator.getInstance("EC").apply {
            initialize(java.security.spec.ECGenParameterSpec("secp256r1"))
        }
        val keyPair = generator.generateKeyPair()
        val publicKey = keyPair.public.encoded.takeLast(65).toByteArray()
        val id = ClientAuthorization.deriveClientId(publicKey)
        val label = "instrumented-pc"
        val tls = SSLContext.getInstance("TLS").apply {
            // Test peer only: the exact certificate is checked after TLS.
            init(null, arrayOf(object : X509TrustManager {
                override fun getAcceptedIssuers() = emptyArray<X509Certificate>()
                override fun checkClientTrusted(chain: Array<X509Certificate>, auth: String) {}
                override fun checkServerTrusted(chain: Array<X509Certificate>, auth: String) {}
            }), SecureRandom())
        }
        try {
            val staleApprove: View
            openPeer(tls, identity, generator.generateKeyPair(), "abandoned-pc").use { first ->
                check(first.result().status == Wire.AUTH_PENDING)
                waitUi { view<TextView>(R.id.client_request).text.contains("abandoned-pc") }
                staleApprove = view(R.id.allow_client)
                reveal(view(R.id.client_request))
                capture("$language-reject-request")
                tap(view(R.id.deny_client))
                check(first.result().status == Wire.AUTH_REJECTED)
            }
            // A deliberate new request after the real owner's prompt cooldown,
            // not an automatic production retry or a test-only approval bypass.
            Thread.sleep(PhoneAuthorization.UNKNOWN_COOLDOWN_MS + 250)
            lateinit var saved: ClientAuthorization.Client
            openPeer(tls, identity, keyPair, label).use { peer ->
                check(peer.result().status == Wire.AUTH_PENDING)
                val fullId = id.joinToString("") { "%02x".format(it) }.chunked(4).joinToString(" ")
                waitUi { view<TextView>(R.id.client_request).isShown &&
                    view<TextView>(R.id.client_request).text.contains(fullId) }
                reveal(view(R.id.client_request))
                capture("$language-pending")
                // Replay a listener already queued for the former view. Primary
                // approve/reject/renew/revoke actions below use real pointer input.
                test.runOnMainSync { staleApprove.performClick() }
                check(ClientStore(context).repository.snapshot().none { it.clientId.contentEquals(id) }) {
                    "a stale approval action granted the unseen replacement"
                }
                test.runOnMainSync { check(!view<Button>(R.id.camera_action).isEnabled) }
                capture("$language-stale-safe")
                tap(view(R.id.allow_client))
                check(peer.result().status == Wire.AUTH_AUTHORIZED)
                peer.read(Wire.TYPE_QUALITY_CAPABILITIES)
                saved = ClientStore(context).repository.snapshot().single { it.clientId.contentEquals(id) }
                check(saved.publicKey.contentEquals(publicKey)) { "UI approval was not persisted" }
                checkStoppedCamera(japanese)
                capture("$language-allowed")
                peer.send(Wire.TYPE_PING, ByteArray(8))
                check(peer.read(Wire.TYPE_PONG).contentEquals(ByteArray(8)))
            }
            // The status is the state; the address is procedure and lives behind
            // Details, which is the contract this scenario now holds to. Wait on the
            // help text itself: several states start with "Wi-Fi", so the status
            // prefix alone does not say which one rendered.
            waitUi { view<TextView>(R.id.lan_help).text.isNotEmpty() }
            check(view<TextView>(R.id.lan_help).visibility == View.GONE) {
                "the phone address must be hidden until Details is opened"
            }
            val status = view<TextView>(R.id.connection_status).text
            check(status.startsWith("Wi-Fi") && !status.contains("TLS")) {
                "the status must be a short state, not the procedure: $status"
            }
            tap(view(R.id.details_action))
            waitUi { view<TextView>(R.id.lan_help).visibility == View.VISIBLE }
            capture("$language-details-open")
            tap(view(R.id.details_action))
            waitUi { view<TextView>(R.id.lan_help).visibility == View.GONE }
            openPeer(tls, identity, keyPair, label).use { peer ->
                val known = peer.result()
                check(known.status == Wire.AUTH_AUTHORIZED && known.expiresAtUnixSeconds == saved.expiresAtUnixSeconds)
                peer.read(Wire.TYPE_QUALITY_CAPABILITIES)
                check(ClientStore(context).repository.snapshot().single { it.clientId.contentEquals(id) } == saved) {
                    "known-PC reconnect silently renewed authorization"
                }
                checkStoppedCamera(japanese)
                capture("$language-reconnected")
                while (System.currentTimeMillis() / 1000 <= saved.approvedAtUnixSeconds) Thread.sleep(50)
                tap(view(R.id.authorized_clients)); tapText(label)
                capture("$language-client-details")
                tapText(if (japanese) "90日間更新" else "Renew 90 days")
                tapText(if (japanese) "確定" else "Confirm")
                waitUi { ClientStore(context).repository.snapshot().single { it.clientId.contentEquals(id) }
                    .expiresAtUnixSeconds > saved.expiresAtUnixSeconds }
                tap(view(R.id.authorized_clients)); tapText(label)
                tapText(if (japanese) "許可を取り消す" else "Revoke")
                tapText(if (japanese) "確定" else "Confirm")
                waitUi { ClientStore(context).repository.snapshot().none { it.clientId.contentEquals(id) } }
                val terminal = runCatching { peer.socket.inputStream.read() }
                check(terminal.getOrNull() == -1 || terminal.exceptionOrNull() is java.io.EOFException ||
                    terminal.exceptionOrNull() is javax.net.ssl.SSLException ||
                    terminal.exceptionOrNull() is java.net.SocketException) {
                    "revoked UI session was not terminal: $terminal"
                }
                waitUi { !view<Button>(R.id.camera_action).isEnabled }
                capture("$language-revoked")
            }
        } finally {
            test.runOnMainSync { activity.finish() }
            waitUi { activity.isDestroyed }
            ClientStore(context).repository.revoke(id)
        }
    }

    private fun checkStoppedCamera(japanese: Boolean) {
        waitUi { view<Button>(R.id.camera_action).isEnabled }
        test.runOnMainSync {
            check(view<Button>(R.id.camera_action).text == if (japanese) "カメラ開始" else "Start camera") {
                "authorization implicitly started capture or inherited Start intent"
            }
        }
    }

    private class Peer(val socket: SSLSocket) : AutoCloseable {
        fun send(type: Byte, payload: ByteArray, sequence: Int = 2) =
            Wire.writeFrame(socket.outputStream, Wire.Header(type, 0, sequence, payload.size, 0), payload)
        fun read(type: Byte): ByteArray {
            val (header, payload) = Wire.readFrame(socket.inputStream)
            check(header.type == type) { "unexpected frame before/after UI approval: ${header.type}" }
            return payload
        }
        fun result() = Wire.decodeClientAuthResult(read(Wire.TYPE_CLIENT_AUTH_RESULT))
        override fun close() = socket.close()
    }

    private fun openPeer(tls: SSLContext, identity: SecureLanIdentity,
        keyPair: java.security.KeyPair, label: String): Peer {
        val peer = Peer(tls.socketFactory.createSocket("127.0.0.1", LanServer.DEFAULT_PORT) as SSLSocket)
        try {
            peer.socket.enabledProtocols = arrayOf("TLSv1.3")
            peer.socket.soTimeout = 5000
            peer.socket.startHandshake()
            val binding = Wire.sha256(peer.socket.session.peerCertificates[0].encoded)
            check(binding.joinToString("") { "%02x".format(it) }.equals(identity.fingerprint(), ignoreCase = true)) {
                "UI peer certificate mismatch"
            }
            peer.send(Wire.TYPE_HELLO, identity.pairingCode().chunked(2).map { it.toInt(16).toByte() }.toByteArray(), 1)
            check(peer.read(Wire.TYPE_HELLO_ACK).isEmpty())
            val challenge = Wire.decodeClientChallenge(peer.read(Wire.TYPE_CLIENT_CHALLENGE))
            check(challenge.transport == Wire.AUTH_LAN && challenge.phoneBinding.contentEquals(binding))
            val point = keyPair.public.encoded.takeLast(65).toByteArray()
            val message = Wire.clientAuthMessage(Wire.AUTH_LAN, binding, challenge.nonce, point, label)
            val signature = java.security.Signature.getInstance("SHA256withECDSA").run {
                initSign(keyPair.private); update(message); sign()
            }
            peer.send(Wire.TYPE_CLIENT_AUTH, Wire.encodeClientAuth(Wire.ClientAuth(
                Wire.CLIENT_AUTH_VERSION, point, label, rawSignature(signature))))
            return peer
        } catch (error: Throwable) {
            peer.close()
            throw error
        }
    }

    private fun storageFailureUi() {
        val prefs = context.getSharedPreferences("client_authorization", Context.MODE_PRIVATE)
        // The store's own alias, which a deliberate reset must delete.
        val alias = "mobile_webcam_client_authorization_aes_v1"
        var resetHappened = false
        val original = checkNotNull(prefs.getString(ClientStore.RECORD, null))
        val damaged = android.util.Base64.decode(original, android.util.Base64.NO_WRAP)
        damaged[damaged.lastIndex] = (damaged.last().toInt() xor 1).toByte()
        val encoded = android.util.Base64.encodeToString(damaged, android.util.Base64.NO_WRAP)
        check(prefs.edit().putString(ClientStore.RECORD, encoded).commit())
        try {
            activity = test.startActivitySync(Intent(context, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
            waitUi {
                !view<Button>(R.id.lan_mode).isEnabled && !view<Button>(R.id.usb_mode).isEnabled &&
                    !view<Button>(R.id.camera_action).isEnabled &&
                    view<TextView>(R.id.connection_status).text.contains("Authorization storage unavailable")
            }
            check(prefs.getString(ClientStore.RECORD, null) == encoded) { "failed startup reset authorization data" }
            val attempted = runCatching {
                java.net.Socket().use { it.connect(java.net.InetSocketAddress("127.0.0.1", LanServer.DEFAULT_PORT), 1000) }
            }
            check(attempted.exceptionOrNull() is java.net.ConnectException) { "corrupt-store startup opened a listener: $attempted" }
            capture("storage-failed")
            // The action is the only way out of this state. Press it twice — the
            // first press only arms it — and require the *effect*: the record and
            // the namespace key are gone, and a fresh store reads empty. When it
            // does not happen, the message carries the app's own words, because
            // "an effect did not arrive" is what made the earlier attempts
            // undiagnosable.
            fun keyStore() = java.security.KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
            fun recovered() = !prefs.contains(ClientStore.RECORD) && !keyStore().containsAlias(alias)
            tap(view(R.id.storage_action))
            tap(view(R.id.storage_action))
            val deadline = SystemClock.elapsedRealtime() + 12000
            while (!recovered()) {
                check(SystemClock.elapsedRealtime() < deadline) {
                    val fresh = runCatching { ClientStore(activity).repository.snapshot() }
                    "the reset action did not take effect; app says " +
                        "\"${view<TextView>(R.id.connection_status).text}\" " +
                        "(record=${prefs.contains(ClientStore.RECORD)}, " +
                        "key=${keyStore().containsAlias(alias)}, " +
                        "epoch=${dev.nekomario.amb.StorageGeneration.current("client_authorization")}, " +
                        "freshStore=${fresh.exceptionOrNull() ?: fresh.getOrNull()})"
                }
                Thread.sleep(50)
            }
            resetHappened = true
            val store = runCatching { ClientStore(activity).repository.snapshot() }
            check(store.getOrNull()?.isEmpty() == true) {
                "a fresh store after the reset: ${store.exceptionOrNull() ?: store.getOrNull()}"
            }
        } finally {
            try {
                test.runOnMainSync { activity.finish() }
                waitUi { activity.isDestroyed }
            } finally {
                // A successful reset deliberately removed the record *and* the key
                // that could read it, so putting the old record back would leave
                // exactly the state this scenario exists to escape.
                if (!resetHappened) check(prefs.edit().putString(ClientStore.RECORD, original).commit())
            }
        }
    }

    private fun waitUi(predicate: () -> Boolean) {
        val deadline = SystemClock.elapsedRealtime() + 8000
        while (true) {
            var done = false
            test.runOnMainSync { done = predicate() }
            if (done) return
            check(SystemClock.elapsedRealtime() < deadline) { "authorization UI effect did not arrive" }
            Thread.sleep(50)
        }
    }

    private inline fun <reified T : View> view(id: Int): T = checkNotNull(activity.findViewById<T>(id))

    private fun reveal(view: View) {
        test.runOnMainSync { view.requestRectangleOnScreen(Rect(0, 0, view.width, view.height), true) }
        test.waitForIdleSync()
    }
    private fun tap(view: View) {
        waitUi { view.hasWindowFocus() && view.isEnabled }
        reveal(view)
        val rect = Rect()
        test.runOnMainSync { check(view.isEnabled && view.getGlobalVisibleRect(rect) && !rect.isEmpty) }
        tap(rect)
    }
    private fun tapText(text: String) {
        val deadline = SystemClock.elapsedRealtime() + 8000
        while (true) {
            val candidates = test.uiAutomation.rootInActiveWindow?.findAccessibilityNodeInfosByText(text).orEmpty()
            // Android exposes transformed button text (e.g. RENEW 90 DAYS).
            val node = candidates.firstOrNull {
                it.text?.toString()?.equals(text, ignoreCase = true) == true ||
                    it.text?.toString()?.startsWith("$text\n", ignoreCase = true) == true
            }
            if (node != null) {
                val rect = Rect(); node.getBoundsInScreen(rect); check(!rect.isEmpty)
                tap(rect); return
            }
            check(SystemClock.elapsedRealtime() < deadline) {
                "dialog action missing: $text; candidate text=${candidates.map { it.text }}"
            }
            Thread.sleep(50)
        }
    }
    private fun tap(rect: Rect) {
        val down = SystemClock.uptimeMillis()
        for (action in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
            val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action,
                rect.exactCenterX(), rect.exactCenterY(), 0)
            try { test.sendPointerSync(event) } finally { event.recycle() }
            if (action == MotionEvent.ACTION_DOWN) Thread.sleep(40)
        }
        test.waitForIdleSync()
    }
    private fun capture(name: String) {
        test.waitForIdleSync()
        val bitmap = checkNotNull(test.uiAutomation.takeScreenshot())
        val directory = checkNotNull(context.getExternalFilesDir("authorization-ui"))
        check(directory.isDirectory || directory.mkdirs())
        File(directory, "$name.png").outputStream().use {
            check(bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it))
        }
        val configuration = activity.resources.configuration
        File(directory, "$name.json").writeText("""{"scenario":"$name","width":${bitmap.width},"height":${bitmap.height},"widthDp":${configuration.screenWidthDp},"fontScale":${configuration.fontScale},"locale":"${configuration.locales[0].toLanguageTag()}"}""")
        bitmap.recycle()
    }
    private fun rawSignature(der: ByteArray): ByteArray {
        check(der[0] == 0x30.toByte() && der[1].toInt() == der.size - 2)
        var offset = 2
        val output = ByteArray(64)
        repeat(2) { part ->
            check(der[offset++] == 2.toByte())
            val size = der[offset++].toInt() and 0xff
            val integer = java.math.BigInteger(der.copyOfRange(offset, offset + size))
            check(integer.signum() >= 0 && integer.bitLength() <= 256)
            val bytes = integer.toByteArray().takeLast(32).toByteArray()
            bytes.copyInto(output, (part + 1) * 32 - bytes.size)
            offset += size
        }
        check(offset == der.size)
        return output
    }
}
