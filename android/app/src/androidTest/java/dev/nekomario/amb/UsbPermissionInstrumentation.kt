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
import java.security.SecureRandom
import java.security.cert.X509Certificate
import java.util.Collections
import java.util.concurrent.atomic.AtomicReference
import javax.net.ssl.KeyManagerFactory
import javax.net.ssl.SSLContext
import javax.net.ssl.X509KeyManager
import javax.net.ssl.SSLSocket
import javax.net.ssl.X509TrustManager
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/** Exercises the platform notification boundary without pretending a USB device is attached. */
class UsbPermissionInstrumentation : Instrumentation() {
    override fun onCreate(arguments: Bundle?) {
        super.onCreate(arguments)
        start()
    }

    private fun smokeSecureWifi(context: Context) {
        val identity = SecureLanIdentity(context)
        val stored = context.getSharedPreferences("secure_wifi", Context.MODE_PRIVATE)
        check(!stored.contains("secret")) { "plaintext Wi-Fi credential remains on disk" }
        check(stored.contains("encrypted_secret_v1")) { "encrypted Wi-Fi credential missing" }
        check(SecureLanIdentity(context).pairingCode() == identity.pairingCode()) {
            "Wi-Fi access key did not persist"
        }
        val expectedFingerprint = identity.fingerprint()
        check(expectedFingerprint.length == 64)

        identity.listener(0).use { listener ->
            listener.soTimeout = 4_000
            val timeline = Collections.synchronizedList(mutableListOf<String>())
            val testStore = java.security.KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
            val testManagers = KeyManagerFactory.getInstance(
                KeyManagerFactory.getDefaultAlgorithm()
            ).apply { init(testStore, null) }
            val aliases = testManagers.keyManagers.filterIsInstance<X509KeyManager>()
                .map { it.chooseServerAlias("EC", null, null) ?: "none" }
            timeline.add("KeyManagers EC aliases=$aliases; listener clientMode=${listener.useClientMode}")
            val serverFailure = AtomicReference<Throwable?>(null)
            val worker = Thread {
                try {
                    (listener.accept() as SSLSocket).use { peer ->
                        timeline.add("Server accepted mode=${peer.useClientMode}, protocols=${peer.enabledProtocols.joinToString()}")
                        peer.useClientMode = false
                        peer.enabledProtocols = arrayOf("TLSv1.3")
                        peer.soTimeout = 4_000
                        timeline.add("Server about to handshake")
                        peer.startHandshake()
                        timeline.add("Server handshake established")
                        val (header, secret) = Wire.readFrame(peer.inputStream, 16)
                        check(header.type == Wire.TYPE_HELLO && header.sequence == 1)
                        check(MessageDigest.isEqual(
                            secret,
                            SecureLanIdentity.decodeHex(identity.pairingCode())
                        ))
                        Wire.writeFrame(
                            peer.outputStream,
                            Wire.Header(Wire.TYPE_HELLO_ACK, 0, 1, 0, 0),
                            ByteArray(0)
                        )
                    }
                } catch (failure: Throwable) {
                    serverFailure.set(failure)
                }
            }
            worker.start()

            // This trust-all manager exists ONLY in the isolated loopback
            // instrumentation client. Production Rust verifies the SHA-256 pin
            // and TLS signature before sending a secret.
            val testTrust = object : X509TrustManager {
                override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
                override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) {}
                override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {}
            }
            val tls = SSLContext.getInstance("TLS")
            tls.init(null, arrayOf(testTrust), SecureRandom())
            try {
                (tls.socketFactory.createSocket("127.0.0.1", listener.localPort) as SSLSocket).use { client ->
                timeline.add("Client created mode=${client.useClientMode}, protocols=${client.enabledProtocols.joinToString()}")
                client.useClientMode = true
                client.enabledProtocols = arrayOf("TLSv1.3")
                client.soTimeout = 4_000
                timeline.add("Client about to handshake")
                client.startHandshake()
                timeline.add("Client handshake established")
                val certificate = client.session.peerCertificates[0] as X509Certificate
                val actual = MessageDigest.getInstance("SHA-256")
                    .digest(certificate.encoded)
                    .joinToString("") { "%02X".format(it.toInt() and 0xff) }
                check(actual == expectedFingerprint) { "presented TLS certificate differs from phone display" }
                val credential = SecureLanIdentity.decodeHex(identity.pairingCode())
                Wire.writeFrame(client.outputStream, Wire.Header(Wire.TYPE_HELLO, 0, 1, 16, 0), credential)
                val (ack, payload) = Wire.readFrame(client.inputStream, 0)
                check(ack.type == Wire.TYPE_HELLO_ACK && ack.sequence == 1 && payload.isEmpty())
            }
            } catch (failure: Throwable) {
                worker.join(5_000)
                val serverCause = serverFailure.get()?.stackTraceToString() ?: "no server-side failure recorded"
                throw IllegalStateException(
                    "TLS client failed: " + failure.stackTraceToString() +
                    "\nServer result: " + serverCause +
                    "\nHandshake timeline: " + timeline.joinToString("; "),
                    failure,
                )
            }
            worker.join(5_000)
            check(!worker.isAlive) { "TLS server thread did not finish" }
            serverFailure.get()?.let { throw it }
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
                putString("stream", "PASS USB permission and Android Keystore TLS 1.3 loopback handshake\n")
            })
        } catch (error: Throwable) {
            finish(Activity.RESULT_CANCELED, Bundle().apply { putString("stream", "FAIL: $error\n") })
        } finally {
            context.unregisterReceiver(receiver)
        }
    }
}
