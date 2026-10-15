package dev.nekomario.amb

import java.math.BigInteger
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.Signature
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECPublicKeySpec
import java.security.spec.X509EncodedKeySpec

/**
 * Which PCs may use this phone's camera, and until when.
 *
 * The phone keeps a list of authorized clients. The physical plug or the
 * pairing bootstrap gets a client *registered*; only an explicit tap on this
 * screen grants it the camera. Nothing here decides on the PC's word: the
 * client ID is derived from the public key the phone stores, and a label the PC
 * proposes is never an identity.
 *
 * The list itself is the security boundary, so the transitions are small and
 * total: every input produces exactly one decision, and every decision is
 * expressible as a new list.
 */
object ClientAuthorization {
    /** Bounds both the phone's storage and the user's review burden. */
    const val MAX_CLIENTS = 8

    /** Ninety days: long enough that a desktop user is not nagged, short enough
     * that a forgotten PC stops working. */
    const val EXPIRY_SECONDS = 90L * 24 * 60 * 60

    const val CLIENT_ID_BYTES = Wire.CLIENT_ID_BYTES
    const val PUBLIC_KEY_BYTES = 65
    const val SIGNATURE_BYTES = Wire.SIGNATURE_BYTES

    enum class Decision { AUTHORIZED, PENDING, REJECTED }

    /** Why a client was rejected. Mirrors the wire reasons exactly. */
    enum class Reason(val code: Int) {
        NONE(Wire.AUTH_REASON_NONE),
        UNKNOWN(Wire.AUTH_REASON_UNKNOWN),
        REVOKED(Wire.AUTH_REASON_REVOKED),
        EXPIRED(Wire.AUTH_REASON_EXPIRED),
        TOO_MANY(Wire.AUTH_REASON_TOO_MANY),
        BAD_SIGNATURE(Wire.AUTH_REASON_BAD_SIGNATURE),
    }

    /** One authorized PC. The public key is the identity; the label is a
     * convenience string the PC proposed and the user may edit. */
    data class Client(
        val clientId: ByteArray,
        val publicKey: ByteArray,
        val label: String,
        val approvedAtUnixSeconds: Long,
        val expiresAtUnixSeconds: Long,
    ) {
        override fun equals(other: Any?) = other is Client &&
            clientId.contentEquals(other.clientId) &&
            publicKey.contentEquals(other.publicKey) &&
            label == other.label &&
            approvedAtUnixSeconds == other.approvedAtUnixSeconds &&
            expiresAtUnixSeconds == other.expiresAtUnixSeconds

        override fun hashCode(): Int {
            var result = clientId.contentHashCode()
            result = 31 * result + publicKey.contentHashCode()
            result = 31 * result + label.hashCode()
            result = 31 * result + approvedAtUnixSeconds.hashCode()
            result = 31 * result + expiresAtUnixSeconds.hashCode()
            return result
        }
    }

    data class Result(
        val decision: Decision,
        val reason: Reason,
        val expiresAtUnixSeconds: Long,
    ) {
        fun toWire(): Wire.ClientAuthResult = Wire.ClientAuthResult(
            when (decision) {
                Decision.AUTHORIZED -> Wire.AUTH_AUTHORIZED
                Decision.PENDING -> Wire.AUTH_PENDING
                Decision.REJECTED -> Wire.AUTH_REJECTED
            },
            reason.code,
            expiresAtUnixSeconds,
        )
    }

    /**
     * Decide what a connecting client gets.
     *
     * The signature is checked first: an unauthenticated client is rejected
     * whatever the list says, and its ID is never trusted as an identity.
     */
    fun evaluate(
        clientId: ByteArray,
        publicKey: ByteArray,
        message: ByteArray,
        signature: ByteArray,
        clients: List<Client>,
        nowUnixSeconds: Long,
    ): Result {
        if (clientId.size != CLIENT_ID_BYTES || publicKey.size != PUBLIC_KEY_BYTES ||
            signature.size != SIGNATURE_BYTES
        ) {
            return Result(Decision.REJECTED, Reason.BAD_SIGNATURE, 0)
        }
        // The ID is derived from the key, so a mismatch means the client lied
        // about who it is — the same class of failure as a bad signature.
        val derived = deriveClientId(publicKey)
        if (!derived.contentEquals(clientId) || !verify(publicKey, message, signature)) {
            return Result(Decision.REJECTED, Reason.BAD_SIGNATURE, 0)
        }
        val known = clients.firstOrNull { it.clientId.contentEquals(clientId) }
            ?: return if (clients.size >= MAX_CLIENTS) {
                Result(Decision.REJECTED, Reason.TOO_MANY, 0)
            } else {
                Result(Decision.PENDING, Reason.NONE, 0)
            }
        // A stored entry is authoritative even if the client re-signs with a
        // different key under the same ID: the ID is derived, so this cannot
        // happen without the stored key having changed.
        if (!known.publicKey.contentEquals(publicKey)) {
            return Result(Decision.REJECTED, Reason.BAD_SIGNATURE, 0)
        }
        return if (nowUnixSeconds >= known.expiresAtUnixSeconds) {
            Result(Decision.REJECTED, Reason.EXPIRED, known.expiresAtUnixSeconds)
        } else {
            Result(Decision.AUTHORIZED, Reason.NONE, known.expiresAtUnixSeconds)
        }
    }

    /** A user approved a pending client. Returns the new list, or null if the
     * list is already full — an approval that cannot be stored is refused, never
     * silently dropped. */
    fun approve(
        clients: List<Client>,
        clientId: ByteArray,
        publicKey: ByteArray,
        label: String,
        nowUnixSeconds: Long,
    ): List<Client>? {
        if (clients.none { it.clientId.contentEquals(clientId) } && clients.size >= MAX_CLIENTS) {
            return null
        }
        val entry = Client(
            clientId = clientId,
            publicKey = publicKey,
            label = label.take(MAX_LABEL_BYTES),
            approvedAtUnixSeconds = nowUnixSeconds,
            expiresAtUnixSeconds = nowUnixSeconds + EXPIRY_SECONDS,
        )
        return clients.filterNot { it.clientId.contentEquals(clientId) } + entry
    }

    /** A user revoked a client. Revoking the live session's client must also
     * end that session; the caller owns that, because it owns the socket. */
    fun revoke(clients: List<Client>, clientId: ByteArray): List<Client> =
        clients.filterNot { it.clientId.contentEquals(clientId) }

    /** Re-approving refreshes the expiry rather than stacking a second entry. */
    fun renew(clients: List<Client>, clientId: ByteArray, nowUnixSeconds: Long): List<Client> =
        clients.map {
            if (it.clientId.contentEquals(clientId)) {
                it.copy(
                    approvedAtUnixSeconds = nowUnixSeconds,
                    expiresAtUnixSeconds = nowUnixSeconds + EXPIRY_SECONDS,
                )
            } else {
                it
            }
        }

    /** The ID is a truncated hash of the public key, never chosen. */
    fun deriveClientId(publicKey: ByteArray): ByteArray =
        Wire.sha256(publicKey).copyOfRange(0, CLIENT_ID_BYTES)

    /**
     * Verify a signature against the public point the phone stored.
     *
     * The point is wrapped into a SubjectPublicKeyInfo so the platform's own
     * verifier parses it; building an ECParameterSpec by hand would mean
     * hardcoding the curve, and the DER prefix is a shorter constant to be
     * wrong about.
     */
    fun verify(publicKeyPoint: ByteArray, message: ByteArray, signature: ByteArray): Boolean =
        runCatching {
            val key = KeyFactory.getInstance("EC")
                .generatePublic(X509EncodedKeySpec(EC_SPKI_PREFIX + publicKeyPoint))
            val verifier = Signature.getInstance("SHA256withECDSA")
            verifier.initVerify(key)
            verifier.update(message)
            // The wire carries a fixed 64-byte r||s; the platform verifies DER.
            verifier.verify(rawSignatureToDer(signature))
        }.getOrDefault(false)

    /**
     * Wrap a fixed-width r||s signature in the DER sequence the platform wants.
     *
     * The halves are fixed-width, so a leading zero byte is significant and is
     * never stripped; a zero is only *added* when the high bit is set, because
     * DER integers are signed.
     */
    internal fun rawSignatureToDer(signature: ByteArray): ByteArray {
        require(signature.size == SIGNATURE_BYTES) { "signature must be 64 raw bytes" }
        fun integer(half: ByteArray): ByteArray {
            val body = if ((half[0].toInt() and 0x80) != 0) byteArrayOf(0) + half else half
            return byteArrayOf(0x02, body.size.toByte()) + body
        }
        val contents = integer(signature.copyOfRange(0, 32)) +
            integer(signature.copyOfRange(32, 64))
        return byteArrayOf(0x30, contents.size.toByte()) + contents
    }

    /** The message the wire contract signs: clientId, then nonce, then label. */
    fun signedMessage(clientId: ByteArray, nonce: ByteArray, label: String): ByteArray =
        clientId + nonce + label.toByteArray(Charsets.UTF_8)

    const val MAX_LABEL_BYTES = Wire.MAX_CLIENT_LABEL

    /** SubjectPublicKeyInfo prefix for an uncompressed P-256 public key. */
    private val EC_SPKI_PREFIX = byteArrayOf(
        0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86.toByte(), 0x48, 0xce.toByte(),
        0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86.toByte(), 0x48, 0xce.toByte(),
        0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
    )

    /** Test-only: a platform keypair, so a signature can be produced on the JVM. */
    fun generateTestKeyPair(): Pair<ByteArray, java.security.PrivateKey> {
        val generator = KeyPairGenerator.getInstance("EC")
        generator.initialize(ECGenParameterSpec("secp256r1"))
        val pair = generator.generateKeyPair()
        val point = (pair.public as java.security.interfaces.ECPublicKey).w
        val encoded = byteArrayOf(0x04) +
            point.affineX.toByteArray().let { bytes ->
                // Strip the sign byte BigInteger adds, then left-pad to 32.
                val raw = bytes.dropWhile { it == 0.toByte() }.takeLast(32).toByteArray()
                ByteArray(32 - raw.size) + raw
            } +
            point.affineY.toByteArray().let { bytes ->
                val raw = bytes.dropWhile { it == 0.toByte() }.takeLast(32).toByteArray()
                ByteArray(32 - raw.size) + raw
            }
        return encoded to pair.private
    }

    fun signForTest(privateKey: java.security.PrivateKey, message: ByteArray): ByteArray =
        Signature.getInstance("SHA256withECDSA").run {
            initSign(privateKey)
            update(message)
            sign()
        }
}
