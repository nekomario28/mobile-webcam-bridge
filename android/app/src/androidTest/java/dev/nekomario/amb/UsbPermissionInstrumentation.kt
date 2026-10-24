package dev.nekomario.amb

import android.app.Activity
import android.app.Instrumentation
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.Manifest
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.Bundle
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/** Exercises the platform notification boundary without pretending a USB device is attached. */
class UsbPermissionInstrumentation : Instrumentation() {
    private var disposableUi = false
    private var cameraAcceptance = false
    /** Explicit opt-in for the camera acceptance on real hardware. */
    private var physicalDevice = false
    /** Explicit opt-in for the USB reader-release measurement (see [UsbReaderReleaseMeasurement]). */
    private var usbReaderMeasurement = false

    override fun onCreate(arguments: Bundle?) {
        super.onCreate(arguments)
        disposableUi = arguments?.getString("disposableUi") == "true"
        cameraAcceptance = arguments?.getString("cameraAcceptance") == "true"
        physicalDevice = arguments?.getString("physicalDevice") == "true"
        usbReaderMeasurement = arguments?.getString("usbReaderMeasurement") == "true"
        start()
    }

    /**
     * Runs the real LanServer on the Android emulator while GitHub Actions
     * connects to it from the host via adb forward. Using an independent
     * desktop TLS stack tests the same Android Keystore server path as the
     * Rust receiver: an unauthorized HELLO is rejected before any session
     * exists, then the authorized client completes pairing.
     */
    private fun smokeSecureWifi(context: Context) {
        val identity = SecureLanIdentity(context)
        val stored = context.getSharedPreferences("secure_wifi", Context.MODE_PRIVATE)
        check(!stored.contains("secret")) { "plaintext Wi-Fi credential remains on disk" }
        check(stored.contains("encrypted_secret_v1")) { "encrypted Wi-Fi credential missing" }
        check(SecureLanIdentity(context).pairingCode() == identity.pairingCode()) {
            "Wi-Fi access key did not persist"
        }
        val pin = identity.fingerprint()
        check(pin.length == 64)
        val sessions = LinkedBlockingQueue<Pair<LanSession, Boolean>>()
        val errors = LinkedBlockingQueue<String>()
        val qualityCommands = LinkedBlockingQueue<Wire.HostCommand>()
        // The emulator holds no camera grant, so the controller must answer a
        // prepare with a typed permission rejection rather than dropping the
        // connection or starting a camera.
        val captureMode = CaptureModes.CaptureMode(
            cameraId = "0",
            codecName = "fixture-encoder",
            width = 1280,
            height = 720,
            targetFps = 30,
            aeRange = CaptureModes.FpsRange(30, 30),
            bitrate = 6_000_000,
            minFrameDurationKnown = true,
        )
        val snapshot = Wire.Capabilities(
            extensionVersion = 1,
            permission = Wire.PERMISSION_PENDING,
            catalogRevision = 7,
            streamEpoch = 0,
            modes = listOf(Wire.QualityMode(1280, 720, 30, 6_000_000)),
        )
        var controller: PhoneQualityController? = null
        // This fixture explicitly approves a valid pending proof. The real
        // Activity's approval UI is exercised separately; no app bypass flag.
        val authorization = PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = { error("fixture storage failed") })
        val server = LanServer(
            identity = identity,
            authorization = authorization,
            onAuthorization = { it.clientAuthorization?.takeIf { gate -> gate.pending() != null }?.approve() },
            // No camera producer exists in this fixture; the conformance gate
            // exercises the UI-side approval/renew/revoke path instead.
            onCaptureCancelled = {},
            port = 48_527,
            onSession = { session, connected ->
                if (connected) {
                    controller = PhoneQualityController(
                        session = session,
                        revision = { 7 },
                        modes = { listOf(captureMode) },
                        permissionReady = {
                            context.checkSelfPermission(Manifest.permission.CAMERA) ==
                                PackageManager.PERMISSION_GRANTED
                        },
                        foreground = { true },
                        streaming = { false },
                        stopCapture = { _ -> },
                        startCapture = { error("the TLS gate must not start a camera") },
                    )
                }
                sessions.add(session to connected)
            },
            onStatus = {},
            onError = { error -> errors.add(error) },
            capabilities = { snapshot },
            onQuality = { _, command ->
                qualityCommands.add(command)
                controller?.handle(command)
            },
        )
        server.start()
        // Test-only: the CI client needs the real per-install credential the
        // same way the real desktop user reads it off the phone screen.
        android.util.Log.i("SecureWifiGate", "READY SHA256=" + pin + " KEY=" + identity.pairingCode())
        try {
            // An unauthorized HELLO must be rejected before any session exists.
            val rejected = errors.poll(20, TimeUnit.SECONDS)
            check(rejected != null && rejected.contains("rejected")) {
                "wrong access key was not rejected: $rejected"
            }
            // The external client attempts the wrong key and then the right one
            // back to back, so a *registered* session may already exist here. The
            // property is that the wrong key created no unauthorized session, and
            // only the authorized client's own attempt may succeed.
            check(sessions.all { it.second }) {
                "wrong access key created an unauthorized session: ${sessions.map { it.second }}"
            }
            // The authorized client then completes pairing through the real server.
            val authorized = sessions.poll(20, TimeUnit.SECONDS)
            check(authorized != null && authorized.second) {
                "authorized client did not connect; server errors observed: $errors"
            }
            check(sessions.isEmpty()) { "a single client created more than one session" }
            // The authorized client also sends a 14-byte PREPARE_QUALITY inside
            // the TLS session; the real server must dispatch it to the
            // controller and keep serving frames afterwards.
            val command = qualityCommands.poll(20, TimeUnit.SECONDS)
            check(command is Wire.HostCommand.Prepare) {
                "TLS server did not dispatch a quality command: $command"
            }
            val request = (command as Wire.HostCommand.Prepare).request
            check(
                request.catalogRevision == 7 && request.width == 1280 &&
                    request.height == 720 && request.targetFps == 30
            ) {
                "quality command lost its fields: $request"
            }
            check(errors.isEmpty()) { "quality dispatch reported an error: $errors" }
            // The client reads the rejection and then disconnects. Closing the
            // server first would drop the queued reply, so the gate waits for
            // the client's own end of the session instead of assuming a timing.
            val disconnected = sessions.poll(20, TimeUnit.SECONDS)
            check(disconnected != null && !disconnected.second) {
                "client did not finish the session after the rejection: $disconnected"
            }
            android.util.Log.i(
                "SecureWifiGate",
                "PASS native TLS 1.3 handshake, wrong-key rejection, authenticated AMB framing and quality command dispatch",
            )
        } finally {
            server.close()
            authorization.close()
        }
    }

    /** Real preferences + Keystore, isolated from the application's approvals. */
    private fun smokeClientStorage() {
        val namespace = "client_authorization_instrumentation"
        val prefs = targetContext.getSharedPreferences(namespace, Context.MODE_PRIVATE)
        val alias = "mobile_webcam_${namespace}_aes_v1"
        val keystore = java.security.KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        check(prefs.edit().clear().commit())
        keystore.deleteEntry(alias)
        try {
            val fixture = context.assets.open("client-signature-v1.bin").use { it.readBytes() }
            val publicKey = fixture.copyOfRange(0, 65)
            val id = ClientAuthorization.deriveClientId(publicKey)
            val now = 1_800_000_000L
            val first = ClientStore(targetContext, namespace).repository
            check(first.snapshot().isEmpty())
            first.approve(id, publicKey, "studio-pc", now)
            val ciphertext = checkNotNull(prefs.getString(ClientStore.RECORD, null))
            check(prefs.all.keys == setOf(ClientStore.RECORD)) { "unexpected plaintext authorization fields" }
            val plaintext = ClientRecordCodec.encode(first.snapshot())
            check(ciphertext != android.util.Base64.encodeToString(plaintext, android.util.Base64.NO_WRAP))
            var reloaded = ClientStore(targetContext, namespace).repository
            check(reloaded.snapshot() == first.snapshot()) { "Keystore authorization round trip failed" }
            reloaded.renew(id, now + 10)
            reloaded = ClientStore(targetContext, namespace).repository
            check(reloaded.snapshot().single().expiresAtUnixSeconds == now + 10 + ClientAuthorization.EXPIRY_SECONDS)
            reloaded.revoke(id)
            // Explicit recovery: the confirmed reset clears the record *and* this
            // namespace's key, so the store is pristine and every PC must be
            // approved again. Partial states already fail closed (a record without
            // its key cannot decrypt; a key without its record is "recovery
            // required"), so this asserts only the deliberate path.
            ClientStore.clear(targetContext, namespace)
            check(!prefs.contains(ClientStore.RECORD)) { "reset left the authorization record" }
            check(!java.security.KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.containsAlias(alias)) {
                "reset left the authorization encryption key"
            }
            val pristine = ClientStore(targetContext, namespace).repository
            check(pristine.snapshot().isEmpty()) { "a reset store must read as never-used" }
            pristine.approve(id, publicKey, "after-reset", now)
            val afterReset = ClientStore(targetContext, namespace).repository
            check(afterReset.snapshot().single().label == "after-reset") {
                "a store must work normally after an explicit reset"
            }
            afterReset.revoke(id)
            check(ClientStore(targetContext, namespace).repository.snapshot().isEmpty())
            // The fence, against a real Keystore: `reloaded` was created before the
            // reset, so it may no longer write. That write is exactly what
            // re-created the erased record on the device and made the recovery
            // action look ineffective, so this is the assertion, not a formality.
            check(runCatching { reloaded.approve(id, publicKey, "stale", now) }.isFailure) {
                "a store created before the reset published after it"
            }
            check(runCatching { reloaded.snapshot() }.isFailure) {
                "a writer the fence refused must latch shut, not keep serving"
            }
            ClientStore(targetContext, namespace).repository.approve(id, publicKey, "studio-pc", now)
            val valid = checkNotNull(prefs.getString(ClientStore.RECORD, null))
            check(prefs.edit().remove(ClientStore.RECORD).commit())
            check(runCatching { ClientStore(targetContext, namespace) }.isFailure) {
                "lost authorization record with a surviving key was treated as first use"
            }
            check(keystore.containsAlias(alias) && !prefs.contains(ClientStore.RECORD)) {
                "lost record/key state was silently repaired"
            }
            check(prefs.edit().putString(ClientStore.RECORD, valid).commit())
            check(ClientStore(targetContext, namespace).repository.snapshot().single().label == "studio-pc") {
                "the authorization record did not round trip after a reset"
            }
            // The case the recovery action actually meets: a record that cannot be
            // decrypted. It must be clearable *without* reading it — constructing a
            // store reads it and throws, which is what made the action unreachable
            // on the device even though the operation itself was correct.
            val corrupt = android.util.Base64.decode(valid, android.util.Base64.NO_WRAP)
            corrupt[corrupt.lastIndex] = (corrupt.last().toInt() xor 1).toByte()
            check(prefs.edit().putString(ClientStore.RECORD,
                android.util.Base64.encodeToString(corrupt, android.util.Base64.NO_WRAP)).commit())
            check(runCatching { ClientStore(targetContext, namespace) }.isFailure) { "a corrupt record was read" }
            ClientStore.clear(targetContext, namespace)
            check(!prefs.contains(ClientStore.RECORD)) { "clearing a corrupt record left it behind" }
            check(!java.security.KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
                .containsAlias(alias)) { "clearing a corrupt record left its key behind" }
            check(ClientStore(targetContext, namespace).repository.snapshot().isEmpty()) {
                "a store cleared of a corrupt record must read as never-used"
            }
            ClientStore(targetContext, namespace).repository.approve(id, publicKey, "after-corrupt", now)
            check(prefs.edit().putString(ClientStore.RECORD, valid).commit())
            val changed = android.util.Base64.decode(valid, android.util.Base64.NO_WRAP)
            changed[changed.lastIndex] = (changed.last().toInt() xor 1).toByte()
            check(prefs.edit().putString(ClientStore.RECORD,
                android.util.Base64.encodeToString(changed, android.util.Base64.NO_WRAP)).commit())
            check(runCatching { ClientStore(targetContext, namespace) }.isFailure) {
                "tampered authorization ciphertext was accepted"
            }
            check(prefs.edit().putString(ClientStore.RECORD, valid).commit())
            keystore.deleteEntry(alias)
            check(runCatching { ClientStore(targetContext, namespace) }.isFailure) {
                "missing Keystore key silently reset the authorization list"
            }
            check(!keystore.containsAlias(alias)) { "decryption unexpectedly generated a replacement key" }
            check(prefs.getString(ClientStore.RECORD, null) == valid) { "corrupt storage was overwritten" }
        } finally {
            prefs.edit().clear().commit()
            keystore.deleteEntry(alias)
        }
    }

    /** Exercise the actual Android EC provider, not only a desktop JVM. The
     * leading-zero fixture used to produce a nonminimal DER INTEGER. */
    private fun smokeClientSignatures() {
        for (name in listOf("client-signature-v1.bin", "client-signature-leading-zero-v1.bin")) {
            val fixture = context.assets.open(name).use { it.readBytes() }
            check(fixture.size == 186) { "unexpected signature fixture shape: $name" }
            val publicKey = fixture.copyOfRange(0, 65)
            val message = fixture.copyOfRange(65, 122)
            val signature = fixture.copyOfRange(122, 186)
            if (name.contains("leading-zero")) {
                check(signature[0] == 0.toByte() && signature[1].toInt() in 1 until 0x80)
            }
            check(ClientAuthorization.verify(publicKey, message, signature)) {
                "Android EC provider rejected the Rust signature: $name"
            }
            for (index in listOf(0, 16, 48, 56)) {
                val changed = message.copyOf()
                changed[index] = (changed[index].toInt() xor 1).toByte()
                check(!ClientAuthorization.verify(publicKey, changed, signature)) {
                    "Android EC provider accepted a changed transcript: $name byte $index"
                }
            }
        }
        val point = context.assets.open("client-signature-v1.bin").use { it.readBytes() }.copyOfRange(0, 65)
        val transcript = context.assets.open("client-auth-transcript-v2.bin").use { it.readBytes() }
        check(Wire.clientAuthMessage(Wire.AUTH_LAN, ByteArray(32) { (it + 32).toByte() }, ByteArray(32) { it.toByte() }, point, "studio-pc").contentEquals(transcript))
        val usbTranscript = context.assets.open("client-auth-transcript-usb-v2.bin").use { it.readBytes() }
        check(Wire.clientAuthMessage(Wire.AUTH_USB, ByteArray(32), ByteArray(32) { it.toByte() }, point, "studio-pc").contentEquals(usbTranscript))
        android.util.Log.i("SecureWifiGate", "PASS Android EC provider signature fixtures and LAN/USB v2 transcripts")
    }

    override fun onStart() {
        val context = targetContext
        val received = LinkedBlockingQueue<Intent>()
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                received.add(intent)
            }
        }
        val filter = IntentFilter(MainActivity.ACTION_USB_PERMISSION)
        if (Build.VERSION.SDK_INT >= 33) {
            context.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            context.registerReceiver(receiver, filter)
        }
        try {
            val emulator = Build.HARDWARE.contains("ranchu") || Build.PRODUCT.startsWith("sdk_gphone")
            // Everything below writes the app's own authorization store and drives
            // its UI, so it needs a profile the caller has cleared. Two explicit
            // opt-ins satisfy that and nothing does it by default, because a real
            // phone carries its owner's approvals:
            //
            //   -e disposableUi true   a disposable API 35 emulator, and
            //   -e physicalDevice true a physical device the caller designated.
            //
            // The pristine-profile check further down is the second gate: a device
            // with any stored approval is refused even with the flag.
            val designated = (disposableUi && emulator && Build.VERSION.SDK_INT >= 35) ||
                (physicalDevice && !emulator && Build.VERSION.SDK_INT >= 29)
            check(designated) {
                "Conformance requires an explicitly disposable API 35 emulator (-e disposableUi true) " +
                    "or a designated physical device (-e physicalDevice true)"
            }
            if (cameraAcceptance) {
                // No preamble here: the Wi-Fi smoke test expects an external TLS
                // client that this invocation does not run.
                val marker = CameraTeardownInstrumentation(this, emulator).run()
                finish(Activity.RESULT_OK, Bundle().apply { putString("stream", "$marker\n") })
                return
            }
            if (usbReaderMeasurement) {
                // Also preamble-free, and for the same reason: this measures the endpoint
                // rather than the app, uses an in-memory repository, and must not run against
                // a profile the other modes would refuse. It does need the accessory to be
                // attached and this app to hold the USB permission for it.
                val marker = UsbReaderReleaseMeasurement(this).run()
                finish(Activity.RESULT_OK, Bundle().apply { putString("stream", marker) })
                return
            }
            check(context.getSharedPreferences("secure_wifi", Context.MODE_PRIVATE).all.isEmpty() &&
                ClientStore(context).repository.snapshot().isEmpty()) { "Refusing to use an existing app profile" }
            val stale = MainActivity.createUsbPermissionRequest(context, "old-activity")
            val replacement = MainActivity.createUsbPermissionRequest(context, "new-activity")
            check(runCatching { stale.send() }.exceptionOrNull() is PendingIntent.CanceledException) {
                "Old Activity request survived replacement"
            }
            for (id in listOf("new-activity", "next-request")) {
                val request = if (id == "new-activity") replacement else MainActivity.createUsbPermissionRequest(context, id)
                request.send(context, 0, Intent().putExtra(UsbManager.EXTRA_PERMISSION_GRANTED, true))
                val result = checkNotNull(received.poll(5, TimeUnit.SECONDS)) { "Callback not delivered" }
                check(result.getStringExtra(MainActivity.EXTRA_USB_REQUEST_ID) == id) { "Request identity lost" }
                check(!result.hasExtra(UsbManager.EXTRA_PERMISSION_GRANTED)) { "Immutable request received fill-in extras" }
                check(runCatching { request.send() }.exceptionOrNull() is PendingIntent.CanceledException) {
                    "USB request could be replayed"
                }
            }
            smokeClientStorage()
            smokeClientSignatures()
            if (Build.VERSION.SDK_INT >= 29) smokeSecureWifi(context)
            ClientAuthorizationUiInstrumentation(this, allowDisplayChanges = physicalDevice).run()
            finish(Activity.RESULT_OK, Bundle().apply {
                putString("stream", "PASS USB permission, encrypted client storage, EC signature fixtures, " +
                    "Android Keystore TLS 1.3, wrong-key rejection, host handshake, approval UI and live revocation" +
                    if (emulator) "\n" else " (physical device)\n")
            })
        } catch (error: Throwable) {
            android.util.Log.e("SecureWifiGate", "Instrumentation failure", error)
            finish(Activity.RESULT_CANCELED, Bundle().apply { putString("stream", "FAIL: $error\n${error.stackTraceToString()}\n") })
        } finally {
            context.unregisterReceiver(receiver)
        }
    }
}
