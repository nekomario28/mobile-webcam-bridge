package dev.nekomario.amb

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Android persistence adapter. Separate alias/AEAD context from the Wi-Fi
 * bearer key, which has its own existing storage format and migration policy. */
class ClientStore(context: Context, private val namespace: String = "client_authorization") {
    private val context = context.applicationContext
    private val prefs = this.context.getSharedPreferences(namespace, Context.MODE_PRIVATE)
    private val alias = aliasFor(namespace)
    private val aad = "AMB authorized clients v1:$namespace".toByteArray(Charsets.UTF_8)
    /** The epoch this store was created in; a reset makes it stale. */
    private val born = StorageGeneration.current()
    val repository = ClientRepository(::load, ::save, { StorageGeneration.requireCurrent(born) })

    private fun key(create: Boolean): SecretKey = synchronized(keyLock) {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val existing = store.getKey(alias, null)
        if (existing != null) return@synchronized existing as SecretKey
        check(create) { "authorization encryption key is missing; stored data preserved" }
        KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
            init(KeyGenParameterSpec.Builder(alias,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256).build())
            generateKey()
        }
    }

    /**
     * Explicit recovery: delete the encrypted record and this namespace's key.
     *
     * Only the confirmed recovery action calls this; `load` never does, so an
     * unreadable record still fails closed. Afterwards the store is pristine and
     * every approved PC must be approved again. Partial failure also fails closed:
     * a record without its key cannot decrypt, and a key without its record is
     * already treated as "recovery required" rather than as first use.
     */
    fun reset() = clear(context, namespace)

    private fun load(): ByteArray? {
        if (!prefs.contains(RECORD)) {
            // SharedPreferences can report missing after XML loss. An existing
            // namespace key proves this was not a pristine, never-written list.
            synchronized(keyLock) {
                check(!KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.containsAlias(alias)) {
                    "authorization record is missing but its encryption key remains; recovery required"
                }
            }
            return null
        }
        val encoded = checkNotNull(prefs.getString(RECORD, null)) { "invalid stored authorization type" }
        require(encoded.length <= 2048) { "encrypted authorization list exceeds limit" }
        val bytes = Base64.decode(encoded, Base64.NO_WRAP)
        require(bytes.size in 30..(ClientRecordCodec.MAX_BYTES + 29) && bytes[0] == 1.toByte()) {
            "invalid encrypted authorization record"
        }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(create = false), GCMParameterSpec(128, bytes, 1, 12))
        cipher.updateAAD(aad)
        return cipher.doFinal(bytes.copyOfRange(13, bytes.size))
    }

    private fun save(plaintext: ByteArray) {
        require(plaintext.size <= ClientRecordCodec.MAX_BYTES)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key(create = true))
        check(cipher.iv.size == 12) { "unexpected GCM nonce width" }
        cipher.updateAAD(aad)
        val bytes = byteArrayOf(1) + cipher.iv + cipher.doFinal(plaintext)
        check(prefs.edit().putString(RECORD, Base64.encodeToString(bytes, Base64.NO_WRAP)).commit()) {
            "unable to persist encrypted authorization list"
        }
    }

    companion object {
        internal const val RECORD = "authorized_clients_v1"
        private val keyLock = Any()

        private fun aliasFor(namespace: String) = "mobile_webcam_${namespace}_aes_v1"

        /**
         * Clear the record and this namespace's key **without reading either**.
         *
         * Recovery must not be reachable only through an object whose construction
         * reads what cannot be read. Constructing a store decrypts the record, so
         * `ClientStore(context).reset()` threw `AEADBadTagException` before it could
         * clear anything — the device run for the recovery action failed exactly
         * there, with the record and its key still in place afterwards.
         *
         * Bumping the epoch first means no writer from before the reset can slip in
         * between the clear and the bump and re-create what the user erased.
         */
        fun clear(context: Context, namespace: String = "client_authorization") {
            synchronized(keyLock) {
                StorageGeneration.bump()
                val prefs = context.applicationContext
                    .getSharedPreferences(namespace, Context.MODE_PRIVATE)
                check(prefs.edit().remove(RECORD).commit()) { "unable to clear the authorization record" }
                val failure = runCatching {
                    KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
                        .deleteEntry(aliasFor(namespace))
                }.exceptionOrNull()
                failure?.let {
                    throw IllegalStateException("unable to clear the authorization encryption key", it)
                }
            }
        }
    }
}
