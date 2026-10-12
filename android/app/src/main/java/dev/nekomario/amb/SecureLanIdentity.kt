package dev.nekomario.amb

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.math.BigInteger
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.X509Certificate
import java.security.spec.ECGenParameterSpec
import java.util.Date
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLServerSocket
import javax.security.auth.x500.X500Principal

/** Device identity is hardware-backed when the handset supports it.
 * Credential rotation invalidates every previously authorized desktop.
 */
class SecureLanIdentity(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences("secure_wifi", Context.MODE_PRIVATE)
    @Volatile private var secret = prefs.getString("secret", null)
        ?.takeIf { isHex(it, 32) } ?: newSecret().also {
            check(prefs.edit().putString("secret", it).commit()) { "unable to save Wi-Fi secret" }
        }

    fun pairingCode(): String = secret

    fun rotate() {
        val next = newSecret()
        check(prefs.edit().putString("secret", next).commit()) { "unable to rotate Wi-Fi secret" }
        secret = next
    }

    fun authenticates(candidate: ByteArray): Boolean =
        MessageDigest.isEqual(candidate, decodeHex(secret))

    fun fingerprint(): String {
        val cert = keyStore().getCertificate(KEY_ALIAS) as X509Certificate
        return hex(MessageDigest.getInstance("SHA-256").digest(cert.encoded))
    }

    fun listener(port: Int): SSLServerSocket {
        require(Build.VERSION.SDK_INT >= 29) { "Secure Wi-Fi requires Android 10 or later; USB remains available" }
        val store = keyStore()
        val manager = javax.net.ssl.KeyManagerFactory.getInstance(
            javax.net.ssl.KeyManagerFactory.getDefaultAlgorithm()
        )
        manager.init(store, null)
        val tls = SSLContext.getInstance("TLS")
        tls.init(manager.keyManagers, null, SecureRandom())
        return (tls.serverSocketFactory.createServerSocket(port) as SSLServerSocket).apply {
            enabledProtocols = arrayOf("TLSv1.3")
            reuseAddress = true
        }
    }

    private fun keyStore(): KeyStore {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        if (!store.containsAlias(KEY_ALIAS)) {
            val generator = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, "AndroidKeyStore")
            generator.initialize(
                KeyGenParameterSpec.Builder(
                    KEY_ALIAS, KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY
                )
                    .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                    .setDigests(KeyProperties.DIGEST_SHA256)
                    .setCertificateSubject(X500Principal("CN=Mobile Webcam"))
                    .setCertificateSerialNumber(BigInteger.ONE)
                    .setCertificateNotBefore(Date(1_700_000_000_000L))
                    .setCertificateNotAfter(Date(4_000_000_000_000L))
                    .build()
            )
            generator.generateKeyPair()
        }
        return store
    }

    companion object {
        private const val KEY_ALIAS = "mobile_webcam_wifi_tls_v1"
        internal fun isHex(value: String, size: Int): Boolean =
            value.length == size && value.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }

        internal fun decodeHex(value: String): ByteArray {
            require(value.length % 2 == 0 && isHex(value, value.length))
            return ByteArray(value.length / 2) {
                value.substring(it * 2, it * 2 + 2).toInt(16).toByte()
            }
        }

        private fun hex(bytes: ByteArray): String =
            bytes.joinToString("") { "%02X".format(it.toInt() and 0xff) }

        private fun newSecret(): String =
            ByteArray(16).also { SecureRandom().nextBytes(it) }.let(::hex)
    }
}
