package dev.nekomario.amb

import android.content.Context
import android.os.Build
import android.util.Base64
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.math.BigInteger
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.X509Certificate
import java.security.spec.ECGenParameterSpec
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import java.util.Date
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLServerSocket
import javax.security.auth.x500.X500Principal

/** Device identity is hardware-backed when the handset supports it.
 * Credential rotation invalidates every previously authorized desktop.
 */
class SecureLanIdentity(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences("secure_wifi", Context.MODE_PRIVATE)
    @Volatile private var secret: String = loadOrCreateSecret()

    private fun loadOrCreateSecret(): String {
        val encrypted = prefs.getString(ENCRYPTED_SECRET, null)
        if (encrypted != null) {
            val bytes = Base64.decode(encrypted, Base64.NO_WRAP)
            require(bytes.size in 29..128) { "invalid stored Wi-Fi credential length" }
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, encryptionKey(), GCMParameterSpec(128, bytes, 0, 12))
            val candidate = String(cipher.doFinal(bytes.copyOfRange(12, bytes.size)), Charsets.US_ASCII)
            require(isHex(candidate, 32)) { "invalid decrypted Wi-Fi credential" }
            return candidate
        }
        // One-way migration of the original unencrypted development-build preference.
        val migrated = prefs.getString("secret", null)?.takeIf { isHex(it, 32) }
        return (migrated ?: newSecret()).also { persist(it) }
    }

    private fun encryptionKey(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val existing = store.getKey(ENCRYPTION_ALIAS, null)
        if (existing != null) return existing as SecretKey
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(
                ENCRYPTION_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return generator.generateKey()
    }

    private fun persist(next: String) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, encryptionKey())
        val ciphertext = cipher.doFinal(next.toByteArray(Charsets.US_ASCII))
        val encoded = Base64.encodeToString(cipher.iv + ciphertext, Base64.NO_WRAP)
        check(prefs.edit().putString(ENCRYPTED_SECRET, encoded).remove("secret").commit()) {
            "unable to protect Wi-Fi credential"
        }
    }

    fun pairingCode(): String = secret

    fun rotate() {
        val next = newSecret()
        persist(next)
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
            useClientMode = false
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
        private const val ENCRYPTION_ALIAS = "mobile_webcam_wifi_secret_aes_v1"
        private const val ENCRYPTED_SECRET = "encrypted_secret_v1"
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
