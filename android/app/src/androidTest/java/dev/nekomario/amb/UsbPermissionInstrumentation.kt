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
import java.security.MessageDigest
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/** Exercises the platform notification boundary without pretending a USB device is attached. */
class UsbPermissionInstrumentation : Instrumentation() {
    override fun onCreate(arguments: Bundle?) {
        super.onCreate(arguments)
        start()
    }

    /**
     * Runs on the Android emulator while GitHub Actions connects to this
     * socket from the host via adb forward. Using an independent desktop TLS
     * stack tests the same Android Keystore server path as the Rust receiver.
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
        val fixedTestKey = SecureLanIdentity.decodeHex("00112233445566778899aabbccddeeff")
        identity.listener(48_527).use { listener ->
            listener.soTimeout = 15_000
            android.util.Log.i("SecureWifiGate", "READY SHA256=" + pin)
            (listener.accept() as javax.net.ssl.SSLSocket).use { peer ->
                peer.soTimeout = 8_000
                peer.useClientMode = false
                peer.enabledProtocols = arrayOf("TLSv1.3")
                peer.startHandshake()
                check(peer.session.protocol == "TLSv1.3")
                val (header, credential) = Wire.readFrame(peer.inputStream, 16)
                check(header.type == Wire.TYPE_HELLO && header.sequence == 1)
                check(MessageDigest.isEqual(credential, fixedTestKey))
                Wire.writeFrame(
                    peer.outputStream,
                    Wire.Header(Wire.TYPE_HELLO_ACK, 0, 1, 0, 0),
                    ByteArray(0),
                )
                android.util.Log.i("SecureWifiGate", "PASS native TLS 1.3 handshake and authenticated AMB framing")
            }
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
                putString("stream", "PASS USB permission and Android Keystore TLS 1.3 host handshake\n")
            })
        } catch (error: Throwable) {
            finish(Activity.RESULT_CANCELED, Bundle().apply { putString("stream", "FAIL: $error\n") })
        } finally {
            context.unregisterReceiver(receiver)
        }
    }
}
