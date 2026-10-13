package dev.nekomario.amb

import android.app.Activity
import android.app.Instrumentation
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.Bundle
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/** Exercises the platform notification boundary without pretending a USB device is attached. */
class UsbPermissionInstrumentation : Instrumentation() {
    override fun onCreate(arguments: Bundle?) {
        super.onCreate(arguments)
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
        val server = LanServer(
            identity = identity,
            port = 48_527,
            onSession = { session, connected -> sessions.add(session to connected) },
            onStatus = {},
            onError = { error -> errors.add(error) },
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
            check(sessions.isEmpty) { "wrong access key created a session" }
            // The authorized client then completes pairing through the real server.
            val authorized = sessions.poll(20, TimeUnit.SECONDS)
            check(authorized != null && authorized.second) {
                "authorized client did not connect; server errors observed: $errors"
            }
            android.util.Log.i(
                "SecureWifiGate",
                "PASS native TLS 1.3 handshake, wrong-key rejection and authenticated AMB framing",
            )
        } finally {
            server.close()
        }
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
            if (Build.VERSION.SDK_INT >= 29) smokeSecureWifi(context)
            finish(Activity.RESULT_OK, Bundle().apply {
                putString("stream", "PASS USB permission, Android Keystore TLS 1.3, wrong-key rejection and host handshake\n")
            })
        } catch (error: Throwable) {
            finish(Activity.RESULT_CANCELED, Bundle().apply { putString("stream", "FAIL: $error\n") })
        } finally {
            context.unregisterReceiver(receiver)
        }
    }
}
